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
    ShellBallistics, ShellEffects, ShellKind, ShellStun, VehiclePerformance,
};

const SHOOTER: ParticipantId = ParticipantId(1);
const VICTIM: ParticipantId = ParticipantId(2);
const NEARBY_VICTIM: ParticipantId = ParticipantId(3);

fn projectile_battle(teams: &[u8]) -> (u32, Battle, Arc<ArenaWire>) {
    let path = std::env::var_os("WOT_RESOURCE_DIR").unwrap();
    let resources = ResourceBundle::open(std::path::Path::new(&path)).unwrap();
    let dispatch = Arc::new(ScriptDispatch::new(
        wot_platform::mercury::script::load(&resources).unwrap(),
    ));
    let mut runtime = BattleRuntime::load(dispatch, &resources, true).unwrap();
    let wire = runtime.wire.clone().unwrap();
    let prepared = runtime
        .prepare(
            BattleLaunch {
                parameters: Default::default(),
                arena_type: (1 << 16) | 5,
                duration: Duration::from_secs(60),
                mode: BattleMode::Training,
                participants: teams
                    .iter()
                    .enumerate()
                    .map(|(index, &team)| {
                        let player = index as u64 + 1;
                        let mut vehicle = Entrant::sample().vehicle;
                        vehicle.performance = Some(profile());
                        BattleParticipant {
                            id: ParticipantId(player),
                            account_id: player as i64,
                            name: format!("Player {player}"),
                            team,
                            is_host: index == 0,
                            observer: false,
                            stationary_dummy: false,
                            customization_display_type: 0,
                            vehicle,
                        }
                    })
                    .collect(),
            },
            (1..=teams.len() as u32)
                .map(|player| BattleEntityIds {
                    avatar: player * 10 + 1,
                    vehicle: player * 10 + 2,
                    info: player * 10 + 3,
                })
                .collect(),
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
    stickers: Vec<(ParticipantId, Vec<u8>)>,
    tracers: Vec<(i32, bool, f32)>,
    stops: Vec<i32>,
    health_changes: Vec<u32>,
    damage_logs: usize,
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
            AvatarUpdate::BattleEvents(_) => self.damage_logs += 1,
            AvatarUpdate::Tracer {
                id,
                ricochet,
                range,
                ..
            } if recipient == SHOOTER => {
                let _delivery = tracing::info_span!("test.tracer_delivery").entered();
                self.tracers.push((id, ricochet, range));
            }
            AvatarUpdate::StopTracer { id, .. } if recipient == SHOOTER => {
                self.stops.push(id);
            }
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
        recipient: ParticipantId,
        entity: Option<u32>,
        _: u8,
        property: impl Element<()> + Send + 'static,
    ) -> std::io::Result<()> {
        if entity == Some(22) {
            let _delivery = tracing::info_span!("test.damage_sticker_delivery").entered();
            let mut payload = Vec::new();
            property.write(&mut payload, &())?;
            self.stickers.push((recipient, payload));
        }
        Ok(())
    }

    fn arena_vehicle_method(
        &mut self,
        _: ParticipantId,
        entity: u32,
        name: &str,
        _: Vec<ScriptValue>,
    ) -> std::io::Result<()> {
        if name == "onHealthChanged" {
            let _delivery = tracing::info_span!("test.health_delivery").entered();
            self.health_changes.push(entity);
        }
        Ok(())
    }

    fn publish_positions(&mut self, _: BattlePositions) {
        unreachable!()
    }
}

