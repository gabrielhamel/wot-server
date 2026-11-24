use super::*;
use crate::{
    application::BattleParticipant,
    infrastructure::{
        delivery::{BattlePositions, QueuedDelivery},
        worker::BattleOutput,
    },
};
use opentelemetry::trace::TracerProvider;
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};
use tracing_subscriber::prelude::*;
use wot_platform::mercury::net::element::{Element, SimpleElement};
use wot_shared_kernel::{
    ArmorMesh, ArmorPlate, ArmorTriangle, ExplosiveShell, GunLoading, GunPerformance, MountedArmor,
    ShellBallistics, ShellKind, ShellStun, VehiclePerformance,
};

const SHOOTER: ParticipantId = ParticipantId(1);
const VICTIM: ParticipantId = ParticipantId(2);

fn projectile_battle() -> (u32, Battle, Arc<ArenaWire>) {
    let path = std::env::var_os("WG_RESOURCE_DIR").unwrap();
    let resources = ResourceBundle::open(std::path::Path::new(&path)).unwrap();
    let dispatch = Arc::new(ScriptDispatch::new(
        wot_platform::mercury::script::load(&resources).unwrap(),
    ));
    let mut runtime = BattleRuntime::load(dispatch, &resources, true).unwrap();
    let wire = runtime.wire.clone().unwrap();
    let prepared = runtime
        .prepare(
            BattleLaunch {
                arena_type: (1 << 16) | 5,
                duration: Duration::from_secs(60),
                mode: BattleMode::Training,
                participants: [1_u8, 2]
                    .into_iter()
                    .map(|team| {
                        let mut vehicle = Entrant::sample().vehicle;
                        vehicle.performance = Some(profile());
                        BattleParticipant {
                            id: ParticipantId(u64::from(team)),
                            account_id: i64::from(team),
                            name: format!("Player {team}"),
                            team,
                            is_host: team == 1,
                            observer: false,
                            customization_display_type: 0,
                            vehicle,
                        }
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
    let crate::contract::PreparedBattle { id, mut battle } = prepared;
    battle.round.phase = RoundPhase::Battle;
    battle.ground = Arc::new(terrain::Terrain::flat());
    battle.obstacles = obstacles::Scenery::default();
    for (&id, participant) in &mut battle.players {
        participant.ready = true;
        participant.entered = true;
        participant.tank.position = if id == SHOOTER {
            Vec3::ZERO
        } else {
            Vec3::new(2.0, 0.0, 3.0)
        };
        participant.tank.yaw = 0.0;
        participant.tank.set_hull_tilt(Vec3::ZERO);
    }
    (id, battle, wire)
}

#[derive(Default)]
struct ImpactDelivery {
    indicators: Vec<(ParticipantId, AvatarUpdate)>,
}

impl ArenaDelivery for ImpactDelivery {
    fn geometry_address(&self) -> std::io::Result<std::net::SocketAddrV4> {
        unreachable!()
    }

    fn disconnect(&mut self, participant: ParticipantId) {
        panic!("Unexpected disconnect for {participant:?}");
    }

    fn create_avatar(&mut self, _: ParticipantId, _: u32, _: ScriptValue) -> std::io::Result<()> {
        unreachable!()
    }

    fn enable_arena_cell(&mut self, _: ParticipantId, _: u32) -> std::io::Result<()> {
        unreachable!()
    }

    fn send_avatar(
        &mut self,
        recipient: ParticipantId,
        update: AvatarUpdate,
    ) -> std::io::Result<()> {
        match update {
            AvatarUpdate::HitDirection { .. } => {
                let _delivery = tracing::info_span!("test.hit_direction_delivery").entered();
                self.indicators.push((recipient, update));
            }
            AvatarUpdate::BattleEvents(_) => panic!("Unexpected damage log"),
            _ => {}
        }
        Ok(())
    }

    fn send_arena_element<E: SimpleElement + Send + 'static>(
        &mut self,
        _: ParticipantId,
        _: E,
    ) -> std::io::Result<()> {
        Ok(())
    }

    fn send_arena_property(
        &mut self,
        _: ParticipantId,
        _: Option<u32>,
        _: u8,
        _: impl Element<()> + Send + 'static,
    ) -> std::io::Result<()> {
        Ok(())
    }

    fn arena_vehicle_method(
        &mut self,
        _: ParticipantId,
        _: u32,
        name: &str,
        _: Vec<ScriptValue>,
    ) -> std::io::Result<()> {
        assert_ne!(name, "onHealthChanged");
        Ok(())
    }

    fn publish_positions(&mut self, _: BattlePositions) {
        unreachable!()
    }
}

#[test]
#[ignore = "Requires WG_RESOURCE_DIR with the extracted EU client bundle"]
fn blocked_impacts_send_direction_only_to_the_victim() {
    use crate::domain::vehicle::Aim;

    let (id, mut battle, wire) = projectile_battle();
    let mut delivery = ImpactDelivery::default();
    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    let subscriber = tracing_subscriber::registry()
        .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("hit-direction-test")));
    let bearing = 1.5_f32.atan2(3.0);
    tracing::subscriber::with_default(subscriber, || {
        for (i, (ricochet_cos, damage_factor)) in
            [(0.0, 1.0), (0.9, 1.0), (0.0, 0.0)].into_iter().enumerate()
        {
            let now = battle.started_at + Duration::from_secs(i as u64 * 2);
            let target = &mut battle.players.get_mut(&VICTIM).unwrap().tank;
            let hull = target.performance.as_mut().unwrap().armor[1]
                .as_mut()
                .unwrap();
            for triangle in &mut Arc::get_mut(&mut hull.mesh).unwrap().triangles {
                triangle.plate.ricochets = true;
                triangle.plate.damage_factor = damage_factor;
            }
            let tank = &mut battle.players.get_mut(&SHOOTER).unwrap().tank;
            tank.performance
                .as_mut()
                .unwrap()
                .gun
                .shell_ballistics
                .insert(
                    10,
                    ShellBallistics {
                        kind: ShellKind::ArmorPiercing,
                        caliber_mm: 20.0,
                        penetration_mm: [1.0; 2],
                        penetration_randomization: 0.0,
                        damage_randomization: 0.0,
                        uniform_penetration: true,
                        uniform_damage: true,
                        far_damage: 100.0,
                        damage_limited_range: f32::INFINITY,
                        device_damage: [0.0; 2],
                        explosive: None,
                        speed: 100.0,
                        gravity: 0.0,
                        normalization: 0.0,
                        ricochet_cos,
                        trace_ricochet: true,
                        obstacle_damage: None,
                        obstacle_penetration_loss: 0.0,
                    },
                );
            tank.aim_at(Aim::HullAngles {
                yaw: bearing,
                pitch: 0.0,
            });
            tank.advance_gun(1.0);
            assert_eq!(
                battle.fire(&mut delivery, &wire, id, SHOOTER, now).unwrap(),
                "launched"
            );
            battle.advance_projectiles(&mut delivery, &wire, id, now + Duration::from_secs(1));
            let [
                (
                    VICTIM,
                    AvatarUpdate::HitDirection {
                        yaw,
                        attacker: 12,
                        damage: 100,
                        criticals: 0,
                        blocked: true,
                        explosive: false,
                        victim: 22,
                    },
                ),
            ] = delivery.indicators.as_slice()
            else {
                panic!("Expected one blocked hit direction for the victim");
            };
            assert!((*yaw - bearing).abs() < 0.001);
            assert_eq!(battle.players[&VICTIM].tank.health, 100);
            delivery.indicators.clear();
        }
        battle.players.get_mut(&SHOOTER).unwrap().pending_reload = None;
    });

    let spans = exporter.get_finished_spans().unwrap();
    let has = |span: &opentelemetry_sdk::trace::SpanData, key, value| {
        span.attributes
            .iter()
            .any(|field| field.key.as_str() == key && field.value.to_string() == value)
    };
    for outcome in ["armor_blocked", "ricochet", "screen_blocked"] {
        let shot = spans
            .iter()
            .find(|span| span.name == "battles.shot" && has(span, "output.outcome", outcome))
            .unwrap();
        assert!(has(shot, "output.damage", "0"));
        assert!(has(shot, "output.hit_direction.blocked", "true"));
        assert!(has(shot, "output.hit_direction.damage", "100"));
        assert_eq!(
            spans
                .iter()
                .filter(|delivery| delivery.name == "test.hit_direction_delivery"
                    && delivery.parent_span_id == shot.span_context.span_id()
                    && delivery.span_context.trace_id() == shot.span_context.trace_id())
                .count(),
            1,
        );
    }
}

#[test]
#[ignore = "Requires WG_RESOURCE_DIR with the extracted EU client bundle"]
fn ground_explosion_damages_and_stuns_vehicles_only_for_artillery() {
    let (id, mut battle, wire) = projectile_battle();
    let shooter = SHOOTER;
    let victim = VICTIM;
    let shot_profile = |artillery: bool| ShellBallistics {
        kind: ShellKind::HighExplosive {
            shield_penetration: false,
            obstacle_penetration: false,
        },
        caliber_mm: 100.0,
        penetration_mm: [100.0; 2],
        penetration_randomization: 0.0,
        damage_randomization: 0.0,
        uniform_penetration: false,
        uniform_damage: false,
        far_damage: 100.0,
        damage_limited_range: f32::INFINITY,
        device_damage: [0.0; 2],
        explosive: Some(ExplosiveShell {
            scenery_radius: 6.0,
            artillery_radius: if artillery { 6.0 } else { 0.0 },
            splash_factor: 0.5,
            edge_factor: 0.05,
            armor_absorption: 1.0,
            stun: artillery.then_some(ShellStun {
                radius: 6.0,
                duration: 10.0,
                guaranteed: 1.0,
                damage_coefficient: 0.0,
                minimum: 1.0,
                turret_traverse: 0.6,
            }),
            ..ExplosiveShell::default()
        }),
        speed: 100.0,
        gravity: if artillery { 20.0 } else { 0.0 },
        normalization: 0.0,
        ricochet_cos: 0.0,
        trace_ricochet: false,
        obstacle_damage: None,
        obstacle_penetration_loss: 0.0,
    };
    let (outbox, mut receiver) = tokio::sync::mpsc::channel(256);
    let mut delivery = QueuedDelivery {
        address: "127.0.0.1:20010".parse().unwrap(),
        outbox,
    };
    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    let subscriber = tracing_subscriber::registry()
        .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("explosion-test")));
    let now = battle.started_at;
    tracing::subscriber::with_default(subscriber, || {
        for (i, artillery) in [false, true].into_iter().enumerate() {
            let now = now + Duration::from_secs(i as u64 * 2);
            let tank = &mut battle.players.get_mut(&shooter).unwrap().tank;
            tank.performance
                .as_mut()
                .unwrap()
                .gun
                .shell_ballistics
                .insert(10, shot_profile(artillery));
            tank.aim_at(crate::domain::vehicle::Aim::HullAngles {
                yaw: 0.0,
                pitch: 0.4,
            });
            tank.advance_gun(1.0);
            assert_eq!(
                battle.fire(&mut delivery, &wire, id, shooter, now).unwrap(),
                "launched"
            );
            battle.advance_projectiles(&mut delivery, &wire, id, now + Duration::from_secs(1));
            let target = &battle.players[&victim];
            assert_eq!(target.tank.health < 100, artillery);
            assert_eq!(target.pending_stun.is_some(), artillery);
        }
        battle.expire_stuns(&mut delivery, &wire, now + Duration::from_secs(20));
        assert!(battle.players[&victim].pending_stun.is_none());
        battle.players.get_mut(&shooter).unwrap().pending_reload = None;
    });
    let delivered = std::iter::from_fn(|| receiver.try_recv().ok())
        .filter(|output| matches!(output, BattleOutput::Deliver(_)))
        .count();
    assert!(delivered > 0);
    let spans = exporter.get_finished_spans().unwrap();
    let splash = spans
        .iter()
        .find(|span| {
            span.name == "battles.explosion"
                && span.attributes.iter().any(|field| {
                    field.key.as_str() == "output.explosion.targets"
                        && field.value.to_string() == "1"
                })
        })
        .unwrap();
    let shot = spans
        .iter()
        .find(|span| {
            span.name == "battles.shot"
                && splash.parent_span_id == span.span_context.span_id()
                && splash.span_context.trace_id() == span.span_context.trace_id()
        })
        .unwrap();
    assert!(
        shot.attributes
            .iter()
            .any(|field| field.key.as_str() == "input.shell.gravity_mps2"
                && field.value.to_string() == "20")
    );
}

