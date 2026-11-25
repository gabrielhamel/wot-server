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
#[ignore = "Requires WOT_RESOURCE_DIR with the extracted EU client bundle"]
fn pushing_through_a_wall_parents_pose_and_fracture_delivery_to_the_drive_command() {
    let path = std::env::var_os("WOT_RESOURCE_DIR").unwrap();
    let resources = ResourceBundle::open(std::path::Path::new(&path)).unwrap();
    let dispatch = Arc::new(ScriptDispatch::new(script::load(&resources).unwrap()));
    let mut runtime = BattleRuntime::load(dispatch, &resources, true).unwrap();
    let arena = (1 << 16) | 5;
    let map = runtime.maps.get_mut(&arena).unwrap().as_mut().unwrap();
    let start = map.spawns[0];
    let forward = (map.spawns[1] - start).normalize();
    map.spawns[1] = start + forward * 5.001;
    let vehicle = || VehicleLoadout {
        descriptor: vec![1],
        outfits: Default::default(),
        health: 100,
        crew_size: 3,
        ammunition: Vec::new(),
        performance: Some(VehiclePerformance {
            forward_speed: 12.0,
            reverse_speed: 4.0,
            hull_traverse: 0.7,
            mobility: Mobility::default(),
            turret_pivot: None,
            armor: Default::default(),
            device_health: Default::default(),
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
                shell_ballistics: Default::default(),
            },
        }),
    };
    let prepared = runtime
        .prepare(
            BattleLaunch {
                parameters: Default::default(),
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
                        stationary_dummy: false,
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
    let pusher_start = battle.players[&ParticipantId(1)].vehicle.position;
    let along = Vec3::new(forward.x, 0.0, forward.y);
    let side = Vec3::new(forward.y, 0.0, -forward.x);
    let wall = initial + along * 2.525;
    battle.obstacles = Scenery::test_numbered_mesh(
        Some(0x7f7f_0002),
        vec![(
            [
                wall - side * 4.0 - Vec3::Y,
                wall + side * 4.0 - Vec3::Y,
                wall + Vec3::Y * 4.0,
            ],
            Some((1, 1)),
        )],
    );
    battle.areas.assign(&battle.obstacles, || 100);
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
        let drive = |simulation: &mut BattleSimulation, delivery: &mut QueuedDelivery, flags| {
            tracing::info_span!("game.command").in_scope(|| {
                simulation.vehicle_control_with_delivery(
                    delivery,
                    VehicleControl {
                        participant: ParticipantId(1),
                        entity_id: 11,
                        method: "vehicle_moveWith".into(),
                        argument_count: 1,
                        command: VehicleCommand::Drive(flags),
                    },
                )
            });
        };
        drive(&mut simulation, &mut delivery, 1);
        simulation.tick_with_delivery(&mut delivery, next);
        let battle = simulation.active.get_mut(&id).unwrap();
        assert!(
            battle
                .obstacles
                .intersect(initial + Vec3::Y, along, 5.0)
                .is_none()
        );
        battle.players.get_mut(&ParticipantId(2)).unwrap().observer = true;
        let player = battle.players.get_mut(&ParticipantId(1)).unwrap();
        player.tank.hold_world_position();
        let hull = player.tank.collision_hull();
        let forward = Vec3::new(forward.x, 0.0, forward.y);
        let side = Vec3::new(forward.z, 0.0, -forward.x);
        let center = hull.position + side * (hull.half_size.x + 0.005);
        battle.obstacles = Scenery::test_numbered_mesh(
            Some(0x7f7f_0003),
            vec![(
                [
                    center - forward * 4.0 - Vec3::Y,
                    center + forward * 4.0 - Vec3::Y,
                    center + Vec3::Y * 4.0,
                ],
                Some((crate::infrastructure::obstacles::TREE_PART, 1)),
            )],
        );
        battle.obstacles.test_register_tree(0x7f7f_0003, center);
        battle.areas.assign(&battle.obstacles, || 101);
        drive(&mut simulation, &mut delivery, 8);
        simulation.tick_with_delivery(&mut delivery, next + Duration::from_millis(100));
        assert!(
            simulation.active[&id]
                .obstacles
                .intersect(hull.position + Vec3::Y, side, 20.0)
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
    let pusher = positions
        .vehicles
        .iter()
        .find(|v| v.entity_id == 12)
        .unwrap()
        .position;
    assert!((pusher.xz() - pusher_start.xz()).dot(forward) > 0.0);
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
    assert!(has_field(contact, "output.world.fragments_destroyed", "1"));
    assert!(spans.iter().any(|span| span.name == "battles.replication"
        && has_field(span, "input.entity_id", "22")
        && child_of(span, contact)));
    assert!(spans.iter().any(|span| span.name == "battles.scenery"
        && has_field(span, "output.scenery.deliveries.count", "2")
        && child_of(span, contact)));
    let fracture = spans
        .iter()
        .find(|span| {
            span.name == "battles.world_motion"
                && has_field(span, "input.entity_id", "12")
                && has_field(span, "output.world.fragments_destroyed", "1")
                && has_field(span, "output.world.turn_contact", "true")
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
            parameters: Default::default(),
            position: Vec3::Z * distance,
            yaw: 0.0,
            half_size: Vec2::new(1.0, 2.0),
            offset: Vec2::ZERO,
            body: None,
        },
        travel: Vec2::Y * travel,
        inverse_mass,
        scenery_normal: None,
        scenery_impulse: Vec2::ZERO,
        vehicle_normal: None,
        vehicle_contacts: 0,
        scenery: SceneryMotion::new(inverse_mass, 0.1),
        drive_pressure: Vec2::ZERO,
        pushed_pressure: Vec2::ZERO,
        impulse: Vec2::ZERO,
        slid: false,
        pileup_held: false,
    }
}

#[test]
fn heavier_tank_pushes_a_lighter_tank_and_stops_at_an_immovable_wreck() {
    let bounds = [Vec2::splat(-40.0), Vec2::splat(40.0)];
    let heavy = || vehicle_motion(1, 0.0, 4.0, 1.0 / 60_000.0);
    let mut vehicles = [heavy(), vehicle_motion(2, 5.0, 0.0, 1.0 / 20_000.0)];
    resolve_contacts(&mut vehicles, &mut Scenery::default(), bounds, 0.1, true);
    let [pusher, pushed] = vehicles;
    assert!(pushed.travel.y > 2.0 && pushed.travel.y < pusher.travel.y);
    assert!(pushed.impulse.y > 0.0);
    assert!(5.0 + pushed.travel.y - pusher.travel.y >= 4.0);

    let mut vehicles = [heavy(), vehicle_motion(2, 5.0, 0.0, 0.0)];
    resolve_contacts(&mut vehicles, &mut Scenery::default(), bounds, 0.1, true);
    let [pusher, wreck] = vehicles;
    assert!((pusher.travel.y - 1.0).abs() < 0.001);
    assert_eq!(wreck.travel, Vec2::ZERO);
}

#[test]
fn engine_pressure_through_a_parked_tank_breaks_the_wall_and_both_advance() {
    let bounds = [Vec2::splat(-40.0), Vec2::splat(40.0)];
    let wall = [
        Vec3::new(-4.0, -2.0, 6.0),
        Vec3::new(4.0, -2.0, 6.0),
        Vec3::new(0.0, 4.0, 6.0),
    ];
    let pair = || {
        let mut pusher = vehicle_motion(1, 0.0, 0.02, 1.0 / 36_000.0);
        pusher.drive_pressure = Vec2::Y * 12.0;
        [pusher, vehicle_motion(2, 4.0, 0.0, 1.0 / 36_000.0)]
    };
    let mut scenery = Scenery::test_mesh(vec![(wall, Some((17, 18)))]);
    let mut blocked = pair();
    resolve_contacts(&mut blocked, &mut scenery, bounds, 0.1, true);
    assert!(blocked.iter().all(|tank| tank.travel.length() < 0.001));
    assert!(blocked.iter().all(|tank| tank.breakages().0.is_empty()));

    let mut released = pair();
    resolve_contacts(&mut released, &mut scenery, bounds, 0.1, true);
    let [pusher, pushed] = released;
    assert!(pusher.breakages().0.is_empty());
    assert_eq!(pushed.breakages().0.len(), 1);
    assert!(pusher.travel.y > 0.009 && pushed.travel.y > 0.009);
    assert!(pushed.scenery_normal.is_none());

    let mut solid = Scenery::test_mesh(vec![(wall, None)]);
    let mut blocked = pair();
    resolve_contacts(&mut blocked, &mut solid, bounds, 0.1, true);
    assert!(blocked.iter().all(|tank| tank.travel.length() < 0.001));
}

#[test]
fn steering_into_a_parallel_wall_can_fracture_it_without_translation() {
    let wall = [
        Vec3::new(1.14, -2.0, -8.0),
        Vec3::new(1.14, -2.0, 8.0),
        Vec3::new(1.14, 8.0, 0.0),
    ];
    let hull = Hull {
        parameters: Default::default(),
        position: Vec3::ZERO,
        yaw: 0.0,
        half_size: Vec2::new(1.0, 2.5),
        offset: Vec2::ZERO,
        body: None,
    };
    let mut solid = Scenery::test_mesh(vec![(wall, None)]);
    let turn = 0.2;
    let mut motion = SceneryMotion::new(1.0 / 36_000.0, 0.1);
    let (blocked, surface) = motion.turn(&mut solid, hull, turn, 7.0, true);
    assert!(surface.is_some());
    assert!(blocked.yaw >= 0.0 && blocked.yaw < turn);
    assert_eq!(blocked.position, hull.position);

    let mut destructible = Scenery::test_mesh(vec![(wall, Some((73, 80)))]);
    let (mut stalled, surface) = motion.turn(&mut destructible, hull, turn, 7.0, true);
    assert!(surface.is_some());
    assert!(motion.broken.is_empty());
    for _ in 0..12 {
        let mut next = SceneryMotion::new(1.0 / 36_000.0, 0.1);
        let (turned, surface) = next.turn(&mut destructible, stalled, turn, 7.0, true);
        assert_eq!(turned.position, hull.position);
        if surface.is_none() {
            assert!((turned.yaw - turn).abs() < 1e-6);
            assert_eq!(next.broken.len(), 1);
            return;
        }
        stalled = turned;
    }
    panic!("the pivot should eventually break the wall");
}

#[test]
fn engine_pressure_breaks_from_rest_and_light_vehicles_lose_more_speed() {
    let wall = [
        Vec3::new(-4.0, -1.0, 2.03),
        Vec3::new(4.0, -1.0, 2.03),
        Vec3::new(0.0, 5.0, 2.03),
    ];
    let hull = Hull {
        parameters: Default::default(),
        position: Vec3::ZERO,
        yaw: 0.0,
        half_size: Vec2::new(1.0, 2.0),
        offset: Vec2::ZERO,
        body: None,
    };
    let apply = |mass: f32, reach, travel, press| {
        let mut scene = Scenery::test_mesh(vec![(wall, Some((73, 12)))]);
        let mut motion = SceneryMotion::new(1.0 / mass, 0.1);
        motion.push(&mut scene, hull, reach, travel, press);
        motion
    };
    let stopped = apply(36_000.0, Vec2::Y * PRESS_REACH, Vec2::ZERO, Vec2::Y * 12.0);
    assert_eq!(stopped.broken.len(), 1);
    let coast = apply(36_000.0, Vec2::Y * PRESS_REACH, Vec2::ZERO, Vec2::ZERO);
    assert!(coast.broken.is_empty());

    let light = apply(2_500.0, Vec2::Y, Vec2::Y, Vec2::ZERO);
    let medium = apply(36_000.0, Vec2::Y, Vec2::Y, Vec2::ZERO);
    assert_eq!(light.broken.len(), 1);
    assert_eq!(medium.broken.len(), 1);
    assert!((0.89..0.91).contains(&light.speed_scale));
    assert!(medium.speed_scale > 0.99);
}

#[test]
fn tks_20_entry_speed_affects_how_many_widepark_strength_panels_it_breaks() {
    let seconds = TICK.as_secs_f32();
    let mass = 2_800.0;
    let engine_power = mass * 11.664_455;
    let pressure = Vec2::Y * (1.13 * engine_power / 2.0 * 0.0002 * seconds);
    let bounds = [Vec2::splat(-40.0), Vec2::splat(40.0)];
    let panels: Vec<_> = (1_u8..=5)
        .map(|part| {
            let z = 1.5 + 1.05 * f32::from(part);
            (
                [
                    Vec3::new(-4.0, -1.0, z),
                    Vec3::new(4.0, -1.0, z),
                    Vec3::new(0.0, 5.0, z),
                ],
                Some((u32::from(part), 10)),
            )
        })
        .collect();
    let start = Hull {
        parameters: Default::default(),
        position: Vec3::ZERO,
        yaw: 0.0,
        half_size: Vec2::new(0.9, 1.5),
        offset: Vec2::ZERO,
        body: None,
    };
    for (entry_speed, expected_breaks) in [(8.5, 3), (9.8, 4), (10.3, 5)] {
        let mut scene = Scenery::test_mesh(panels.clone());
        let mut hull = start;
        let mut speed = entry_speed;
        let mut broken = 0;
        let mut stalled = false;
        for _ in 0..12 {
            let mut step = SceneryMotion::new(1.0 / mass, seconds);
            let travel = Vec2::Y * (speed * seconds);
            step.push(
                &mut scene,
                hull,
                travel + Vec2::Y * PRESS_REACH,
                travel,
                pressure,
            );
            let (allowed, contact) = constrain(&scene, hull, travel, bounds);
            hull.position.z += allowed.y;
            speed *= step.speed_scale;
            broken += step.broken.len();
            stalled = contact.is_some();
            if stalled || broken == 5 {
                break;
            }
        }
        assert_eq!(broken, expected_breaks, "entry speed {entry_speed} m/s");
        assert_eq!(stalled, expected_breaks < 5);
        assert!((entry_speed * 0.58..entry_speed * 0.81).contains(&speed));
    }
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
        parameters: Default::default(),
        position: Vec3::ZERO,
        yaw: 0.0,
        half_size: Vec2::ONE,
        offset: Vec2::ZERO,
        body: None,
    };
    let mut motion = SceneryMotion::new(1.0 / 30_000.0, 0.1);
    motion.push(&mut scene, hull, Vec2::Y * 3.0, Vec2::Y * 3.0, Vec2::ZERO);
    assert_eq!(motion.broken.len(), 1);
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
        parameters: Default::default(),
        position: Vec3::new(-0.2, 0.0, 0.0),
        yaw: std::f32::consts::FRAC_PI_2,
        half_size: Vec2::new(1.0, 1.5),
        offset: Vec2::ZERO,
        body: None,
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
                i as f32 * 4.0,
                3.0 * f32::from(u8::from(i == 0)),
                inverse_mass,
            )
        })
        .collect();
    let mut unrelated = vehicle_motion(10, 0.0, 3.0, 1.0 / 30_000.0);
    unrelated.hull.position.x = 20.0;
    moves.push(unrelated);
    resolve_contacts(&mut moves, &mut Scenery::default(), bounds, 0.1, true);
    for pair in moves[..10].windows(2) {
        let [first, other] = pair else { unreachable!() };
        let gap = other.hull.position.z + other.travel.y - first.hull.position.z - first.travel.y;
        assert!(gap >= 4.0 - PILEUP_TOLERANCE_METRES);
    }
    assert_eq!(moves[9].travel, Vec2::ZERO);
    assert_eq!(moves[10].travel, Vec2::Y * 3.0);
}

#[test]
#[ignore = "Requires WOT_RESOURCE_DIR with the extracted EU Serene Coast collision and terrain"]
fn extracted_serene_coast_bridge_keeps_its_deck_separate_from_the_terrain() {
    let directory = std::env::var_os("WOT_RESOURCE_DIR").unwrap();
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
        parameters: Default::default(),
        position: Vec3::new(point.x, deck_y, point.y),
        yaw: 0.0,
        half_size: Vec2::new(1.0, 2.0),
        offset: Vec2::ZERO,
        body: None,
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
