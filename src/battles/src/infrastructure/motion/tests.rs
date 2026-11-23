use super::*;
use crate::application::{BattleParticipant, VehicleCommand, VehicleControl};
use crate::infrastructure::{delivery::QueuedDelivery, worker::BattleOutput};
use opentelemetry::trace::{Status, TracerProvider};
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider, SpanData};
use std::net::SocketAddrV4;
use tracing_subscriber::prelude::*;
use wot_platform::mercury::script;
use wot_shared_kernel::{GunLoading, GunPerformance, Mobility, VehiclePerformance};

fn has_field(span: &SpanData, key: &str, expected: &str) -> bool {
    span.attributes
        .iter()
        .any(|attribute| attribute.key.as_str() == key && attribute.value.to_string() == expected)
}

fn child_of(span: &SpanData, parent: &SpanData) -> bool {
    span.parent_span_id == parent.span_context.span_id()
        && span.span_context.trace_id() == parent.span_context.trace_id()
}

#[test]
#[ignore = "Requires WG_RESOURCE_DIR with the extracted EU client bundle"]
fn drive_contact_moves_the_other_tank_and_parents_its_position_delivery() {
    let path = std::env::var_os("WG_RESOURCE_DIR").unwrap();
    let resources = ResourceBundle::open(std::path::Path::new(&path)).unwrap();
    let dispatch = Arc::new(ScriptDispatch::new(script::load(&resources).unwrap()));
    let mut runtime = BattleRuntime::load(dispatch, &resources, true).unwrap();
    let arena = (1 << 16) | 5;
    let map = runtime.maps.get_mut(&arena).unwrap().as_mut().unwrap();
    let start = map.spawns[0];
    let forward = (map.spawns[1] - start).normalize();
    map.spawns[1] = start + forward * 5.03;
    let vehicle = || VehicleLoadout {
        descriptor: vec![1],
        outfit: Vec::new(),
        health: 100,
        crew_size: 3,
        ammunition: Vec::new(),
        performance: Some(VehiclePerformance {
            forward_speed: 12.0,
            reverse_speed: 4.0,
            hull_traverse: 0.7,
            mobility: Mobility::default(),
            turret_pivot: None,
            gun: GunPerformance {
                turret_traverse: 0.8,
                gun_traverse: 0.0,
                yaw_limits: None,
                min_pitch: vec![(-180.0, -0.1), (180.0, -0.1)],
                max_pitch: vec![(-180.0, 0.2), (180.0, 0.2)],
                aiming_time: 1.0,
                base_dispersion_angle: None,
                shot_position: None,
                loading: GunLoading {
                    capacity: 1,
                    reload_time: 3.0,
                    intra_clip_time: 0.0,
                    forced_reload_time: 0.0,
                    autoreload_times: Vec::new(),
                },
                shell_damage: Default::default(),
                shell_ranges: Default::default(),
                shell_effects: Default::default(),
            },
        }),
    };
    let prepared = runtime
        .prepare(
            BattleLaunch {
                arena_type: arena,
                duration: Duration::from_secs(60),
                mode: BattleMode::Training,
                participants: [1, 2]
                    .into_iter()
                    .map(|team| BattleParticipant {
                        id: ParticipantId(u64::from(team)),
                        account_id: i64::from(team),
                        name: format!("Player {team}"),
                        team,
                        is_host: team == 1,
                        observer: false,
                        customization_display_type: 0,
                        vehicle: vehicle(),
                    })
                    .collect(),
            },
            vec![
                BattleEntityIds {
                    avatar: 11,
                    vehicle: 12,
                    info: 13,
                },
                BattleEntityIds {
                    avatar: 21,
                    vehicle: 22,
                    info: 23,
                },
            ],
        )
        .unwrap()
        .unwrap();
    let mut simulation = BattleSimulation {
        active: Default::default(),
        engagements: Default::default(),
        wire: runtime.wire.clone(),
    };
    let id = prepared.id;
    simulation.start(prepared);
    let battle = simulation.active.get_mut(&id).unwrap();
    battle.round.phase = RoundPhase::Battle;
    for player in battle.players.values_mut() {
        player.ready = true;
        player.entered = true;
    }
    let initial = battle.players[&ParticipantId(2)].vehicle.position;
    let next = battle.started_at + Duration::from_millis(100);
    let (outbox, mut received) = tokio::sync::mpsc::channel(128);
    let mut delivery = QueuedDelivery {
        address: SocketAddrV4::new(std::net::Ipv4Addr::LOCALHOST, 0),
        outbox,
    };
    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    let subscriber = tracing_subscriber::registry()
        .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("scenery-test")));
    tracing::subscriber::with_default(subscriber, || {
        let drive = |simulation: &mut BattleSimulation, delivery: &mut QueuedDelivery| {
            tracing::info_span!("game.command").in_scope(|| {
                simulation.vehicle_control_with_delivery(
                    delivery,
                    VehicleControl {
                        participant: ParticipantId(1),
                        entity_id: 11,
                        method: "vehicle_moveWith".into(),
                        argument_count: 1,
                        command: VehicleCommand::Drive(1),
                    },
                )
            });
        };
        drive(&mut simulation, &mut delivery);
        simulation.tick_with_delivery(&mut delivery, next);
        let battle = simulation.active.get_mut(&id).unwrap();
        battle.players.get_mut(&ParticipantId(2)).unwrap().observer = true;
        let hull = battle.players[&ParticipantId(1)].tank.collision_hull();
        let forward = Vec3::new(forward.x, 0.0, forward.y);
        let side = Vec3::new(forward.z, 0.0, -forward.x);
        let center = hull.position + forward * (hull.half_size.y + 0.005);
        battle.obstacles = Scenery::test_numbered_mesh(
            Some(0x7f7f_0003),
            vec![(
                [
                    center - side * 4.0 - Vec3::Y,
                    center + side * 4.0 - Vec3::Y,
                    center + Vec3::Y * 4.0,
                ],
                Some((crate::infrastructure::obstacles::TREE_PART, 1)),
            )],
        );
        battle.obstacles.test_register_tree(0x7f7f_0003, center);
        battle.areas.assign(&battle.obstacles, || 101);
        drive(&mut simulation, &mut delivery);
        simulation.tick_with_delivery(&mut delivery, next + Duration::from_millis(100));
        assert!(
            simulation.active[&id]
                .obstacles
                .intersect(hull.position + Vec3::Y, forward, 20.0)
                .is_none()
        );
        simulation
            .active
            .get_mut(&id)
            .unwrap()
            .players
            .get_mut(&ParticipantId(2))
            .unwrap()
            .entered = false;
        simulation
            .enter_with_delivery(&mut delivery, ParticipantId(2))
            .unwrap();
        let (closed_outbox, closed_receiver) = tokio::sync::mpsc::channel(1);
        drop(closed_receiver);
        let mut failed_delivery = QueuedDelivery {
            address: delivery.address,
            outbox: closed_outbox,
        };
        simulation.active.get_mut(&id).unwrap().publish_breakages(
            &mut failed_delivery,
            runtime.wire.as_deref().unwrap(),
            &[Breakage {
                native: Some(0x7f7f_0004),
                part: 1,
            }],
            BreakageCause::Shot { yaw: 0.0 },
        );
    });
    let battle = &simulation.active[&id];
    assert_eq!(battle.players[&ParticipantId(1)].tank.health, 100);
    assert_eq!(battle.players[&ParticipantId(2)].tank.health, 100);
    let positions = std::iter::from_fn(|| received.try_recv().ok())
        .find_map(|message| {
            if let BattleOutput::Positions(positions) = message {
                Some(positions)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(positions.recipients.len(), 2);
    let pushed = positions
        .vehicles
        .iter()
        .find(|v| v.entity_id == 22)
        .unwrap()
        .position;
    assert!((pushed.xz() - initial.xz()).dot(forward) > 0.0);
    drop(positions);
    drop(received);
    drop(simulation);
    provider.force_flush().unwrap();
    let spans = exporter.get_finished_spans().unwrap();
    let control = spans
        .iter()
        .find(|span| {
            span.name == "battles.control"
                && spans
                    .iter()
                    .any(|request| request.name == "game.command" && child_of(span, request))
                && spans.iter().any(|contact| {
                    contact.name == "battles.world_motion"
                        && has_field(contact, "input.entity_id", "22")
                        && child_of(contact, span)
                })
        })
        .unwrap();
    let contact = spans
        .iter()
        .find(|span| {
            span.name == "battles.world_motion"
                && has_field(span, "input.entity_id", "22")
                && child_of(span, control)
        })
        .unwrap();
    assert!(spans.iter().any(|span| span.name == "battles.replication"
        && has_field(span, "input.entity_id", "22")
        && child_of(span, contact)));
    let fracture = spans
        .iter()
        .find(|span| {
            span.name == "battles.world_motion"
                && has_field(span, "input.entity_id", "12")
                && has_field(span, "output.world.fragments_destroyed", "1")
        })
        .unwrap();
    assert!(
        spans
            .iter()
            .any(|span| span.name == "battles.control" && child_of(fracture, span))
    );
    assert!(spans.iter().any(|span| span.name == "battles.replication"
        && has_field(span, "input.entity_id", "12")
        && child_of(span, fracture)));
    assert!(spans.iter().any(|span| span.name == "battles.scenery"
        && has_field(span, "output.scenery.deliveries.count", "2")
        && has_field(span, "output.scenery.trees.count", "1")
        && child_of(span, fracture)));
    assert!(spans.iter().any(|span| span.name == "battles.enter"
        && has_field(span, "output.scenery.snapshot_chunks.count", "1")));
    assert!(spans.iter().any(|span| span.name == "battles.scenery"
        && has_field(span, "output.outcome", "delivery_failed")
        && matches!(span.status, Status::Error { .. })));
}

fn vehicle_motion(id: u64, distance: f32, travel: f32, inverse_mass: f32) -> VehicleMotion {
    VehicleMotion {
        client: ParticipantId(id),
        previous_direction: Vec3::ZERO,
        previous_tracks: [0.0; 2],
        drive: None,
        parent: None,
        changed: false,
        hull: Hull {
            position: Vec3::Z * distance,
            yaw: 0.0,
            half_size: Vec2::new(1.0, 2.0),
            offset: Vec2::ZERO,
        },
        travel: Vec2::Y * travel,
        inverse_mass,
        scenery_normal: None,
        scenery_impulse: Vec2::ZERO,
        vehicle_normal: None,
        vehicle_contacts: 0,
        impulse: Vec2::ZERO,
        slid: false,
        scenery_breaks: Vec::new(),
        scenery_cause: BreakageCause::Contact {
            yaw: 0.0,
            speed: 0.0,
        },
        pileup_held: false,
    }
}

#[test]
fn heavier_tank_pushes_a_lighter_tank_and_stops_at_an_immovable_wreck() {
    let bounds = [Vec2::splat(-40.0), Vec2::splat(40.0)];
    let heavy = || vehicle_motion(1, 0.0, 4.0, 1.0 / 60_000.0);
    let mut vehicles = [heavy(), vehicle_motion(2, 5.0, 0.0, 1.0 / 20_000.0)];
    resolve_contacts(&mut vehicles, &Scenery::default(), bounds, 0.1);
    let [pusher, pushed] = vehicles;
    assert!(pushed.travel.y > 2.0 && pushed.travel.y < pusher.travel.y);
    assert!(pushed.impulse.y > 0.0);
    assert!(5.0 + pushed.travel.y - pusher.travel.y >= 4.0);

    let mut vehicles = [heavy(), vehicle_motion(2, 5.0, 0.0, 0.0)];
    resolve_contacts(&mut vehicles, &Scenery::default(), bounds, 0.1);
    let [pusher, wreck] = vehicles;
    assert!((pusher.travel.y - (1.0 - CONTACT_SKIN)).abs() < 0.001);
    assert_eq!(wreck.travel, Vec2::ZERO);
}

#[test]
fn scenery_fracture_is_local_and_removes_only_the_hit_fragment() {
    let panel = |z| {
        [
            Vec3::new(-4.0, 0.0, z),
            Vec3::new(4.0, 0.0, z),
            Vec3::new(0.0, 4.0, z),
        ]
    };
    let mut scene = Scenery::test_mesh(vec![
        (panel(2.0), Some((1, 35))),
        (panel(6.0), Some((2, 20))),
    ]);
    let fresh_round = scene.test_new_round();
    let origin = Vec3::Y;
    let fragment = scene
        .intersect(origin, Vec3::Z, 20.0)
        .unwrap()
        .fragment
        .unwrap();
    assert!(scene.damage(fragment, 30).is_empty());
    assert_eq!(
        scene.intersect(origin, Vec3::Z, 20.0).unwrap().distance,
        2.0
    );
    let hull = Hull {
        position: Vec3::ZERO,
        yaw: 0.0,
        half_size: Vec2::ONE,
        offset: Vec2::ZERO,
    };
    assert_eq!(
        crush_along(&mut scene, hull, Vec2::Y * 3.0, 1.0 / 30_000.0, 0.1, 0.0).len(),
        1
    );
    assert!(scene.damage(fragment, 100).is_empty());
    assert!(scene.sweep_hull(hull, Vec2::Y * 3.0).is_none());
    assert_eq!(
        scene.intersect(origin, Vec3::Z, 20.0).unwrap().distance,
        6.0
    );
    assert_eq!(
        fresh_round
            .intersect(origin, Vec3::Z, 20.0)
            .unwrap()
            .distance,
        2.0
    );
}

#[test]
fn ramps_can_be_climbed_and_bridge_support_respects_the_underpass() {
    let terrain = terrain::Terrain::flat();
    let quad = |x0, x1, y0, y1| {
        let a = Vec3::new(x0, y0, -4.0);
        let b = Vec3::new(x0, y0, 4.0);
        let c = Vec3::new(x1, y1, -4.0);
        let d = Vec3::new(x1, y1, 4.0);
        [([a, b, c], None), ([b, d, c], None)]
    };
    let scenery = Scenery::test_mesh(
        quad(0.0, 8.0, 0.0, 4.0)
            .into_iter()
            .chain(quad(8.0, 16.0, 4.0, 4.0))
            .collect(),
    );
    let mut hull = Hull {
        position: Vec3::new(-0.2, 0.0, 0.0),
        yaw: std::f32::consts::FRAC_PI_2,
        half_size: Vec2::new(1.0, 1.5),
        offset: Vec2::ZERO,
    };
    for _ in 0..12 {
        let travel = Vec2::X;
        assert!(scenery.sweep_hull(hull, travel).is_none());
        let (position, _, elevated) = surface_pose(&terrain, &scenery, hull, travel).unwrap();
        assert!(elevated);
        hull.position = position;
    }
    assert!((hull.position.y - 4.0).abs() < 0.01);
    let under = Hull {
        position: Vec3::new(12.0, 0.0, 0.0),
        ..hull
    };
    let (floor, _, elevated) = surface_pose(&terrain, &scenery, under, Vec2::Y).unwrap();
    assert!(!elevated);
    assert_eq!(floor.y, 0.0);
}

#[test]
fn a_pileup_against_a_wreck_does_not_compress_or_stop_unrelated_tanks() {
    let bounds = [Vec2::splat(-60.0), Vec2::splat(60.0)];
    let mut moves: Vec<_> = (0..10)
        .map(|i| {
            let inverse_mass = if i == 9 { 0.0 } else { 1.0 / 30_000.0 };
            vehicle_motion(
                i,
                i as f32 * (4.0 + CONTACT_SKIN),
                3.0 * f32::from(u8::from(i == 0)),
                inverse_mass,
            )
        })
        .collect();
    let mut unrelated = vehicle_motion(10, 0.0, 3.0, 1.0 / 30_000.0);
    unrelated.hull.position.x = 20.0;
    moves.push(unrelated);
    resolve_contacts(&mut moves, &Scenery::default(), bounds, 0.1);
    for pair in moves[..10].windows(2) {
        let [first, other] = pair else { unreachable!() };
        let gap = other.hull.position.z + other.travel.y - first.hull.position.z - first.travel.y;
        assert!(gap >= 4.0 - PILEUP_TOLERANCE_METRES);
    }
    assert_eq!(moves[9].travel, Vec2::ZERO);
    assert_eq!(moves[10].travel, Vec2::Y * 3.0);
}

#[test]
#[ignore = "Requires WG_RESOURCE_DIR with the extracted EU Serene Coast collision and terrain"]
fn extracted_serene_coast_bridge_keeps_its_deck_separate_from_the_terrain() {
    let directory = std::env::var_os("WG_RESOURCE_DIR").unwrap();
    let resources = ResourceBundle::open(std::path::Path::new(&directory)).unwrap();
    let geometry = "spaces/47_canada_a";
    let terrain = terrain::Terrain::load(&resources, geometry).unwrap();
    let scenery = Scenery::new(Arc::new(
        super::super::obstacles::Obstacles::load(&resources, geometry, 47).unwrap(),
    ));
    let point = Vec2::new(-143.73, 102.71);
    let floor = terrain.height_at(point).unwrap();
    let deck_y = -5.47;
    assert!(deck_y - floor > 1.0);
    let hull = Hull {
        position: Vec3::new(point.x, deck_y, point.y),
        yaw: 0.0,
        half_size: Vec2::new(1.0, 2.0),
        offset: Vec2::ZERO,
    };
    let (deck, _, elevated) = surface_pose(&terrain, &scenery, hull, Vec2::ZERO).unwrap();
    assert!(elevated);
    assert!((deck.y - deck_y).abs() < 0.7);
    let ground_hull = Hull {
        position: Vec3::new(point.x, floor, point.y),
        ..hull
    };
    let (below, _, elevated) = surface_pose(&terrain, &scenery, ground_hull, Vec2::ZERO).unwrap();
    assert!(!elevated);
    assert!(below.y < deck.y - 0.7);
}
