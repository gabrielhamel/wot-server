use super::*;
use crate::application::{BattleParticipant, VehicleCommand, VehicleControl};
use delivery::QueuedDelivery;
use glam::Vec3Swizzles;
use projectile_tests::profile;

fn prepare(count: u32, model: u8) -> (u32, BattleSimulation) {
    let resources = ResourceBundle::open(std::path::Path::new(
        &std::env::var_os("WOT_RESOURCE_DIR").expect("extracted resources"),
    ))
    .unwrap();
    let dispatch = Arc::new(ScriptDispatch::new(
        wot_platform::mercury::script::load(&resources).unwrap(),
    ));
    let mut runtime = BattleRuntime::load(dispatch, &resources, true).unwrap();
    let parameters =
        BattleParameters::from_overrides(&[("hull_physics".to_owned(), f64::from(model))].into())
            .unwrap();
    let arena = runtime
        .maps
        .iter()
        .filter_map(|(id, map)| {
            map.as_ref()
                .filter(|map| map.geometry.ends_with("47_canada_a"))
                .map(|_| *id)
        })
        .min()
        .expect("Serene Coast Training arena");
    let prepared = runtime
        .prepare(
            BattleLaunch {
                arena_type: arena,
                parameters,
                duration: Duration::from_secs(600),
                mode: BattleMode::Training,
                participants: (1..=count)
                    .map(|i| {
                        let mut vehicle = Entrant::sample().vehicle;
                        vehicle.performance = Some(profile());
                        BattleParticipant {
                            id: ParticipantId(u64::from(i)),
                            account_id: i64::from(i),
                            name: format!("Driver {i}"),
                            team: if i % 2 == 1 { 1 } else { 2 },
                            is_host: i == 1,
                            observer: false,
                            stationary_dummy: false,
                            customization_display_type: 0,
                            vehicle,
                        }
                    })
                    .collect(),
            },
            (1..=count)
                .map(|i| BattleEntityIds {
                    avatar: 10 * i + 1,
                    vehicle: 10 * i + 2,
                    info: 10 * i + 3,
                })
                .collect(),
        )
        .unwrap()
        .unwrap();
    let id = prepared.id;
    let mut simulation = BattleSimulation {
        active: Default::default(),
        engagements: Default::default(),
        wire: runtime.wire,
    };
    simulation.start(prepared);
    let arena = simulation.active.get_mut(&id).unwrap();
    for player in arena.players.values_mut() {
        player.ready = true;
        player.entered = true;
    }
    arena.round.phase = RoundPhase::Battle;
    arena.round.deadline = arena.started_at + Duration::from_secs(600);
    (id, simulation)
}

fn place(player: &mut Participant, position: Vec3, yaw: f32) {
    player.tank.position = position;
    player.tank.yaw = yaw;
    let direction = Vec3::Z * yaw;
    player.tank.set_hull_tilt(direction);
    player.vehicle.position = position;
    player.vehicle.direction = direction;
}

fn drive(simulation: &mut BattleSimulation, delivery: &mut QueuedDelivery, id: u32, flags: u8) {
    simulation.vehicle_control_with_delivery(
        delivery,
        VehicleControl {
            participant: ParticipantId(u64::from(id)),
            entity_id: id * 10 + 1,
            method: "vehicle_moveWith".into(),
            argument_count: 1,
            command: VehicleCommand::Drive(flags),
        },
    );
}