fn profile() -> VehiclePerformance {
    let plate = ArmorPlate {
        thickness_mm: 10.0,
        damage_factor: 1.0,
        angled: true,
        ricochets: false,
        caliber_ricochet: false,
        caliber_normalization: false,
        once: false,
        device: None,
    };
    let corners = [
        Vec3::new(-0.5, 0.0, -1.0),
        Vec3::new(-0.5, 2.0, -1.0),
        Vec3::new(-0.5, 2.0, 1.0),
        Vec3::new(-0.5, 0.0, 1.0),
    ];
    let [a, b, c, d] = corners;
    let armor = ArmorMesh {
        bounds: [Vec3::new(-0.5, 0.0, -1.0), Vec3::new(0.5, 2.0, 1.0)],
        triangles: [[a, b, c], [a, c, d]]
            .map(|vertices| ArmorTriangle {
                vertices,
                plate,
                material: 1,
            })
            .to_vec(),
    };
    VehiclePerformance {
        forward_speed: 10.0,
        reverse_speed: 5.0,
        hull_traverse: 1.0,
        mobility: Default::default(),
        turret_pivot: None,
        armor: [
            None,
            Some(MountedArmor {
                offset: Vec3::ZERO,
                mesh: Arc::new(armor),
            }),
            None,
            None,
        ],
        device_health: Default::default(),
        gun: GunPerformance {
            turret_traverse: 1.0,
            gun_traverse: 1.0,
            yaw_limits: None,
            min_pitch: vec![(0.0, -0.5), (1.0, -0.5)],
            max_pitch: vec![(0.0, 0.5), (1.0, 0.5)],
            aiming_time: 1.0,
            base_dispersion_angle: None,
            shot_position: None,
            loading: GunLoading {
                capacity: 1,
                reload_time: 1.0,
                intra_clip_time: 0.0,
                forced_reload_time: 0.0,
                autoreload_times: Vec::new(),
            },
            shell_damage: [(10, 100)].into(),
            shell_ranges: [(10, 100.0)].into(),
            shell_effects: Default::default(),
            shell_ballistics: Default::default(),
        },
    }
}