#[test]
#[ignore = "Requires WOT_RESOURCE_DIR with the extracted EU client bundle"]
fn blocked_impacts_publish_direction_and_vehicle_marks() {
    use crate::domain::vehicle::Aim;

    let (id, mut battle, wire) = projectile_battle(&[1, 2]);
    let tank = &mut battle.players.get_mut(&SHOOTER).unwrap().tank;
    tank.performance.as_mut().unwrap().gun.shell_effects.insert(
        10,
        ShellEffects {
            caliber_mm: 20.0,
            pierced_sticker: Some(12),
            resisted_sticker: Some(1),
            prefab_index: 2,
            ..ShellEffects::default()
        },
    );
    let mut delivery = ImpactDelivery::default();
    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    let subscriber = tracing_subscriber::registry()
        .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("hit-direction-test")));
    tracing::subscriber::with_default(subscriber, || {
        for (i, (ricochet_cos, damage_factor, hit_effect)) in
            [(0.0, 1.0, 3), (0.9, 1.0, 2), (0.0, 0.0, 3)]
                .into_iter()
                .enumerate()
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
            let gun = &mut tank.performance.as_mut().unwrap().gun;
            gun.shell_ballistics.insert(
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
            let bearing = 1.5_f32.atan2(3.0 + 0.25 * i as f32);
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
            assert!(delivery.health_changes.is_empty());
            assert_eq!(delivery.damage_logs, 0);
            delivery.indicators.clear();
            delivery.stickers.sort_by_key(|(recipient, _)| recipient.0);
            let [(SHOOTER, shooter_marks), (VICTIM, victim_marks)] = delivery.stickers.as_slice()
            else {
                panic!("Expected the vehicle sticker array for both viewers");
            };
            assert_eq!(shooter_marks, victim_marks);
            assert_eq!(victim_marks[0] as usize, i + 1);
            assert_eq!(victim_marks.len(), 1 + 16 * (i + 1));
            let mark = &victim_marks[1 + 16 * i..];
            assert_eq!(&mark[..4], &[0; 4]);
            assert_eq!(mark[4], 1);
            assert_eq!(mark[5], 1);
            assert_ne!(mark[6..9], mark[9..12]);
            assert_eq!(
                u32::from_le_bytes(mark[12..].try_into().unwrap()),
                (2 << 24) | (200 << 8) | (2 << 4) | hit_effect,
            );
            delivery.stickers.clear();
        }
        battle.players.get_mut(&SHOOTER).unwrap().pending_reload = None;
    });

    let spans = exporter.get_finished_spans().unwrap();
    let has = |span: &opentelemetry_sdk::trace::SpanData, key, value| {
        span.attributes
            .iter()
            .any(|field| field.key.as_str() == key && field.value.to_string() == value)
    };
    let child_of = |child: &opentelemetry_sdk::trace::SpanData,
                    parent: &opentelemetry_sdk::trace::SpanData| {
        child.parent_span_id == parent.span_context.span_id()
            && child.span_context.trace_id() == parent.span_context.trace_id()
    };
    for (outcome, count) in [
        ("armor_blocked", "1"),
        ("ricochet", "2"),
        ("screen_blocked", "3"),
    ] {
        let shot = spans
            .iter()
            .find(|span| span.name == "battles.shot" && has(span, "output.outcome", outcome))
            .unwrap();
        assert!(has(shot, "output.damage", "0"));
        assert!(has(shot, "output.hit_direction.blocked", "true"));
        assert!(has(shot, "output.hit_direction.damage", "100"));
        assert!(has(shot, "output.damage_stickers.count", count));
        let stickers: Vec<_> = spans
            .iter()
            .filter(|span| span.name == "battles.damage_stickers" && child_of(span, shot))
            .collect();
        assert_eq!(stickers.len(), 2);
        for sticker in stickers {
            assert!(has(sticker, "input.entity_id", "22"));
            assert!(has(sticker, "input.damage_stickers.count", count));
            assert!(
                spans
                    .iter()
                    .any(|delivery| delivery.name == "test.damage_sticker_delivery"
                        && child_of(delivery, sticker))
            );
        }
        assert_eq!(
            spans
                .iter()
                .filter(|delivery| delivery.name == "test.hit_direction_delivery"
                    && child_of(delivery, shot))
                .count(),
            1,
        );
    }
}

