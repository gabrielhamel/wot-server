use super::*;
use crate::{
    application::BattleParticipant,
    infrastructure::{
        avatar::ShotResult,
        delivery::{BattlePositions, QueuedDelivery},
        worker::BattleOutput,
    },
};
use opentelemetry::trace::TracerProvider;
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};
use tracing_subscriber::prelude::*;
use wot_platform::mercury::{
    app::client::element::arena::{ApplyNrlUnlink, ArenaEntityProperties, NrlCreateNode, NrlData},
    net::{
        codec::Codec,
        element::{Element, SimpleElement},
    },
};
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
    let visible = battle
        .players
        .values()
        .map(|participant| participant.vehicle.entity_id)
        .collect::<HashSet<_>>();
    for (&id, participant) in &mut battle.players {
        participant.visible_vehicles = visible.clone();
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
    fire_damage: Vec<(ParticipantId, Vec<ScriptValue>)>,
    fire_logs: Vec<(ParticipantId, avatar::BattleEvent)>,
    damage_logs: usize,
    shot_results: Vec<(ParticipantId, ShotResult)>,
    track_updates: Vec<(ParticipantId, u8, Vec<u8>)>,
    impacts: Vec<(ParticipantId, Vec<ScriptValue>)>,
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
            AvatarUpdate::BattleEvents(event) => {
                self.damage_logs += 1;
                if (event.details >> 17) & 255 == 1 {
                    self.fire_logs.push((recipient, event));
                }
            }
            AvatarUpdate::ShotResults(result) => {
                self.shot_results.push((recipient, result));
            }
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
        recipient: ParticipantId,
        element: E,
    ) -> std::io::Result<()> {
        if matches!(
            E::ID,
            NrlData::ID
                | ArenaEntityProperties::ID
                | NrlCreateNode::ID
                | ApplyNrlUnlink::ID
                | CreateArenaEntity::ID
                | LeaveArenaEntity::ID
        ) {
            let mut payload = Vec::new();
            element.write(&mut payload, &())?;
            let _delivery = tracing::info_span!("test.track_delivery").entered();
            self.track_updates.push((recipient, E::ID, payload));
        }
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
        recipient: ParticipantId,
        entity: u32,
        name: &str,
        args: Vec<ScriptValue>,
    ) -> std::io::Result<()> {
        if name == "onHealthChanged" {
            let _delivery = tracing::info_span!("test.health_delivery").entered();
            self.health_changes.push(entity);
            if args.get(3) == Some(&ScriptValue::UInt8(1)) {
                self.fire_damage.push((recipient, args));
            }
            return Ok(());
        }
        if name == "showDamageFromShot" {
            let _delivery = tracing::info_span!("test.impact_delivery").entered();
            self.impacts.push((recipient, args));
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
            engine_compartment: None,
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
        snapshot_queue_drops: &std::sync::atomic::AtomicU64::new(0),
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
        engine_compartment: None,
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
        spotting: None,
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

fn owner_device_updates(
    delivery: &ImpactDelivery,
    wire: &ArenaWire,
    name: &str,
) -> Vec<Vec<BTreeMap<Arc<str>, ScriptValue>>> {
    let own = wire.dispatch.entity_from_name("OwnVehicle").unwrap().1;
    let (index, property) = own
        .properties
        .iter()
        .enumerate()
        .find(|(_, property)| property.name.as_ref() == name)
        .unwrap();
    let node = (1026 + index as u16).to_be_bytes();
    delivery
        .track_updates
        .iter()
        .filter_map(|(recipient, id, bytes)| {
            if *id != NrlData::ID || !bytes.starts_with(&node) {
                return None;
            }
            assert_eq!(*recipient, VICTIM);
            let mut body = &bytes[4..];
            assert_eq!(usize::from(bytes[3]), body.len());
            let ScriptValue::Seq(rows) = ScriptValue::read(&mut body, &property.ty).unwrap() else {
                panic!("Expected a device property array");
            };
            Some(
                rows.into_iter()
                    .map(|row| {
                        let ScriptValue::Dict(fields) = row else {
                            panic!("Expected a device row")
                        };
                        fields
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>()
}

#[test]
#[ignore = "Requires WOT_RESOURCE_DIR with the extracted EU client bundle"]
fn shot_breaks_both_tracks_and_repair_restores_drive_with_client_feedback() {
    use crate::domain::{devices::TRACKS, movement::GroundContact, vehicle::Aim};
    use glam::Vec2;
    use wot_shared_kernel::{ArmorDevice, DeviceHealth};

    let (id, mut battle, wire) = projectile_battle(&[1, 2]);
    let extras = wire.track_extras;
    let target = &mut battle.players.get_mut(&VICTIM).unwrap().tank;
    target.position = Vec3::new(8.0, 0.0, 0.0);
    let target_profile = target.performance.as_mut().unwrap();
    let hull = Arc::get_mut(&mut target_profile.armor[1].as_mut().unwrap().mesh).unwrap();
    let entry = hull.triangles.clone();
    hull.triangles.extend(entry.iter().map(|entry| {
        let mut exit = *entry;
        exit.vertices.iter_mut().for_each(|point| point.x += 1.0);
        exit.vertices.swap(1, 2);
        exit
    }));
    for triangle in &mut hull.triangles {
        triangle.plate.damage_factor = 0.02;
    }
    hull.bounds[0].x = -0.75;
    hull.bounds[1].x = 0.75;
    for (side, device) in TRACKS.into_iter().enumerate() {
        hull.triangles.extend(entry.iter().map(|entry| {
            let mut track = *entry;
            track
                .vertices
                .iter_mut()
                .for_each(|point| point.x = [-0.75, 0.75][side]);
            track.material = 2 + side as u32;
            track.plate.damage_factor = 0.0;
            track.plate.once = true;
            track.plate.device = Some(ArmorDevice {
                device,
                can_ignite_engine: false,
                health: None,
                projectile_chance: 1.0,
                explosion_chance: 0.0,
            });
            track
        }));
        target_profile.device_health.insert(
            device,
            DeviceHealth {
                maximum: 100,
                repaired: 50,
                repair_seconds: [1.0, 2.0][side],
                fire_chance: 0.0,
            },
        );
    }
    let tank = &mut battle.players.get_mut(&SHOOTER).unwrap().tank;
    let gun = &mut tank.performance.as_mut().unwrap().gun;
    gun.shell_damage.insert(10, 60);
    gun.loading.reload_time = 0.1;
    gun.shell_ballistics.insert(
        10,
        ShellBallistics {
            kind: ShellKind::ArmorPiercing,
            caliber_mm: 90.0,
            penetration_mm: [100.0; 2],
            far_damage: 60.0,
            device_damage: [1.0; 2],
            speed: 100.0,
            penetration_randomization: 0.0,
            damage_randomization: 0.0,
            uniform_penetration: true,
            uniform_damage: true,
            damage_limited_range: f32::INFINITY,
            explosive: None,
            gravity: 0.0,
            normalization: 0.0,
            ricochet_cos: 0.0,
            trace_ricochet: false,
            obstacle_damage: None,
            obstacle_penetration_loss: 0.0,
        },
    );
    assert!(tank.aim_at(Aim::HullAngles {
        yaw: std::f32::consts::FRAC_PI_2,
        pitch: 0.0
    }));
    tank.advance_gun(2.0);

    let own = wire.dispatch.entity_from_name("OwnVehicle").unwrap().1;
    let property_updates =
        |delivery: &ImpactDelivery, name| owner_device_updates(delivery, &wire, name);
    let vehicle = wire.dispatch.entity_from_name("Vehicle").unwrap().1;
    let public_index = vehicle
        .properties
        .iter()
        .position(|property| property.name.as_ref() == "publicStateModifiers")
        .unwrap() as u8;
    let public_updates = |delivery: &ImpactDelivery| {
        let mut updates = delivery
            .track_updates
            .iter()
            .filter(|(_, id, bytes)| {
                *id == ArenaEntityProperties::ID && bytes.starts_with(&[1, public_index])
            })
            .map(|(recipient, _, bytes)| (*recipient, bytes[2..].to_vec()))
            .collect::<Vec<_>>();
        updates.sort_by_key(|(recipient, _)| recipient.0);
        updates
    };

    let mut delivery = ImpactDelivery::default();
    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    let subscriber = tracing_subscriber::registry()
        .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("track-test")));
    tracing::subscriber::with_default(subscriber, || {
        let mut now = battle.started_at;
        for damage_code in [3, 4] {
            assert_eq!(
                battle.fire(&mut delivery, &wire, id, SHOOTER, now).unwrap(),
                "launched"
            );
            now += Duration::from_millis(200);
            battle.advance_projectiles(&mut delivery, &wire, id, now);
            let (recipient, result) = delivery.shot_results.pop().unwrap();
            assert_eq!(recipient, SHOOTER);
            assert_ne!(result.hitFlags & (1 << 11), 0);
            let rows = property_updates(&delivery, "vehicleDamageInfoList");
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].len(), 2);
            for (row, extra) in rows[0].iter().zip(extras) {
                assert_eq!(row["extraIndex"], ScriptValue::UInt8(extra));
                assert_eq!(row["damageIndex"], ScriptValue::UInt8(damage_code));
                assert_eq!(row["entityID"], ScriptValue::Int32(12));
            }
            let target = &battle.players[&VICTIM].tank;
            assert_eq!(target.condition.immobilized(), damage_code == 4);
            assert_eq!(target.health, if damage_code == 3 { 99 } else { 98 });
            if damage_code == 3 {
                assert!(public_updates(&delivery).is_empty());
                delivery.track_updates.clear();
                now += Duration::from_secs(2);
            }
        }
        let node = |name| {
            (1026
                + own
                    .properties
                    .iter()
                    .position(|p| p.name.as_ref() == name)
                    .unwrap() as u16)
                .to_be_bytes()
        };
        let nrl = &delivery.track_updates;
        let update_position = |name| {
            nrl.iter()
                .position(|(_, id, payload)| *id == NrlData::ID && payload.starts_with(&node(name)))
                .unwrap()
        };
        assert!(
            update_position("vehicleDamageInfoList")
                < update_position("destroyedDevicesIsRepairing")
        );
        let rows = property_updates(&delivery, "destroyedDevicesIsRepairing");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].len(), 2);
        for (row, seconds) in rows[0].iter().zip([1.0, 2.0]) {
            assert_eq!(row["progress"], ScriptValue::UInt8(0));
            assert_eq!(row["timeLeft"], ScriptValue::Float32(seconds));
        }
        let both_broken = vec![2, extras[0], extras[1]];
        assert_eq!(
            public_updates(&delivery),
            [(SHOOTER, both_broken.clone()), (VICTIM, both_broken)]
        );
        let ground = GroundContact {
            surfaces: [4, 0, 0],
            pitch: 0.0,
        };
        let bounds = [Vec2::splat(-100.0), Vec2::splat(100.0)];
        let target = &mut battle.players.get_mut(&VICTIM).unwrap().tank;
        assert!(target.drive(1 | 4));
        assert_eq!(target.engine_mode(), [1, 0]);
        assert_eq!(target.scenery_push_load(0.1, false), 0.0);
        assert_eq!(target.scenery_push_load(0.1, true), 0.0);
        assert!(!target.step(0.1, bounds, ground));

        // Stop the follow-up shot at the hull after it strikes the near track.
        let hull = target.performance.as_mut().unwrap().armor[1]
            .as_mut()
            .unwrap();
        for triangle in &mut Arc::get_mut(&mut hull.mesh).unwrap().triangles {
            if triangle.plate.damage_factor > 0.0 {
                triangle.plate.thickness_mm = 200.0;
            }
        }
        delivery.track_updates.clear();
        now += Duration::from_millis(300);
        assert_eq!(
            battle.fire(&mut delivery, &wire, id, SHOOTER, now).unwrap(),
            "launched"
        );
        now += Duration::from_millis(200);
        battle.advance_projectiles(&mut delivery, &wire, id, now);
        assert_eq!(battle.players[&VICTIM].tank.health, 98);
        assert!(property_updates(&delivery, "vehicleDamageInfoList").is_empty());
        assert!(public_updates(&delivery).is_empty());
        let timers = property_updates(&delivery, "destroyedDevicesIsRepairing");
        assert_eq!(timers.len(), 1);
        assert_eq!(timers[0].len(), 2);
        for ((row, extra), (seconds, progress)) in
            timers[0].iter().zip(extras).zip([(1.0, 0), (1.5, 25)])
        {
            assert_eq!(row["extraIndex"], ScriptValue::UInt8(extra));
            assert_eq!(row["timeLeft"], ScriptValue::Float32(seconds));
            assert_eq!(row["progress"], ScriptValue::UInt8(progress));
        }

        delivery.track_updates.clear();
        now += Duration::from_millis(500);
        battle.advance_damage(&mut delivery, &wire, id, now);
        assert!(delivery.track_updates.is_empty());
        assert!(battle.players[&VICTIM].tank.condition.immobilized());

        for (side, state) in [vec![1, extras[1]], vec![0]].into_iter().enumerate() {
            delivery.track_updates.clear();
            now += Duration::from_millis(500);
            battle.advance_damage(&mut delivery, &wire, id, now);
            let rows = property_updates(&delivery, "vehicleDamageInfoList");
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].len(), 1);
            assert_eq!(rows[0][0]["extraIndex"], ScriptValue::UInt8(extras[side]));
            assert_eq!(rows[0][0]["damageIndex"], ScriptValue::UInt8(15));
            assert_eq!(
                public_updates(&delivery),
                [(SHOOTER, state.clone()), (VICTIM, state)]
            );
            assert_eq!(
                battle
                    .players
                    .get_mut(&VICTIM)
                    .unwrap()
                    .tank
                    .step(0.1, bounds, ground),
                side == 1
            );
        }
        battle.players.get_mut(&SHOOTER).unwrap().pending_reload = None;
    });

    let spans = exporter.get_finished_spans().unwrap();
    let has = |span: &opentelemetry_sdk::trace::SpanData, key, value| {
        span.attributes
            .iter()
            .any(|field| field.key.as_str() == key && field.value.to_string() == value)
    };
    let shot = spans
        .iter()
        .find(|span| span.name == "battles.shot" && has(span, "output.tracks.left", "broken"))
        .unwrap();
    assert!(has(shot, "output.tracks.right", "broken"));
    let armor = spans
        .iter()
        .find(|span| {
            span.name == "battles.resolve_armor"
                && span.parent_span_id == shot.span_context.span_id()
        })
        .unwrap();
    assert!(has(armor, "output.tracks.left_damage_hp", "60"));
    assert!(has(armor, "output.tracks.right_damage_hp", "60"));

    let restarted = spans
        .iter()
        .find(|span| {
            span.name == "battles.shot" && has(span, "output.tracks.left", "repair_restarted")
        })
        .unwrap();
    assert!(has(restarted, "output.devices.destroyed", "0"));
    assert!(has(restarted, "output.tracks.hit_count", "1"));
    assert!(spans.iter().any(|span| span.name == "test.track_delivery"
        && span.parent_span_id == restarted.span_context.span_id()));
    for (side, parent) in [("left", restarted), ("right", shot)] {
        let repair = spans
            .iter()
            .find(|span| span.name == "battles.repair_track" && has(span, "input.track.side", side))
            .unwrap();
        assert_eq!(repair.parent_span_id, parent.span_context.span_id());
        assert_eq!(
            repair.span_context.trace_id(),
            parent.span_context.trace_id()
        );
        assert!(has(repair, "output.outcome", "repaired_critical"));
        assert!(has(repair, "output.recipients", "2"));
        assert!(spans.iter().any(|span| span.name == "test.track_delivery"
            && span.parent_span_id == repair.span_context.span_id()));
    }
}

