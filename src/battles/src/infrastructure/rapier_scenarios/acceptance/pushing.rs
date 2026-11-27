use super::*;
use opentelemetry_sdk::trace::SpanData;

fn child_of(child: &SpanData, parent: &SpanData) -> bool {
    child.parent_span_id == parent.span_context.span_id()
        && child.span_context.trace_id() == parent.span_context.trace_id()
}

fn has(span: &SpanData, key: &str, value: &str) -> bool {
    span.attributes
        .iter()
        .any(|a| a.key.as_str() == key && a.value.to_string() == value)
}

#[test]
#[ignore = "Requires WOT_RESOURCE_DIR with current EU entity definitions"]
fn powered_push_replicates_the_parked_hull_and_only_the_broken_wall_part() {
    let (arena_id, mut simulation) = prepare(2);
    let pusher = ParticipantId(1);
    let pushed = ParticipantId(2);
    let battle = simulation.active.get_mut(&arena_id).unwrap();
    let skin = battle.parameters.contact_skin_metres;
    let half_length = battle.players[&pushed].tank.collision_extents().0.z;
    let pushed_z = half_length * 2.0 + 2.0 * skin;
    let wall_z = pushed_z + half_length + skin;
    let face = |x| {
        [
            Vec3::new(x - 3.0, -1.0, wall_z),
            Vec3::new(x + 3.0, -1.0, wall_z),
            Vec3::new(x, 4.0, wall_z),
        ]
    };
    place(battle.players.get_mut(&pusher).unwrap(), Vec3::ZERO, 0.0);
    place(
        battle.players.get_mut(&pushed).unwrap(),
        Vec3::Z * pushed_z,
        0.0,
    );
    let start_health = [
        battle.players[&pusher].tank.health,
        battle.players[&pushed].tank.health,
    ];
    battle.ground = Arc::new(terrain::Terrain::flat_with_material(0));
    battle.obstacles = obstacles::Scenery::test_numbered_mesh(
        Some(0x7f7f_0002),
        vec![(face(0.0), Some((1, 2))), (face(10.0), Some((2, 2)))],
    );
    battle.areas.assign(&battle.obstacles, || 100);
    battle.physics = physics::ArenaPhysics::new(
        &battle.ground,
        &battle.obstacles,
        battle.map.bounds,
        battle.parameters,
    )
    .unwrap();
    for _ in 0..10 {
        battle.advance_world(0.1);
    }
    let wall_present = |battle: &Battle, x| {
        battle
            .obstacles
            .intersect(Vec3::new(x, 1.0, 0.0), Vec3::Z, wall_z + 1.0)
            .is_some()
    };
    assert!(wall_present(battle, 0.0) && wall_present(battle, 10.0));
    let start = battle.started_at;
    let (outbox, mut receiver) = tokio::sync::mpsc::channel(512);
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
        .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("powered-push")));
    let mut pushed_replica = None;
    tracing::subscriber::with_default(subscriber, || {
        tracing::info_span!("game.command")
            .in_scope(|| drive(&mut simulation, &mut delivery, 1, 1));
        for tick in 1..=50 {
            simulation.tick_with_delivery(&mut delivery, start + TICK * tick);
            while let Ok(message) = receiver.try_recv() {
                if let BattleOutput::Positions(snapshot) = message
                    && snapshot.recipients.len() == 2
                    && let Some(vehicle) = snapshot.vehicles.iter().find(|v| v.entity_id == 22)
                {
                    pushed_replica = Some(vehicle.position);
                }
            }
        }
    });
    let battle = &simulation.active[&arena_id];
    let positions = [
        battle.players[&pusher].vehicle.position.z,
        battle.players[&pushed].vehicle.position.z,
    ];
    assert!(
        !wall_present(battle, 0.0),
        "powered push never broke the wall: {positions:?}"
    );
    assert!(wall_present(battle, 10.0));
    assert!(
        positions[0] > 1.0 && positions[1] > wall_z,
        "broken wall still blocked the hull: {positions:?}, wall at {wall_z}"
    );
    assert_eq!(
        pushed_replica,
        Some(battle.players[&pushed].vehicle.position)
    );
    assert_eq!(
        start_health,
        [
            battle.players[&pusher].tank.health,
            battle.players[&pushed].tank.health
        ]
    );
    drop(simulation);
    drop(receiver);
    provider.force_flush().unwrap();
    let spans = exporter.get_finished_spans().unwrap();
    let command = spans
        .iter()
        .find(|span| span.name == "game.command")
        .unwrap();
    let control = spans
        .iter()
        .find(|span| span.name == "battles.control" && child_of(span, command))
        .unwrap();
    let fracture = spans
        .iter()
        .find(|span| {
            span.name == "battles.world_motion"
                && has(span, "input.entity_id", "22")
                && has(span, "output.world.fragments_destroyed", "1")
                && child_of(span, control)
        })
        .expect("pushed tank fracture belongs to the pusher's command");
    for key in [
        "output.world.pitch_radians",
        "output.world.lateral_speed_mps",
        "output.world.vertical_speed_mps",
    ] {
        assert!(fracture.attributes.iter().any(|attribute| {
            attribute.key.as_str() == key
                && matches!(attribute.value, opentelemetry::Value::F64(value) if value.is_finite())
        }), "missing solved hull measurement {key}");
    }
    assert!(spans.iter().any(|span| span.name == "battles.scenery"
        && child_of(span, fracture)
        && has(span, "output.scenery.fragments.count", "1")
        && has(span, "output.scenery.deliveries.count", "2")));
    assert!(spans.iter().any(|span| span.name == "battles.replication"
        && child_of(span, fracture)
        && has(span, "input.entity_id", "22")));
}