#[test]
#[ignore = "Requires WOT_RESOURCE_DIR with the extracted EU client bundle"]
fn ricochet_continues_to_a_second_plate_within_ten_calibers() {
    for (first_part, second_part, second_target, second_entity, distance, penetrates) in [
        (2, 1, VICTIM, 22, 0.75, true),
        (1, 2, VICTIM, 22, 0.75, true),
        (2, 1, NEARBY_VICTIM, 32, 0.99, true),
        (2, 1, NEARBY_VICTIM, 32, 1.01, false),
    ] {
        let (id, mut battle, wire) = projectile_battle(&[1, 2, 2]);
        for participant in [VICTIM, NEARBY_VICTIM] {
            battle
                .players
                .get_mut(&participant)
                .unwrap()
                .tank
                .performance
                .as_mut()
                .unwrap()
                .armor = Default::default();
        }
        let shooter = &mut battle.players.get_mut(&SHOOTER).unwrap().tank;
        let gun = &mut shooter.performance.as_mut().unwrap().gun;
        gun.shell_damage.insert(10, 40);
        gun.shell_ballistics.insert(
            10,
            ShellBallistics {
                kind: ShellKind::ArmorPiercing,
                caliber_mm: 100.0,
                penetration_mm: [90.0; 2],
                penetration_randomization: 0.0,
                damage_randomization: 0.0,
                uniform_penetration: false,
                uniform_damage: false,
                far_damage: 40.0,
                damage_limited_range: f32::INFINITY,
                device_damage: [0.0; 2],
                explosive: None,
                speed: 100.0,
                gravity: 0.0,
                normalization: 0.0,
                ricochet_cos: 70.0_f32.to_radians().cos(),
                trace_ricochet: true,
                obstacle_damage: None,
                obstacle_penetration_loss: 0.0,
            },
        );
        let ray = shooter.gun_ray();
        let contact = ray.origin + ray.direction * 3.0;
        let normal = Vec3::new(
            15.0_f32.to_radians().cos(),
            0.0,
            -15.0_f32.to_radians().sin(),
        );
        let reflected = Vec3::new(0.5, 0.0, 3.0_f32.sqrt() / 2.0);
        mount_test_plate(&mut battle, VICTIM, first_part, contact, normal, true);
        mount_test_plate(
            &mut battle,
            second_target,
            second_part,
            contact + reflected * distance,
            -reflected,
            false,
        );

        let mut delivery = ImpactDelivery::default();
        let exporter = InMemorySpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let subscriber = tracing_subscriber::registry()
            .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("ricochet-test")));
        tracing::subscriber::with_default(subscriber, || {
            let now = battle.started_at;
            assert_eq!(
                battle.fire(&mut delivery, &wire, id, SHOOTER, now).unwrap(),
                "launched"
            );
            battle.advance_projectiles(&mut delivery, &wire, id, now + Duration::from_millis(32));
            assert_eq!(battle.players[&VICTIM].tank.health, 100);
            assert_eq!(battle.players[&NEARBY_VICTIM].tank.health, 100);
            assert!(matches!(
                delivery.indicators.as_slice(),
                [(
                    VICTIM,
                    AvatarUpdate::HitDirection {
                        blocked: true,
                        victim: 22,
                        ..
                    }
                )]
            ));
            let &[(direct_id, false, _), (reflected_id, true, range)] = delivery.tracers.as_slice()
            else {
                panic!("Expected a direct and a reflected tracer");
            };
            assert_ne!(direct_id, reflected_id);
            assert!((range - 0.998).abs() < 1e-6);
            assert_eq!(delivery.stops, [direct_id]);

            battle.advance_projectiles(&mut delivery, &wire, id, now + Duration::from_millis(50));
            if penetrates {
                assert_eq!(battle.players[&second_target].tank.health, 60);
                assert!(matches!(
                    delivery.indicators.get(1),
                    Some((target, AvatarUpdate::HitDirection { blocked: false, damage: 40, victim, .. }))
                        if *target == second_target && *victim == second_entity
                ));
                assert_eq!(delivery.stops, [direct_id, reflected_id]);
                assert_eq!(delivery.health_changes, [second_entity; 3]);
                assert_eq!(delivery.damage_logs, 2);
            } else {
                assert_eq!(battle.players[&second_target].tank.health, 100);
                assert_eq!(delivery.indicators.len(), 1);
                assert_eq!(delivery.stops, [direct_id]);
                assert!(delivery.health_changes.is_empty());
                assert_eq!(delivery.damage_logs, 0);
            }
            battle.players.get_mut(&SHOOTER).unwrap().pending_reload = None;
        });

        let spans = exporter.get_finished_spans().unwrap();
        let shot = spans
            .iter()
            .find(|span| span.name == "battles.shot")
            .unwrap();
        let field = |span: &opentelemetry_sdk::trace::SpanData, key: &str| {
            span.attributes
                .iter()
                .rfind(|field| field.key.as_str() == key)
                .map(|field| field.value.to_string())
                .unwrap()
        };
        assert_eq!(field(shot, "output.projectile.ricocheted"), "true");
        let traced_range: f64 = field(shot, "output.projectile.ricochet_range_metres")
            .parse()
            .unwrap();
        assert!((traced_range - 0.998).abs() < 1e-6);
        assert_eq!(
            field(shot, "output.outcome"),
            if penetrates { "penetrated" } else { "ricochet" }
        );
        let children = |name: &str| {
            spans
                .iter()
                .filter(|span| span.name == name)
                .inspect(|span| {
                    assert_eq!(span.parent_span_id, shot.span_context.span_id());
                    assert_eq!(span.span_context.trace_id(), shot.span_context.trace_id());
                })
                .collect::<Vec<_>>()
        };
        let armor = children("battles.resolve_armor");
        assert_eq!(armor.len(), if penetrates { 2 } else { 1 });
        assert_eq!(field(armor[0], "output.outcome"), "ricochet");
        assert_eq!(field(armor[0], "output.part.index"), first_part.to_string());
        if penetrates {
            assert_eq!(field(armor[1], "output.outcome"), "penetrated");
            assert_eq!(
                field(armor[1], "output.part.index"),
                second_part.to_string()
            );
            assert_eq!(
                field(armor[1], "input.target.entity_id"),
                second_entity.to_string()
            );
            assert_eq!(field(armor[1], "output.penetration.remaining_mm"), "17.5");
        }
        assert_eq!(children("test.tracer_delivery").len(), 2);
        assert_eq!(
            children("test.health_delivery").len(),
            if penetrates { 3 } else { 0 }
        );
    }
}