#[test]
#[ignore = "Requires WOT_RESOURCE_DIR with the extracted EU client bundle"]
fn track_only_hits_show_a_chassis_critical_even_when_the_hull_ricochets() {
    use crate::domain::vehicle::Aim;
    use wot_shared_kernel::{ArmorDevice, Device, DeviceHealth};

    for (ricochet_cos, outcome, contact_flag) in [
        (0.0, "screen_penetrated", 1 << 6),
        (0.0, "armor_blocked", 1 << 5),
        (1.0, "ricochet", 1 << 3),
    ] {
        let (id, mut battle, wire) = projectile_battle(&[1, 2]);
        let target = &mut battle.players.get_mut(&VICTIM).unwrap().tank;
        target.position = Vec3::new(8.0, 0.0, 0.0);
        let profile = target.performance.as_mut().unwrap();
        let hull = Arc::get_mut(&mut profile.armor[1].as_mut().unwrap().mesh).unwrap();
        for triangle in &mut hull.triangles {
            triangle.plate.ricochets = true;
        }
        let mut tracks = hull.triangles.clone();
        for triangle in &mut tracks {
            triangle.plate.thickness_mm = 1.0;
            triangle.plate.damage_factor = 0.0;
            triangle.plate.ricochets = false;
            triangle.plate.device = Some(ArmorDevice {
                device: Device::TrackLeft,
                can_ignite_engine: false,
                health: Some(DeviceHealth {
                    maximum: 80,
                    repaired: 40,
                    repair_seconds: 2.0,
                    fire_chance: 0.0,
                }),
                projectile_chance: 1.0,
                explosion_chance: 1.0,
            });
        }
        profile.armor[0] = Some(MountedArmor {
            offset: Vec3::new(-0.25, 0.0, 0.0),
            mesh: Arc::new(ArmorMesh {
                engine_compartment: None,
                bounds: hull.bounds,
                triangles: tracks,
            }),
        });
        if outcome == "screen_penetrated" {
            profile.armor[1] = None;
        }
        let shooter = &mut battle.players.get_mut(&SHOOTER).unwrap().tank;
        shooter
            .performance
            .as_mut()
            .unwrap()
            .gun
            .shell_ballistics
            .insert(
                10,
                ShellBallistics {
                    kind: ShellKind::ArmorPiercing,
                    caliber_mm: 20.0,
                    penetration_mm: [5.0; 2],
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
        assert!(shooter.aim_at(Aim::HullAngles {
            yaw: std::f32::consts::FRAC_PI_2,
            pitch: 0.0
        }));
        shooter.advance_gun(2.0);

        let mut delivery = ImpactDelivery::default();
        let exporter = InMemorySpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let subscriber = tracing_subscriber::registry()
            .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("track-only-hit")));
        tracing::subscriber::with_default(subscriber, || {
            let now = battle.started_at;
            assert_eq!(
                battle.fire(&mut delivery, &wire, id, SHOOTER, now).unwrap(),
                "launched"
            );
            battle.advance_projectiles(&mut delivery, &wire, id, now + Duration::from_millis(200));
            let target = &battle.players[&VICTIM].tank;
            assert_eq!(target.health, 100);
            assert!(target.condition.disabled(Device::TrackLeft));
        });
        drop(battle);

        let [(SHOOTER, result)] = delivery.shot_results.as_slice() else {
            panic!("Expected the shooter's shot result");
        };
        assert_eq!(
            result.hitFlags,
            (1 << 20) | (1 << 11) | (1 << 8) | contact_flag
        );
        assert!(matches!(
            delivery.indicators.as_slice(),
            [(
                VICTIM,
                AvatarUpdate::HitDirection {
                    damage: 0,
                    criticals: 0x10000,
                    blocked: false,
                    ..
                }
            ),]
        ));
        assert!(delivery.health_changes.is_empty());
        assert_eq!(delivery.impacts.len(), 2);
        for (_, args) in &delivery.impacts {
            let ScriptValue::Seq(points) = &args[1] else {
                panic!("Expected impact points");
            };
            let [ScriptValue::Dict(point)] = points.as_slice() else {
                panic!("Expected one track impact");
            };
            let ScriptValue::UInt64(segment) = point["segment"] else {
                panic!("Expected packed segment");
            };
            let [effect, part, ..] = segment.to_le_bytes();
            assert_eq!((effect, part), (5, 0));
            assert_eq!(point["params"].as_i64().unwrap() & 0xf, 5);
            assert_eq!(args[4], ScriptValue::Int32(0));
            assert_eq!(args[6], ScriptValue::UInt8(0));
        }

        let spans = exporter.get_finished_spans().unwrap();
        let shot = spans
            .iter()
            .find(|span| span.name == "battles.shot")
            .unwrap();
        for (key, value) in [
            ("output.outcome", outcome),
            ("output.hit_direction.blocked", "false"),
            ("output.hit_direction.critical_flags", "65536"),
        ] {
            assert!(
                shot.attributes
                    .iter()
                    .any(|field| field.key.as_str() == key && field.value.to_string() == value)
            );
        }
        for name in ["test.impact_delivery", "test.hit_direction_delivery"] {
            assert!(spans.iter().any(
                |span| span.name == name && span.parent_span_id == shot.span_context.span_id()
            ));
        }
    }
}