mod acceptance {
    mod pushing;
    use super::*;
    use opentelemetry::trace::TracerProvider;
    use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};
    use tracing_subscriber::prelude::*;

    #[test]
    #[ignore = "Requires WOT_RESOURCE_DIR"]
    fn flat_ground_acceleration_braking_and_pivots_match_standard_controls() {
        let (id, mut simulation) = prepare(2, 0);
        let mut results = Vec::new();
        for material in [0, 2] {
            for model in [0, 1] {
                let battle = simulation.active.get_mut(&id).unwrap();
                battle.ground = Arc::new(terrain::Terrain::flat_with_material(material));
                battle.obstacles = obstacles::Scenery::default();
                battle.physics = (model == 1).then(|| {
                    physics::ArenaPhysics::new(
                        &battle.ground,
                        &battle.obstacles,
                        battle.map.bounds,
                        battle.parameters,
                    )
                    .unwrap()
                });
                let id = ParticipantId(1);
                battle.players.get_mut(&ParticipantId(2)).unwrap().observer = true;
                let driver = battle.players.get_mut(&id).unwrap();
                let mut entrant = Entrant::sample();
                entrant.vehicle.performance = Some(profile());
                driver.tank = entrant.tank();
                place(driver, Vec3::ZERO, 0.0);
                for _ in 0..10 {
                    battle.advance_world(0.1);
                }
                battle.players.get_mut(&id).unwrap().tank.drive(1);
                for _ in 0..20 {
                    battle.advance_world(0.1);
                }
                let at_speed = battle.players[&id].vehicle.position.z;
                battle.players.get_mut(&id).unwrap().tank.drive(64);
                for _ in 0..10 {
                    battle.advance_world(0.1);
                }
                let stopped = battle.players[&id].vehicle.position;
                battle.players.get_mut(&id).unwrap().tank.drive(8);
                for _ in 0..10 {
                    battle.advance_world(0.1);
                }
                let player = &battle.players[&id];
                let braking = stopped.z - at_speed;
                let rotation = player.vehicle.direction.z;
                eprintln!(
                    "material={material} model={model} acceleration distance={at_speed:.3} metres, braking distance={braking:.3} metres, pivot={rotation:.3} radians"
                );
                assert!(at_speed > 1.0 && braking >= 0.0 && rotation > 0.1);
                assert!(stopped.xz().distance(player.vehicle.position.xz()) < 0.35);
                results.push([at_speed, braking, rotation]);
            }
        }
        for pair in results.as_chunks::<2>().0 {
            for (regular, rapier) in pair[0].into_iter().zip(pair[1]) {
                assert!(
                    (regular - rapier).abs() < 0.2 * regular + 0.10,
                    "{regular} vs {rapier}"
                );
            }
        }
        for i in 0..2 {
            assert!(results[i][0] > results[i + 2][0]);
        }
    }

    #[test]
    #[ignore = "Requires WOT_RESOURCE_DIR with Serene Coast collision data"]
    fn serene_coast_bridge_and_underpass_replicate_the_solved_hulls() {
        let (id, mut simulation) = prepare(2, 1);
        let battle = simulation.active.get_mut(&id).unwrap();
        // This bridge has a level channel and an opening between its piers 8 m from its midpoint.
        let center = Vec2::new(269.27103, -124.133965);
        let along = Vec2::new(0.57964814, 0.81486684);
        let across = Vec2::new(along.y, -along.x);
        let deck = -10.53;
        for (id, start, heading, on_deck) in [
            (1, center - along * 25.0, along, true),
            (2, center - along * 8.0 - across * 10.0, across, false),
        ] {
            let yaw = heading.x.atan2(heading.y);
            let player = battle.players.get_mut(&ParticipantId(id)).unwrap();
            let support = |point| {
                let floor = battle.ground.height_at(point).unwrap();
                let ceiling = if on_deck { deck + 0.5 } else { deck - 1.5 };
                battle
                    .obstacles
                    .support_height(point, floor, ceiling)
                    .unwrap_or(floor)
            };
            let (spawn, direction) = terrain::pose_from_samples(
                start,
                yaw,
                player.tank.collision_hull().half_size,
                support(start),
                support,
            );
            place(player, spawn, yaw);
            player.tank.set_hull_tilt(direction);
            player.vehicle.direction = direction;
        }
        let start = battle.started_at;
        let (outbox, mut rx) = tokio::sync::mpsc::channel(512);
        let drops = std::sync::atomic::AtomicU64::new(0);
        let mut delivery = QueuedDelivery {
            address: "127.0.0.1:0".parse().unwrap(),
            outbox,
            snapshot_queue_drops: &drops,
        };
        let exporter = InMemorySpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let subscriber = tracing_subscriber::registry()
            .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("hull-physics")));
        let mut delivered = Vec::new();
        tracing::subscriber::with_default(subscriber, || {
            for tick in 1..=95 {
                if tick == 6 {
                    for participant in [1, 2] {
                        tracing::info_span!("game.command")
                            .in_scope(|| drive(&mut simulation, &mut delivery, participant, 1));
                    }
                }
                simulation.tick_with_delivery(&mut delivery, start + TICK * tick);
                while let Ok(message) = rx.try_recv() {
                    if let BattleOutput::Positions(snapshot) = message {
                        delivered.push(snapshot);
                    }
                }
            }
        });
        let players = &simulation.active[&id].players;
        let top = players[&ParticipantId(1)].vehicle.position;
        let below = players[&ParticipantId(2)].vehicle.position;
        eprintln!(
            "bridge final upper {top:?}, lower {below:?}; longitudinal progress {:.2}, perpendicular progress {:.2}",
            (top.xz() - center).dot(along),
            (below.xz() - center).dot(across)
        );
        assert!((top.xz() - center).dot(along) > 25.0);
        assert!((below.xz() - center).dot(across) > 6.5);
        for (entity, upper) in [(12, true), (22, false)] {
            let center = if upper { center } else { center - along * 8.0 };
            let pose = delivered
                .iter()
                .flat_map(|snap| &snap.vehicles)
                .filter(|v| v.entity_id == entity)
                .min_by(|a, b| {
                    a.position
                        .xz()
                        .distance_squared(center)
                        .total_cmp(&b.position.xz().distance_squared(center))
                })
                .unwrap()
                .position;
            eprintln!("center witness {entity} {pose:?}");
            assert!(pose.xz().distance(center) < 4.0);
            if upper {
                assert!((pose.y - deck).abs() < 0.8);
            } else {
                assert!(pose.y < deck - 2.0);
            }
        }
        assert!(
            delivered
                .last()
                .unwrap()
                .vehicles
                .iter()
                .any(|v| v.entity_id == 12 && v.position == top)
        );
        assert!(
            delivered
                .last()
                .unwrap()
                .vehicles
                .iter()
                .any(|v| v.entity_id == 22 && v.position == below)
        );
        drop(delivered);
        drop(simulation);
        provider.force_flush().unwrap();
        let spans = exporter.get_finished_spans().unwrap();
        for entity in ["12", "22"] {
            assert!(spans.iter().any(|motion| {
                motion.name == "battles.world_motion"
                    && motion.attributes.iter().any(|a| {
                        a.key.as_str() == "input.entity_id" && a.value.to_string() == entity
                    })
                    && spans.iter().any(|control| {
                        control.name == "battles.control"
                            && control.span_context.span_id() == motion.parent_span_id
                            && spans.iter().any(|command| {
                                command.name == "game.command"
                                    && command.span_context.span_id() == control.parent_span_id
                            })
                    })
                    && spans.iter().any(|response| {
                        response.name == "battles.replication"
                            && response.parent_span_id == motion.span_context.span_id()
                    })
            }));
        }
        assert!(spans.iter().any(|step| step.name == "battles.hull_physics"
            && step.attributes.iter().any(
                |a| a.key.as_str() == "output.physics.substeps" && a.value.to_string() == "6"
            )));
    }
}