fn mount_test_plate(
    battle: &mut Battle,
    participant: ParticipantId,
    part: usize,
    center: Vec3,
    normal: Vec3,
    ricochets: bool,
) {
    let tank = &mut battle.players.get_mut(&participant).unwrap().tank;
    let (origin, rotation) = tank.armor_pose(part, Vec3::ZERO);
    let across = Vec3::Y.cross(normal).normalize() * 0.15;
    let up = Vec3::Y * 0.15;
    let [a, b, c, d] = [-across - up, across - up, across + up, -across + up]
        .map(|offset| rotation.inverse() * (center + offset - origin));
    let padding = Vec3::splat(0.001);
    tank.performance.as_mut().unwrap().armor[part] = Some(MountedArmor {
        offset: Vec3::ZERO,
        mesh: Arc::new(ArmorMesh {
            bounds: [
                a.min(b).min(c).min(d) - padding,
                a.max(b).max(c).max(d) + padding,
            ],
            triangles: [[a, b, c], [a, c, d]]
                .map(|vertices| ArmorTriangle {
                    vertices,
                    material: 1,
                    plate: ArmorPlate {
                        thickness_mm: 50.0,
                        damage_factor: 1.0,
                        angled: ricochets,
                        ricochets,
                        caliber_ricochet: true,
                        caliber_normalization: false,
                        once: false,
                        device: None,
                    },
                })
                .to_vec(),
        }),
    });
}

#[test]
#[ignore = "Requires WOT_RESOURCE_DIR with the extracted EU client bundle"]
fn ground_explosion_damages_and_stuns_vehicles_only_for_artillery() {
    let (id, mut battle, wire) = projectile_battle(&[1, 2]);
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