fn engine_fire_battle() -> (u32, Battle, Arc<ArenaWire>) {
    use crate::domain::vehicle::Aim;
    use wot_shared_kernel::{ArmorDevice, Device, DeviceHealth, EngineCompartment};

    let (id, mut battle, wire) = projectile_battle(&[1, 2, 2]);
    battle.parameters.fire_ticks = 3;
    battle.parameters.fire_damage_percent = 12.0;
    let target = &mut battle.players.get_mut(&VICTIM).unwrap().tank;
    target.parameters = battle.parameters;
    target.position = Vec3::new(8.0, 0.0, 0.65);
    let profile = target.performance.as_mut().unwrap();
    let hull = Arc::get_mut(&mut profile.armor[1].as_mut().unwrap().mesh).unwrap();
    for triangle in &mut hull.triangles {
        triangle.plate.damage_factor = 0.1;
    }
    hull.engine_compartment = Some(EngineCompartment {
        exhaust_z: -0.9,
        material: 21,
        device: ArmorDevice {
            device: Device::Engine,
            can_ignite_engine: true,
            health: None,
            projectile_chance: 1.0,
            explosion_chance: 0.0,
        },
    });
    profile.device_health.insert(
        Device::Engine,
        DeviceHealth {
            maximum: 200,
            repaired: 100,
            repair_seconds: 10.0,
            fire_chance: 1.0,
        },
    );
    battle
        .players
        .get_mut(&NEARBY_VICTIM)
        .unwrap()
        .tank
        .position = Vec3::new(18.0, 0.0, 10.0);
    let shooter = &mut battle.players.get_mut(&SHOOTER).unwrap().tank;
    shooter
        .performance
        .as_mut()
        .unwrap()
        .gun
        .shell_ballistics
        .insert(
            10,
            ShellBallistics {
                kind: ShellKind::ArmorPiercing,
                caliber_mm: 90.0,
                penetration_mm: [100.0; 2],
                far_damage: 100.0,
                device_damage: [50.0; 2],
                speed: 100.0,
                penetration_randomization: 0.0,
                damage_randomization: 0.0,
                uniform_penetration: true,
                uniform_damage: true,
                damage_limited_range: f32::INFINITY,
                explosive: None,
                gravity: 0.0,
                normalization: 0.0,
                ricochet_cos: 0.0,
                trace_ricochet: false,
                obstacle_damage: None,
                obstacle_penetration_loss: 0.0,
            },
        );
    assert!(shooter.aim_at(Aim::HullAngles {
        yaw: std::f32::consts::FRAC_PI_2,
        pitch: 0.0
    }));
    shooter.advance_gun(2.0);

    (id, battle, wire)
}