#[test]
#[ignore = "Local release benchmark with WOT_RESOURCE_DIR"]
fn measure_thirty_driver_arena_ticks() {
    for model in [0, 1] {
        benchmark_arena_ticks(model, false);
    }
}

#[test]
#[ignore = "Local release benchmark with WOT_RESOURCE_DIR"]
fn measure_thirty_dispersed_driver_arena_ticks() {
    benchmark_arena_ticks(1, true);
}

fn benchmark_arena_ticks(model: u8, dispersed: bool) {
    let (id, mut simulation) = prepare(30, model);
    if dispersed {
        let battle = simulation.active.get_mut(&id).unwrap();
        for i in 0..30 {
            let [min, max] = battle.map.bounds;
            let location = min
                + (max - min)
                    * Vec2::new(((i % 6) as f32 + 0.5) / 6.0, ((i / 6) as f32 + 0.5) / 5.0);
            let yaw = if i % 2 == 0 { 0.7 } else { -0.7 };
            let (position, direction) = battle.ground.vehicle_pose(location, yaw).unwrap();
            let player = battle.players.get_mut(&ParticipantId(i + 1)).unwrap();
            place(player, position, yaw);
            player.tank.set_hull_tilt(direction);
            player.vehicle.direction = direction;
        }
    }
    let start = simulation.active[&id].started_at;
    let (outbox, mut rx) = tokio::sync::mpsc::channel(2048);
    let drops = std::sync::atomic::AtomicU64::new(0);
    let mut delivery = QueuedDelivery {
        address: "127.0.0.1:0".parse().unwrap(),
        outbox,
        snapshot_queue_drops: &drops,
    };
    for id in 1..=30 {
        drive(&mut simulation, &mut delivery, id, 1);
    }
    let mut samples = Vec::new();
    let mut snapshots = 0;
    for tick in 1..=320 {
        if tick % 50 == 0 {
            for id in 1..=30 {
                drive(
                    &mut simulation,
                    &mut delivery,
                    id,
                    if tick % 100 == 0 { 1 } else { 1 | 8 },
                );
            }
        }
        let now = Instant::now();
        simulation.tick_with_delivery(&mut delivery, start + TICK * tick);
        if tick > 20 {
            samples.push(now.elapsed());
        }
        while let Ok(message) = rx.try_recv() {
            if let BattleOutput::Positions(snapshot) = message {
                assert_eq!(snapshot.recipients.len(), 30);
                snapshots += 1;
            }
        }
    }
    assert_eq!(snapshots, 320);
    assert_eq!(drops.load(std::sync::atomic::Ordering::Relaxed), 0);
    samples.sort_unstable();
    eprintln!(
        "30 drivers, model {model}, dispersed={dispersed}, {} arena ticks: p50 {:?}, p95 {:?}, max {:?}",
        samples.len(),
        samples[150],
        samples[284],
        samples.last().unwrap()
    );
}
