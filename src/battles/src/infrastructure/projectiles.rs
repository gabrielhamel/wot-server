use super::{
    avatar::{BattleEvent, ShotResult},
    explosions::{DirectBlastHit, Explosion},
    vehicle_damage::ActiveFire,
    *,
};
use crate::domain::{
    armor::{ArmorHit, ArmorOutcome},
    devices::{DeviceDamage, DeviceEvent},
    tracer::{self, TracerView},
    vehicle::Shot,
};
use wot_platform::mercury::script::Value as WireValue;
use wot_shared_kernel::ShellKind;

const HIGH_EXPLOSIVE_SHELL_KIND: u8 = 1;
const GUN_INSTALLATION_INDEX: u8 = 0;
const VEHICLE_KILLED: u32 = 1;
const RICOCHET: u32 = 1 << 3;
const ARMOR_PIERCED_WITH_DAMAGE: u32 = 1 << 4;
const ARMOR_BLOCKED: u32 = 1 << 5;
const SCREEN_PIERCED: u32 = 1 << 6;
const SCREEN_BLOCKED: u32 = 1 << 7;
const DEVICE_PIERCED_BY_PROJECTILE: u32 = 1 << 8;
const ENGINE_DEVICE_INDEX: u32 = 0;
const FUEL_DEVICE_INDEX: u32 = 2;
const FIRE_STARTED: u32 = 1 << 2;
const TRACK_CRITICAL: u32 = 1 << 4;
const TRACK_DESTROYED: u32 = 1 << (12 + 4);
const TRACK_DAMAGED_BY_PROJECTILE: u32 = 1 << 11;
const TRACK_DAMAGED_BY_EXPLOSION: u32 = 1 << 17;
const DEVICE_DAMAGED_BY_PROJECTILE: u32 = 1 << 10;
const ARMOR_BLOCKED_WITH_DAMAGE: u32 = 1 << 25;
const DIRECT_PROJECTILE: u32 = 1 << 20;
const RICOCHET_PROJECTILE: u32 = 1 << 23;
const MAX_PENDING_IMPACTS: usize = 256;
const MAX_OBSTACLES_PIERCED: u8 = 8;
const MAX_SCENE_SCREENS: u8 = 8;

pub(super) struct PendingImpact {
    due: Instant,
    distance: f32,
    flight_seconds: f32,
    contact: Contact,
    shot: Shot,
    shot_id: i32,
    shooter: ParticipantId,
    attacker: u32,
    team: u8,
    obstacles_pierced: u8,
    tracer_peers: Vec<ParticipantId>,
    screens_pierced: u8,
    span: tracing::Span,
}

enum Contact {
    Scene(Option<obstacles::ObstacleHit>),
    Vehicle {
        participant: ParticipantId,
        id: u32,
        hit: Box<ArmorHit>,
    },
}

struct PlannedContact {
    shot: Shot,
    distance: f32,
    flight: f32,
    contact: Contact,
}

struct VehicleHit {
    victim: ParticipantId,
    entity_id: u32,
    previous_health: i16,
    health: i16,
    direction: f32,
    effect: ShotImpact,
    sticker_added: bool,
    outcome: ArmorOutcome,
    criticals: u8,
    critical_flags: u32,
    device_events: [Option<DeviceEvent>; 4],
    track_hits: u8,
    fire_started: bool,
    fire_stopped: bool,
    detonated: bool,
}

fn device_critical_flag(index: u32, damage: Option<DeviceDamage>) -> u32 {
    match damage {
        Some(DeviceDamage::Critical) => 1 << index,
        Some(DeviceDamage::Destroyed) => 1 << (12 + index),
        Some(DeviceDamage::Hit) | None => 0,
    }
}