#[test]
#[ignore = "Requires WOT_RESOURCE_DIR with the extracted EU client bundle"]
fn engine_hit_replicates_fire_to_driver_attacker_and_observer() {
    use wot_shared_kernel::Device;

    let (id, mut battle, wire) = engine_fire_battle();
    let mut delivery = ImpactDelivery::default();
    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    let subscriber = tracing_subscriber::registry()
        .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("fire-test")));
    let (fire_type, fire_component) = wire.dispatch.entity_from_name("Fire").unwrap();
    let nrl_type = wire
        .dispatch
        .component_nrl_id(fire_type)
        .unwrap()
        .to_le_bytes();
    let remote_root = 4100u16.to_be_bytes();
    let fire_node = 4101u16.to_be_bytes();
    let info_node = 4102u16.to_be_bytes();
    tracing::subscriber::with_default(subscriber, || {
        let command = tracing::info_span!("test.fire_command");
        let now = battle.started_at;
        {
            let _entered = command.enter();
            assert_eq!(
                battle.fire(&mut delivery, &wire, id, SHOOTER, now).unwrap(),
                "launched"
            );
        }
        let impact = now + Duration::from_millis(100);
        battle.advance_projectiles(&mut delivery, &wire, id, impact);
        let target = &battle.players[&VICTIM];
        assert_eq!(target.tank.health, 90);
        assert!(!target.tank.condition.disabled(Device::Engine));
        assert!(target.tank.condition.burning());
        assert_ne!(delivery.shot_results[0].1.hitFlags & 4, 0);

        let mut created: Vec<_> = delivery
            .track_updates
            .iter()
            .filter(|(_, kind, _)| *kind == NrlCreateNode::ID)
            .collect();
        created.sort_by_key(|record| record.0.0);
        assert_eq!(created.len(), 3);
        for (peer, _, bytes) in created {
            let bytes = if *peer == VICTIM {
                bytes.as_slice()
            } else {
                assert_eq!(bytes[..2], remote_root);
                assert_eq!(&bytes[2..10], &[0, 1, 1, 22, 0, 0, 0, 0]);
                &bytes[10..]
            };
            assert_eq!(bytes[..2], fire_node);
            assert_eq!(
                &bytes[2..4],
                if *peer == VICTIM {
                    &[4, 0]
                } else {
                    &remote_root
                }
            );
            assert_eq!(bytes[4], 2);
            assert_eq!(&bytes[5..7], &nrl_type);
            assert_eq!(&bytes[8..], &[1, 1, 0x82, 3, 0]);
        }
        let details: Vec<_> = delivery
            .track_updates
            .iter()
            .filter(|(_, kind, bytes)| *kind == NrlData::ID && bytes.starts_with(&info_node))
            .collect();
        let [(VICTIM, _, bytes)] = details.as_slice() else {
            panic!("Expected fireInfo for the driver only");
        };
        let mut body = &bytes[4..];
        let ScriptValue::Dict(info) =
            ScriptValue::read(&mut body, &fire_component.properties[0].ty).unwrap()
        else {
            panic!("Expected a Fire.fireInfo dictionary");
        };
        assert!(body.is_empty());
        assert_eq!(info["attackerID"], ScriptValue::Int32(12));
        assert!(matches!(info["deviceExtraIndex"], ScriptValue::UInt8(value) if value > 0));
        assert_eq!(info["equipmentID"], ScriptValue::UInt16(0));
        assert_eq!(info["notificationIndex"], ScriptValue::UInt8(0));
        assert_eq!(info["startTime"], ScriptValue::Float64(0.1));

        battle.advance_damage(
            &mut delivery,
            &wire,
            id,
            impact + Duration::from_millis(499),
        );
        assert!(delivery.fire_damage.is_empty());
        for (tick, expected) in [(1, 78), (2, 70), (3, 66)] {
            battle.advance_damage(
                &mut delivery,
                &wire,
                id,
                impact + Duration::from_millis(500 * tick),
            );
            assert_eq!(battle.players[&VICTIM].tank.health, expected);
            let stopped = delivery
                .track_updates
                .iter()
                .filter(|(_, kind, _)| *kind == ApplyNrlUnlink::ID)
                .count();
            assert_eq!(stopped, if tick == 3 { 3 } else { 0 });
        }
        assert!(!battle.players[&VICTIM].tank.condition.burning());
        assert!(battle.players[&VICTIM].active_fire.is_none());
        assert_eq!(delivery.fire_damage.len(), 9);
        for (broadcast, (previous, health)) in
            delivery
                .fire_damage
                .as_chunks::<3>()
                .0
                .iter()
                .zip([(90, 78), (78, 70), (70, 66)])
        {
            for (_, args) in broadcast {
                assert_eq!(
                    args,
                    &[
                        ScriptValue::Int16(health),
                        ScriptValue::Int16(previous),
                        ScriptValue::Int32(12),
                        ScriptValue::UInt8(1),
                        ScriptValue::Int8(-1),
                    ]
                );
            }
        }
        assert_eq!(delivery.fire_logs.len(), 6);
        for (peer, event) in &delivery.fire_logs {
            if *peer == VICTIM {
                assert_eq!(event.eventType, 10);
                assert_eq!(event.targetID.0, 12);
            } else {
                assert_eq!(*peer, SHOOTER);
                assert_eq!(event.eventType, 7);
                assert_eq!(event.targetID.0, 22);
            }
        }
        assert_eq!(
            delivery
                .track_updates
                .iter()
                .filter(|(_, kind, bytes)| *kind == NrlData::ID && bytes.as_slice() == fire_node)
                .count(),
            3
        );
        delivery.track_updates.clear();
        delivery.fire_damage.clear();
        battle.players.get_mut(&VICTIM).unwrap().tank.health = 16;
        let again = now + Duration::from_secs(2);
        command.in_scope(|| {
            assert_eq!(
                battle
                    .fire(&mut delivery, &wire, id, SHOOTER, again)
                    .unwrap(),
                "launched"
            );
        });
        let again = again + Duration::from_millis(100);
        battle.advance_projectiles(&mut delivery, &wire, id, again);
        let components: Vec<_> = delivery
            .track_updates
            .iter()
            .filter(|(_, id, _)| *id == NrlCreateNode::ID)
            .collect();
        assert_eq!(components.len(), 3);
        assert!(
            components
                .iter()
                .all(|(_, _, bytes)| bytes.starts_with(&fire_node))
        );
        assert_eq!(battle.players[&VICTIM].tank.health, 6);
        assert!(battle.players[&VICTIM].active_fire.is_some());
        battle.advance_damage(&mut delivery, &wire, id, again + Duration::from_millis(500));
        assert_eq!(battle.players[&VICTIM].tank.health, 0);
        assert!(battle.players[&VICTIM].active_fire.is_none());
        assert_eq!(delivery.fire_damage.len(), 3);
        assert_eq!(
            delivery
                .track_updates
                .iter()
                .filter(|(_, kind, _)| *kind == ApplyNrlUnlink::ID)
                .count(),
            3
        );
        battle.advance_damage(&mut delivery, &wire, id, again + Duration::from_secs(1));
        assert_eq!(delivery.fire_damage.len(), 3);
        battle.players.get_mut(&SHOOTER).unwrap().pending_reload = None;
    });
    provider.force_flush().unwrap();
    let spans = exporter.get_finished_spans().unwrap();
    let command = spans
        .iter()
        .find(|span| span.name == "test.fire_command")
        .unwrap();
    let shots: Vec<_> = spans
        .iter()
        .filter(|span| span.name == "battles.shot")
        .collect();
    assert_eq!(shots.len(), 2);
    for shot in &shots {
        assert_eq!(shot.parent_span_id, command.span_context.span_id());
        assert!(
            shot.attributes
                .iter()
                .any(|field| field.key.as_str() == "output.fire.origin"
                    && field.value.as_str() == "engine")
        );
    }
    let ticks: Vec<_> = spans
        .iter()
        .filter(|span| span.name == "battles.vehicle_damage_tick")
        .collect();
    assert_eq!(ticks.len(), 4);
    for tick in ticks {
        assert!(
            shots
                .iter()
                .any(|shot| tick.parent_span_id == shot.span_context.span_id())
        );
        assert!(
            tick.attributes
                .iter()
                .any(|field| field.key.as_str() == "output.recipients"
                    && field.value == opentelemetry::Value::I64(3))
        );
    }
}

#[test]
#[ignore = "Requires WOT_RESOURCE_DIR with the extracted EU client bundle"]
fn engine_and_fuel_damage_update_only_the_driver_and_show_repairs() {
    use wot_shared_kernel::{ArmorDevice, Device, DeviceHealth};
    let (id, mut battle, wire) = engine_fire_battle();
    let extras = wire.engine_and_fuel_extras();
    let target = &mut battle.players.get_mut(&VICTIM).unwrap().tank;
    target.parameters.fire_ticks = 0;
    let profile = target.performance.as_mut().unwrap();
    let hull = Arc::get_mut(&mut profile.armor[1].as_mut().unwrap().mesh).unwrap();
    hull.engine_compartment = None;
    let entry = hull.triangles.clone();
    for (index, device) in [Device::Engine, Device::Fuel].into_iter().enumerate() {
        hull.triangles.extend(entry.iter().map(|&triangle| {
            let mut internal = triangle;
            internal
                .vertices
                .iter_mut()
                .for_each(|point| point.x += 0.2);
            internal.material = [21, 29][index];
            internal.plate.damage_factor = 0.0;
            internal.plate.thickness_mm = 0.0;
            internal.plate.device = Some(ArmorDevice {
                device,
                can_ignite_engine: false,
                health: None,
                projectile_chance: 1.0,
                explosion_chance: 0.0,
            });
            internal
        }));
        profile.device_health.insert(
            device,
            DeviceHealth {
                maximum: 100,
                repaired: 60,
                repair_seconds: (index + 1) as f32,
                fire_chance: 0.0,
            },
        );
    }
    let mut delivery = ImpactDelivery::default();
    let damage_updates =
        |delivery: &ImpactDelivery| owner_device_updates(delivery, &wire, "vehicleDamageInfoList");
    let repair_updates = |delivery: &ImpactDelivery| {
        owner_device_updates(delivery, &wire, "destroyedDevicesIsRepairing")
    };
    let state = |delivery: &ImpactDelivery, expected: &[(u8, u8)]| {
        let rows = damage_updates(delivery);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].len(), expected.len());
        for (row, &(extra, code)) in rows[0].iter().zip(expected) {
            assert_eq!(row["extraIndex"], ScriptValue::UInt8(extra));
            assert_eq!(row["damageIndex"], ScriptValue::UInt8(code));
        }
    };
    let timers = |delivery: &ImpactDelivery, expected: &[u8]| {
        let rows = repair_updates(delivery);
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0]
                .iter()
                .map(|row| row["extraIndex"].clone())
                .collect::<Vec<_>>(),
            expected
                .iter()
                .copied()
                .map(ScriptValue::UInt8)
                .collect::<Vec<_>>()
        );
    };
    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    let subscriber = tracing_subscriber::registry()
        .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("module-hud-test")));
    tracing::subscriber::with_default(subscriber, || {
        let mut impact = battle.started_at;
        for code in [3, 4] {
            delivery.track_updates.clear();
            assert_eq!(
                battle
                    .fire(&mut delivery, &wire, id, SHOOTER, impact)
                    .unwrap(),
                "launched"
            );
            impact += Duration::from_millis(100);
            battle.advance_projectiles(&mut delivery, &wire, id, impact);
            state(&delivery, &[(extras[0], code), (extras[1], code)]);
            timers(&delivery, if code == 3 { &[] } else { &extras });
            if code == 3 {
                impact += Duration::from_secs(1);
            }
        }
        assert!(battle.players[&VICTIM].tank.condition.immobilized());
        for (index, remaining) in [extras[1..].as_ref(), &[]].into_iter().enumerate() {
            delivery.track_updates.clear();
            impact += Duration::from_secs(1);
            battle.advance_damage(&mut delivery, &wire, id, impact);
            state(&delivery, &[(extras[index], 15)]);
            timers(&delivery, remaining);
            assert!(!battle.players[&VICTIM].tank.condition.immobilized());
        }
        battle.players.get_mut(&SHOOTER).unwrap().pending_reload = None;
    });
    let spans = exporter.get_finished_spans().unwrap();
    let has = |span: &opentelemetry_sdk::trace::SpanData, key, value| {
        span.attributes
            .iter()
            .any(|field| field.key.as_str() == key && field.value.to_string() == value)
    };
    let shot = spans
        .iter()
        .find(|span| {
            span.name == "battles.shot"
                && has(span, "output.engine.state", "broken")
                && has(span, "output.fuel.state", "broken")
        })
        .unwrap();
    for device in ["engine", "fuel_tank"] {
        let repair = spans
            .iter()
            .find(|span| span.name == "battles.repair_device" && has(span, "input.device", device))
            .unwrap();
        assert_eq!(repair.parent_span_id, shot.span_context.span_id());
        assert!(has(repair, "output.outcome", "repaired_critical"));
        assert!(has(repair, "output.recipients", "1"));
    }
}