impl Battle {
    #[tracing::instrument(name = "battles.shot", skip_all, fields(
        ddd.bounded_context.name = "battles", input.battle.id = battle_id,
        input.participant.id = client.0, input.entity_id = tracing::field::Empty,
        input.battle.unlimited_ammo = self.parameters.unlimited_ammo,
        input.battle.unlimited_life = self.parameters.unlimited_life,
        input.battle.combat_rng = self.parameters.combat_rng,
        input.ammunition.shell = tracing::field::Empty, input.shell.damage = tracing::field::Empty,
        input.shell.kind = tracing::field::Empty, input.shell.caliber_mm = tracing::field::Empty,
        input.shell.max_distance_metres = tracing::field::Empty,
        input.shell.velocity_mps = tracing::field::Empty,
        input.shell.gravity_mps2 = tracing::field::Empty,
        output.tracer.recipients = tracing::field::Empty,
        output.projectile.flight_seconds = tracing::field::Empty,
        output.damage_stickers.count = tracing::field::Empty,
        output.scene_impact.distance_metres = tracing::field::Empty,
        output.obstacle.material_kind = tracing::field::Empty,
        output.obstacle.fragment_destroyed = tracing::field::Empty,
        output.shot.id = tracing::field::Empty,
        output.obstacles_pierced.count = tracing::field::Empty,
        output.projectile.ricocheted = tracing::field::Empty,
        output.projectile.ricochet_range_metres = tracing::field::Empty,
        output.projectile.screens_pierced = tracing::field::Empty,
        output.devices.damaged = tracing::field::Empty,
        output.devices.destroyed = tracing::field::Empty,
        output.tracks.hit_count = tracing::field::Empty,
        output.tracks.left = tracing::field::Empty,
        output.tracks.right = tracing::field::Empty,
        output.engine.state = tracing::field::Empty,
        output.fuel.state = tracing::field::Empty,
        output.crew.injured = tracing::field::Empty,
        output.fire.started = tracing::field::Empty,
        output.fire.origin = tracing::field::Empty,
        output.fire.ended = tracing::field::Empty,
        output.fire.recipients = tracing::field::Empty,
        output.ammo_rack.detonated = tracing::field::Empty,
        output.target.entity_id = tracing::field::Empty, output.damage = tracing::field::Empty,
        output.hit_direction.radians = tracing::field::Empty,
        output.hit_direction.blocked = tracing::field::Empty,
        output.hit_direction.critical_flags = tracing::field::Empty,
        output.hit_direction.damage = tracing::field::Empty,
        output.damage_log.shell_type = tracing::field::Empty,
        output.outcome = tracing::field::Empty, otel.status_code = tracing::field::Empty
    ), err)]
    pub(super) fn fire(
        &mut self,
        delivery: &mut impl ArenaDelivery,
        wire: &ArenaWire,
        battle_id: u32,
        client: ParticipantId,
        now: Instant,
    ) -> Result<&'static str> {
        let span = tracing::Span::current();
        if self.projectiles.len() >= MAX_PENDING_IMPACTS {
            span.record("output.outcome", "projectile_queue_full");
            return Ok("projectile_queue_full");
        }
        let Some(shooter) = self.players.get_mut(&client) else {
            return Ok("vehicle_unavailable");
        };
        let time = now.saturating_duration_since(self.started_at).as_secs_f32();
        shooter.advance_ammunition(delivery, wire, client, now, time);
        span.record("input.entity_id", shooter.vehicle.entity_id);
        let shell = shooter.tank.shell_settings()[0];
        span.record("input.ammunition.shell", shell);
        let shot = match shooter.tank.fire(now) {
            Ok(shot) => shot,
            Err(reason) => {
                span.record("output.outcome", reason);
                if matches!(
                    reason,
                    "shell_damage_unavailable"
                        | "shell_range_unavailable"
                        | "shell_ballistics_unavailable"
                ) {
                    span.record("otel.status_code", "ERROR");
                    error!(shell, reason, "Loaded shell has an invalid shot definition");
                }
                return Ok(reason);
            }
        };
        span.record("input.shell.damage", shot.damage);
        span.record("input.shell.kind", shot.effects.kind);
        span.record("input.shell.caliber_mm", shot.effects.caliber_mm);
        span.record("input.shell.max_distance_metres", shot.max_distance);
        span.record("input.shell.velocity_mps", shot.ballistics.speed);
        span.record("input.shell.gravity_mps2", shot.ballistics.gravity);
        shooter.pending_reload = shooter
            .tank
            .reload_timers()
            .iter()
            .any(Option::is_some)
            .then(|| span.clone());
        shooter.spotting_shot = shooter
            .tank
            .performance
            .as_ref()
            .and_then(|profile| profile.spotting)
            .map(|_| span.clone());
        let entity_id = shooter.vehicle.entity_id;
        let team = shooter.team;
        self.shot_sequence = self.shot_sequence.wrapping_add(1);
        let shot_id = self.shot_sequence;
        span.record("output.shot.id", shot_id);
        let contact = self.plan_contact(shot, team, None);
        let outcome = if contact.is_some() {
            "launched"
        } else {
            "miss"
        };
        span.record("output.outcome", outcome);
        let mut tracer_peers = Vec::new();
        for (&peer, participant) in &self.players {
            if !participant.entered {
                continue;
            }
            let result = (|| -> Result<()> {
                if participant.sees_tracer(&shot, entity_id) {
                    wire.show_tracer(delivery, peer, entity_id, shot_id, &shot)?;
                    tracer_peers.push(peer);
                }
                if participant.visible_vehicles.contains(&entity_id) {
                    delivery.arena_vehicle_method(
                        peer,
                        entity_id,
                        "showShooting",
                        vec![
                            WireValue::UInt8(1),
                            WireValue::Int8(0),
                            WireValue::UInt8(shot.effects.kind),
                        ],
                    )?;
                }
                if peer == client {
                    wire.publish_own_state(
                        delivery,
                        peer,
                        participant.vehicle.entity_id,
                        &participant.tank,
                        now,
                        time,
                    )?;
                }
                Ok(())
            })();
            if let Err(error) = result {
                span.record("otel.status_code", "ERROR");
                error!(%error, "Shot delivery failed");
                delivery.disconnect(peer);
            }
        }
        span.record("output.tracer.recipients", tracer_peers.len());
        if let Some(PlannedContact {
            shot,
            distance,
            flight,
            contact,
        }) = contact
        {
            span.record("output.projectile.flight_seconds", flight);
            let due = now + Duration::from_secs_f32(flight);
            self.queue_impact(PendingImpact {
                due,
                distance,
                flight_seconds: flight,
                contact,
                shot,
                shot_id,
                shooter: client,
                attacker: entity_id,
                team,
                obstacles_pierced: 0,
                screens_pierced: 0,
                tracer_peers,
                span,
            });
        }
        Ok(outcome)
    }

    fn plan_contact(
        &self,
        shot: Shot,
        team: u8,
        ignore: Option<ParticipantId>,
    ) -> Option<PlannedContact> {
        shot.flight_segments(&self.parameters).find_map(|segment| {
            let (distance, contact) =
                self.projectile_contact(&segment.shot, segment.distance, team, ignore)?;
            let partial = segment.seconds * (distance / segment.distance);
            let mut shot = segment.shot;
            shot.ballistics.speed = segment.contact_speed(partial);
            Some(PlannedContact {
                shot,
                distance,
                flight: segment.started + partial,
                contact,
            })
        })
    }

    fn projectile_contact(
        &self,
        shot: &Shot,
        limit: f32,
        team: u8,
        ignore: Option<ParticipantId>,
    ) -> Option<(f32, Contact)> {
        let span = tracing::Span::current();
        // Resolve a layered target along one local tangent when its entry lies near
        // the end of a flight segment.
        let lookahead = (limit + 12.0).min(shot.max_distance);
        let ground = self
            .ground
            .shot_intersection(shot.origin, shot.direction, lookahead);
        let obstacle =
            self.obstacles
                .intersect(shot.origin, shot.direction, ground.unwrap_or(lookahead));
        let scene = obstacle
            .map(|hit| (hit.distance, Some(hit)))
            .or_else(|| ground.map(|distance| (distance, None)));
        let range = scene.map_or(lookahead, |(distance, _)| distance);
        let target = self
            .players
            .iter()
            .filter(|&(&participant, p)| {
                p.team != team
                    && Some(participant) != ignore
                    && p.ready
                    && !p.observer
                    && p.tank.health > 0
            })
            .filter_map(|(&participant, p)| {
                let path = shot.armor_path(&p.tank, range)?;
                (path.first_distance() <= limit).then_some((participant, p.vehicle.entity_id, path))
            })
            .min_by(|(_, a, left), (_, b, right)| {
                left.first_distance()
                    .total_cmp(&right.first_distance())
                    .then(a.cmp(b))
            });
        let vehicle = target.and_then(|(participant, id, path)| {
            let hit = shot.resolve_armor(id, path, &self.parameters)?;
            Some((
                hit.distance,
                Contact::Vehicle {
                    participant,
                    id,
                    hit: Box::new(hit),
                },
            ))
        });
        vehicle.or_else(|| {
            scene
                .filter(|(distance, _)| *distance <= limit)
                .map(|(distance, obstacle)| {
                    span.record("output.scene_impact.distance_metres", distance);
                    if let Some(hit) = obstacle {
                        span.record("output.obstacle.material_kind", hit.material);
                    }
                    (distance, Contact::Scene(obstacle))
                })
        })
    }

    fn queue_impact(&mut self, impact: PendingImpact) {
        let index = self
            .projectiles
            .partition_point(|pending| pending.due <= impact.due);
        self.projectiles.insert(index, impact);
    }

    pub(super) fn advance_projectiles(
        &mut self,
        delivery: &mut impl ArenaDelivery,
        wire: &ArenaWire,
        battle_id: u32,
        now: Instant,
    ) {
        while self.round.phase == RoundPhase::Battle
            && self
                .projectiles
                .first()
                .is_some_and(|impact| impact.due <= now && impact.due < self.round.deadline)
        {
            let impact = self.projectiles.remove(0);
            impact
                .span
                .in_scope(|| self.resolve_impact(delivery, wire, battle_id, now, &impact));
        }
        if self.round.phase == RoundPhase::Finished || now >= self.round.deadline {
            for impact in self.projectiles.drain(..) {
                impact.span.record("output.outcome", "round_inactive");
            }
        }
    }

    fn resolve_impact(
        &mut self,
        delivery: &mut impl ArenaDelivery,
        wire: &ArenaWire,
        battle_id: u32,
        now: Instant,
        impact: &PendingImpact,
    ) {
        let PendingImpact {
            ref shot,
            shot_id,
            distance,
            shooter,
            attacker,
            team,
            ref span,
            ..
        } = *impact;
        let mut defeated = false;
        let mut vehicle_hit = None;
        let mut destructibles = Vec::new();
        let mut continuation = None;
        let mut obstacles_pierced = impact.obstacles_pierced;
        let mut screens_pierced = impact.screens_pierced;
        let outcome = match impact.contact {
            Contact::Scene(Some(hit)) => {
                if let Some(fragment) = hit.fragment {
                    let amount = shot
                        .ballistics
                        .obstacle_damage
                        .unwrap_or(f32::from(shot.damage)) as u32;
                    let broken = self.obstacles.damage(fragment, amount);
                    let loss = shot.ballistics.obstacle_penetration_loss;
                    if self.obstacles.remaining_health(fragment) == 0
                        && matches!(
                            shot.ballistics.kind,
                            ShellKind::HighExplosive {
                                obstacle_penetration: true,
                                ..
                            }
                        )
                        && shot.penetration_at(distance) > loss
                        && obstacles_pierced < MAX_OBSTACLES_PIERCED
                        && let Some(mut onward) =
                            shot.continue_from(distance, shot.direction, &self.parameters)
                    {
                        onward.penetration_loss += loss;
                        continuation = Some((onward, None));
                        obstacles_pierced += 1;
                        span.record("output.obstacles_pierced.count", obstacles_pierced);
                    }
                    span.record("output.obstacle.fragment_destroyed", !broken.is_empty());
                    if !broken.is_empty() {
                        let yaw = shot.direction.x.atan2(shot.direction.z);
                        destructibles = self.publish_breakages(
                            delivery,
                            wire,
                            &broken,
                            scenery::BreakageCause::Shot { yaw },
                        );
                    }
                }
                if continuation.is_some() {
                    "obstacle_penetrated"
                } else {
                    "obstacle_impact"
                }
            }
            Contact::Scene(None) => "ground_impact",
            Contact::Vehicle {
                participant,
                id,
                ref hit,
            } => {
                if let Some(victim) = self
                    .players
                    .get_mut(&participant)
                    .filter(|p| p.tank.health > 0)
                {
                    if let Some(onward) = shot.ricochet_from(**hit, &self.parameters) {
                        continuation = Some((onward, None));
                        span.record("output.projectile.ricocheted", true);
                        span.record(
                            "output.projectile.ricochet_range_metres",
                            onward.max_distance,
                        );
                    } else if screens_pierced < MAX_SCENE_SCREENS
                        && let Some(onward) = shot.through_screen(**hit, &self.parameters)
                    {
                        continuation = Some((onward, Some(participant)));
                        screens_pierced += 1;
                        span.record("output.projectile.screens_pierced", screens_pierced);
                    }
                    let critical = victim.tank.apply_criticals(&hit.devices, now);
                    span.record("output.devices.damaged", critical.damaged);
                    span.record("output.devices.destroyed", critical.destroyed);
                    span.record("output.tracks.hit_count", critical.track_hits);
                    let device_events = [
                        critical.tracks[0],
                        critical.tracks[1],
                        critical.engine.and_then(DeviceDamage::change),
                        critical.fuel.and_then(DeviceDamage::change),
                    ];
                    for ((field, parent), event) in [
                        "output.tracks.left",
                        "output.tracks.right",
                        "output.engine.state",
                        "output.fuel.state",
                    ]
                    .into_iter()
                    .zip(&mut victim.device_hud_repair_parents)
                    .zip(device_events)
                    {
                        if let Some(event) = event {
                            span.record(field, event.name());
                        }
                        if event.is_some_and(DeviceEvent::starts_repair) {
                            *parent = Some(span.clone());
                        }
                    }
                    if victim.device_repair_parent.is_none()
                        && victim.tank.condition.has_other_device_repair()
                    {
                        victim.device_repair_parent = Some(span.clone());
                    }
                    span.record("output.crew.injured", critical.crew);
                    span.record("output.ammo_rack.detonated", critical.detonated);
                    let damage = if critical.detonated {
                        u16::MAX
                    } else {
                        hit.damage
                    };
                    let (previous, health) = victim.apply_damage(damage);
                    let fire_stopped = health == 0 && victim.active_fire.take().is_some();
                    let ignition = critical.fire_started.filter(|_| health > 0);
                    if let Some(origin) = ignition {
                        victim.active_fire = Some(ActiveFire {
                            attacker,
                            origin,
                            started_at: now
                                .saturating_duration_since(self.started_at)
                                .as_secs_f64(),
                            span: span.clone(),
                        });
                        span.record("output.fire.origin", origin.name());
                    }
                    span.record("output.fire.started", ignition.is_some());
                    span.record("output.fire.ended", fire_stopped);
                    let damage = previous - health;
                    let effect = ShotImpact::new(
                        attacker,
                        damage,
                        victim.tank.max_health,
                        hit.segment,
                        hit.outcome,
                        shot.effects,
                        shot.ballistics.speed,
                    )
                    .with_track_impact(critical.track_impact);
                    let sticker_added = victim.damage_stickers.add(effect);
                    if sticker_added {
                        span.record("output.damage_stickers.count", victim.damage_stickers.len());
                    }
                    let direction = shot.direction.x.atan2(shot.direction.z);
                    span.record("output.target.entity_id", id);
                    span.record("output.damage", damage);
                    if let Some(shell_type) = shot.effects.damage_log_type {
                        span.record("output.damage_log.shell_type", shell_type);
                    }
                    let mut critical_flags =
                        device_critical_flag(ENGINE_DEVICE_INDEX, critical.engine)
                            | device_critical_flag(FUEL_DEVICE_INDEX, critical.fuel);
                    if critical
                        .tracks
                        .into_iter()
                        .flatten()
                        .any(DeviceEvent::starts_repair)
                    {
                        critical_flags |= TRACK_DESTROYED;
                    } else if critical.track_hits > 0 {
                        critical_flags |= TRACK_CRITICAL;
                    } else if critical.engine.is_none() && critical.fuel.is_none() {
                        critical_flags = u32::from(critical.damaged);
                    }
                    vehicle_hit = Some(VehicleHit {
                        victim: participant,
                        entity_id: id,
                        previous_health: previous,
                        health,
                        direction,
                        effect,
                        sticker_added,
                        outcome: hit.outcome,
                        criticals: critical.damaged,
                        critical_flags,
                        device_events,
                        track_hits: critical.track_hits,
                        fire_started: ignition.is_some(),
                        fire_stopped,
                        detonated: critical.detonated,
                    });
                    defeated = self
                        .players
                        .values()
                        .all(|p| p.team == team || p.observer || p.tank.health == 0);
                    hit.outcome.name()
                } else {
                    "target_unavailable"
                }
            }
        };
        span.record("output.outcome", outcome);
        for (&peer, participant) in self.players.iter().filter(|(_, player)| player.entered) {
            let result = (|| -> Result<()> {
                if let Contact::Scene(obstacle) = impact.contact {
                    if continuation.is_some() {
                        return Ok(());
                    }
                    return wire.show_scene_impact(
                        delivery,
                        peer,
                        shot_id,
                        shot,
                        distance,
                        (obstacle.map(|hit| hit.material), &destructibles),
                    );
                }
                if impact.tracer_peers.contains(&peer)
                    && continuation.is_none_or(|(onward, _)| onward.ricocheted != shot.ricocheted)
                {
                    delivery.send_avatar(
                        peer,
                        AvatarUpdate::StopTracer {
                            id: shot_id,
                            endpoint: shot.origin + shot.direction * distance,
                        },
                    )?;
                }
                let Some(VehicleHit {
                    victim,
                    entity_id: id,
                    previous_health: previous,
                    health,
                    direction,
                    effect,
                    sticker_added,
                    outcome,
                    criticals,
                    critical_flags,
                    device_events,
                    track_hits,
                    fire_started,
                    fire_stopped,
                    detonated,
                }) = vehicle_hit
                else {
                    return Ok(());
                };
                let damage = previous - health;
                if peer == shooter {
                    let mut flags = if shot.ricocheted {
                        RICOCHET_PROJECTILE
                    } else {
                        DIRECT_PROJECTILE
                    } | match outcome {
                        ArmorOutcome::Penetrated => ARMOR_PIERCED_WITH_DAMAGE,
                        ArmorOutcome::Blocked => ARMOR_BLOCKED,
                        ArmorOutcome::Ricochet => RICOCHET,
                        ArmorOutcome::ScreenPenetrated => SCREEN_PIERCED,
                        ArmorOutcome::ScreenBlocked => SCREEN_BLOCKED,
                        ArmorOutcome::Spall => ARMOR_BLOCKED_WITH_DAMAGE,
                    };
                    if health == 0 {
                        flags |= VEHICLE_KILLED;
                    }
                    if criticals > track_hits {
                        flags |= DEVICE_DAMAGED_BY_PROJECTILE;
                    }
                    if track_hits > 0 {
                        flags |= if outcome == ArmorOutcome::Spall {
                            TRACK_DAMAGED_BY_EXPLOSION
                        } else {
                            TRACK_DAMAGED_BY_PROJECTILE | DEVICE_PIERCED_BY_PROJECTILE
                        };
                    }
                    if fire_started {
                        flags |= FIRE_STARTED;
                    }
                    delivery.send_avatar(
                        peer,
                        AvatarUpdate::ShotResults(ShotResult {
                            vehicleID: id,
                            hitFlags: flags,
                            gunInstallationIndex: GUN_INSTALLATION_INDEX,
                        }),
                    )?;
                }
                let visible = participant.visible_vehicles.contains(&id);
                if visible {
                    wire.show_shot_impact(delivery, peer, id, effect)?;
                }
                if let Some(target) = self.players.get(&victim) {
                    if visible && device_events[..2].contains(&Some(DeviceEvent::Broken)) {
                        wire.publish_broken_tracks(delivery, peer, id, &target.tank)?;
                    }
                    if peer == victim && device_events.iter().any(Option::is_some) {
                        let time = now.saturating_duration_since(self.started_at).as_secs_f32();
                        wire.publish_device_damage(
                            delivery,
                            peer,
                            &target.tank,
                            device_events,
                            attacker,
                            (now, time),
                        )?;
                    }
                }
                if visible
                    && sticker_added
                    && let Some(target) = self.players.get(&victim)
                {
                    wire.publish_damage_stickers(delivery, peer, id, &target.damage_stickers)?;
                }
                let blocked = damage == 0
                    && track_hits == 0
                    && matches!(
                        outcome,
                        ArmorOutcome::Blocked
                            | ArmorOutcome::Ricochet
                            | ArmorOutcome::ScreenBlocked
                    );
                if peer == victim && (damage != 0 || criticals != 0 || blocked) {
                    let indicator_damage = if blocked { shot.damage } else { damage as u16 };
                    delivery.send_avatar(
                        peer,
                        AvatarUpdate::HitDirection {
                            yaw: direction,
                            attacker,
                            damage: indicator_damage,
                            criticals: critical_flags,
                            blocked,
                            explosive: shot.effects.kind == HIGH_EXPLOSIVE_SHELL_KIND,
                            victim: id,
                        },
                    )?;
                    span.record("output.hit_direction.radians", direction);
                    span.record("output.hit_direction.blocked", blocked);
                    span.record("output.hit_direction.critical_flags", critical_flags);
                    span.record("output.hit_direction.damage", indicator_damage);
                }
                if visible && detonated {
                    delivery.arena_vehicle_method(
                        peer,
                        id,
                        "showAmmoBayEffect",
                        vec![
                            WireValue::UInt8(1),
                            WireValue::Float32(1.0),
                            WireValue::Float32(0.0),
                        ],
                    )?;
                }
                if visible && damage > 0 {
                    wire.vehicle_properties(
                        delivery,
                        peer,
                        id,
                        [("health", WireValue::Int16(health))],
                    )?;
                    delivery.arena_vehicle_method(
                        peer,
                        id,
                        "onHealthChanged",
                        vec![
                            WireValue::Int16(health),
                            WireValue::Int16(previous),
                            WireValue::Int32(attacker as i32),
                            WireValue::UInt8(0),
                            WireValue::Int8(-1),
                        ],
                    )?;
                }
                if visible
                    && fire_stopped
                    && let Some(target) = self.players.get(&victim)
                {
                    wire.publish_fire_stop(delivery, peer, target.roster_row)?;
                }
                if damage > 0 && (peer == shooter || peer == victim) {
                    let received = peer == victim;
                    delivery.send_avatar(
                        peer,
                        AvatarUpdate::BattleEvents(BattleEvent::shot_damage(
                            if received { attacker } else { id },
                            damage as u16,
                            received,
                            shot.effects,
                        )),
                    )?;
                }
                if peer == victim && damage > 0 {
                    let time = now.saturating_duration_since(self.started_at).as_secs_f32();
                    wire.publish_own_state(delivery, peer, id, &participant.tank, now, time)?;
                }
                Ok(())
            })();
            if let Err(error) = result {
                span.record("otel.status_code", "ERROR");
                error!(%error, "Projectile impact delivery failed");
                delivery.disconnect(peer);
            }
        }
        if let Some(hit) = vehicle_hit.filter(|hit| hit.fire_started) {
            self.publish_fire_ignition(delivery, wire, hit.victim);
        }
        if let Some((onward, excluded)) = continuation {
            let bounced = onward.ricocheted != shot.ricocheted;
            let shot = onward;
            let mut tracer_peers = impact.tracer_peers.clone();
            let shot_id = if bounced {
                tracer_peers.clear();
                self.shot_sequence = self.shot_sequence.wrapping_add(1);
                let id = self.shot_sequence;
                for (&peer, _) in self
                    .players
                    .iter()
                    .filter(|(_, player)| player.entered && player.sees_tracer(&shot, attacker))
                {
                    if let Err(error) = wire.show_tracer(delivery, peer, attacker, id, &shot) {
                        span.record("otel.status_code", "ERROR");
                        error!(%error, "Ricochet tracer delivery failed");
                        delivery.disconnect(peer);
                    } else {
                        tracer_peers.push(peer);
                    }
                }
                id
            } else {
                shot_id
            };
            if let Some(PlannedContact {
                shot,
                distance,
                flight,
                contact,
            }) = self.plan_contact(shot, team, excluded)
            {
                span.record(
                    "output.projectile.flight_seconds",
                    impact.flight_seconds + flight,
                );
                self.queue_impact(PendingImpact {
                    due: impact.due + Duration::from_secs_f32(flight),
                    distance,
                    flight_seconds: impact.flight_seconds + flight,
                    contact,
                    shot,
                    shot_id,
                    shooter,
                    attacker,
                    team,
                    obstacles_pierced,
                    tracer_peers,
                    screens_pierced,
                    span: span.clone(),
                });
            }
        }
        if continuation.is_none() && shot.ballistics.explosive.is_some() {
            let (splash, direct_hit) = match impact.contact {
                Contact::Scene(_) => (true, None),
                Contact::Vehicle {
                    participant,
                    ref hit,
                    ..
                } => (
                    hit.outcome != ArmorOutcome::Penetrated,
                    matches!(
                        hit.outcome,
                        ArmorOutcome::Blocked | ArmorOutcome::Spall | ArmorOutcome::Penetrated
                    )
                    .then_some(DirectBlastHit {
                        victim: participant,
                        damage: hit.damage,
                        armor_mm: hit.thickness_mm,
                    }),
                ),
            };
            self.resolve_explosion(
                delivery,
                wire,
                now,
                Explosion {
                    shot,
                    distance,
                    shooter,
                    attacker,
                    team,
                    direct_hit,
                },
                splash,
            );
            defeated = self
                .players
                .values()
                .all(|player| player.team == team || player.observer || player.tank.health == 0);
        }
        if defeated {
            self.result = Some((team as i8, ELIMINATION));
            self.round.finish(now);
            self.publish_period(delivery, battle_id, now);
        }
    }
}

impl Participant {
    fn sees_tracer(&self, shot: &Shot, source: u32) -> bool {
        let view = TracerView::Direct {
            direction: self.tank.gun_ray().direction,
        };
        tracer::visible(
            shot.origin,
            self.tank.position,
            self.visible_vehicles.contains(&source),
            view,
            &self.tank.parameters,
        )
    }
}