#[test]
#[ignore = "Requires WOT_RESOURCE_DIR with the extracted EU client bundle"]
fn spotting_delivers_vehicle_transitions_and_preserves_the_movement_trace() {
    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    let tracer = provider.tracer("spotting-test");
    let subscriber =
        tracing_subscriber::registry().with(tracing_opentelemetry::layer().with_tracer(tracer));
    tracing::subscriber::with_default(subscriber, || {
        let (id, mut battle, wire) = projectile_battle(&[1, 2]);
        for player in battle.players.values_mut() {
            player.visible_vehicles = HashSet::from([player.vehicle.entity_id]);
        }
        let enemy = battle.players[&VICTIM].vehicle.entity_id;
        let observer = battle.players[&SHOOTER].vehicle.entity_id;
        let (_, own_vehicle) = wire.dispatch.entity_from_name("OwnVehicle").unwrap();
        let method = own_vehicle
            .client_methods
            .iter()
            .position(|method| method.name.as_ref() == "onObservedByEnemy")
            .unwrap();
        let node = 1026 + u16::try_from(own_vehicle.properties.len() + method).unwrap();
        let [high, low] = node.to_be_bytes();
        let alert = [high, low, 0, 0];
        let is_alert = |(_, kind, body): &(ParticipantId, u8, Vec<u8>)| {
            *kind == NrlData::ID && body.as_slice() == alert
        };
        let start = battle.started_at;
        let mut delivery = ImpactDelivery::default();
        let drive = tracing::info_span!("test.spotting_drive");
        battle.advance_spotting(
            &mut delivery,
            &wire,
            id,
            start,
            &[(observer, drive.clone())],
        );
        assert!(delivery.track_updates.iter().any(|(peer, kind, body)| {
            *peer == SHOOTER
                && *kind == CreateArenaEntity::ID
                && body.get(1..5) == Some(enemy.to_le_bytes().as_slice())
        }));
        assert!(!delivery.track_updates.iter().any(is_alert));
        battle.advance_spotting(
            &mut delivery,
            &wire,
            id,
            start + Duration::from_secs(3),
            &[],
        );
        assert_eq!(
            delivery
                .track_updates
                .iter()
                .filter(|entry| entry.0 == VICTIM && is_alert(entry))
                .count(),
            1
        );
        let target = battle.players.get_mut(&VICTIM).unwrap();
        target.tank.position = Vec3::Z * 600.0;
        target.vehicle.position = target.tank.position;
        battle.advance_spotting(
            &mut delivery,
            &wire,
            id,
            start + Duration::from_secs(4),
            &[],
        );
        assert!(delivery.track_updates.iter().any(|(peer, kind, body)| {
            *peer == SHOOTER && *kind == LeaveArenaEntity::ID && body == &enemy.to_le_bytes()
        }));
        assert!(!battle.players[&SHOOTER].visible_vehicles.contains(&enemy));
        assert_eq!(
            delivery
                .track_updates
                .iter()
                .filter(|entry| entry.0 == VICTIM && is_alert(entry))
                .count(),
            1
        );
    });
    let spans = exporter.get_finished_spans().unwrap();
    let drive = spans
        .iter()
        .find(|span| span.name == "test.spotting_drive")
        .unwrap();
    let visibility = spans
        .iter()
        .find(|span| {
            span.name == "battles.spotting" && span.parent_span_id == drive.span_context.span_id()
        })
        .unwrap();
    assert!(
        visibility
            .attributes
            .iter()
            .any(|field| field.key.as_str() == "output.vehicles.entered"
                && field.value.to_string() == "2")
    );
    assert!(spans.iter().any(|span| span.name == "test.track_delivery"
        && span.parent_span_id == visibility.span_context.span_id()));
    assert!(spans.iter().any(|span| {
        span.name == "battles.sixth_sense"
            && span.parent_span_id == visibility.span_context.span_id()
            && span.attributes.iter().any(|field| {
                field.key.as_str() == "output.outcome" && field.value.to_string() == "enqueued"
            })
    }));
}
