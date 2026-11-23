use super::{
    avatar::{BattleEvent, ShotResult},
    *,
};
use crate::domain::{
    armor::{ArmorHit, ArmorOutcome},
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
const ARMOR_BLOCKED_WITH_DAMAGE: u32 = 1 << 25;
const DIRECT_PROJECTILE: u32 = 1 << 20;
const RICOCHET_PROJECTILE: u32 = 1 << 23;
const MAX_PENDING_IMPACTS: usize = 256;
const MAX_OBSTACLES_PIERCED: u8 = 8;

pub(super) struct PendingImpact {
    due: Instant,
    distance: f32,
    contact: Contact,
    shot: Shot,
    shot_id: i32,
    shooter: ParticipantId,
    attacker: u32,
    team: u8,
    obstacles_pierced: u8,
    span: tracing::Span,
}

enum Contact {
    Scene(Option<obstacles::ObstacleHit>),
    Vehicle {
        participant: ParticipantId,
        id: u32,
        hit: ArmorHit,
    },
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
}

impl Battle {
    #[tracing::instrument(name = "battles.shot", skip_all, fields(
        ddd.bounded_context.name = "battles", input.battle.id = battle_id,
        input.participant.id = client.0, input.entity_id = tracing::field::Empty,
        input.ammunition.shell = tracing::field::Empty, input.shell.damage = tracing::field::Empty,
        input.shell.kind = tracing::field::Empty, input.shell.caliber_mm = tracing::field::Empty,
        input.shell.max_distance_metres = tracing::field::Empty,
        output.projectile.flight_seconds = tracing::field::Empty,
        output.damage_stickers.count = tracing::field::Empty,
        output.scene_impact.distance_metres = tracing::field::Empty,
        output.obstacle.material_kind = tracing::field::Empty,
        output.obstacle.fragment_destroyed = tracing::field::Empty,
        output.shot.id = tracing::field::Empty,
        output.obstacles_pierced.count = tracing::field::Empty,
        output.projectile.ricocheted = tracing::field::Empty,
        output.target.entity_id = tracing::field::Empty, output.damage = tracing::field::Empty,
        output.hit_direction.radians = tracing::field::Empty,
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
        shooter.pending_reload = shooter
            .tank
            .reload_timers()
            .iter()
            .any(Option::is_some)
            .then(|| span.clone());
        let entity_id = shooter.vehicle.entity_id;
        let team = shooter.team;
        self.shot_sequence = self.shot_sequence.wrapping_add(1);
        let shot_id = self.shot_sequence;
        span.record("output.shot.id", shot_id);
        let contact = self.projectile_contact(&shot, team, None);
        let outcome = if contact.is_some() {
            "launched"
        } else {
            "miss"
        };
        span.record("output.outcome", outcome);
        for (&peer, participant) in &self.players {
            if !participant.entered {
                continue;
            }
            let result = (|| -> Result<()> {
                wire.show_tracer(delivery, peer, entity_id, shot_id, &shot)?;
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
        if let Some((distance, contact)) = contact {
            let seconds = distance / shot.ballistics.speed;
            span.record("output.projectile.flight_seconds", seconds);
            let due = now + Duration::from_secs_f32(seconds);
            self.queue_impact(PendingImpact {
                due,
                distance,
                contact,
                shot,
                shot_id,
                shooter: client,
                attacker: entity_id,
                team,
                obstacles_pierced: 0,
                span,
            });
        }
        Ok(outcome)
    }

    fn projectile_contact(
        &self,
        shot: &Shot,
        team: u8,
        ignore: Option<ParticipantId>,
    ) -> Option<(f32, Contact)> {
        let span = tracing::Span::current();
        let ground = self
            .ground
            .shot_intersection(shot.origin, shot.direction, shot.max_distance);
        let obstacle = self.obstacles.intersect(
            shot.origin,
            shot.direction,
            ground.unwrap_or(shot.max_distance),
        );
        let scene = obstacle
            .map(|hit| (hit.distance, Some(hit)))
            .or_else(|| ground.map(|distance| (distance, None)));
        let range = scene.map_or(shot.max_distance, |(distance, _)| distance);
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
                Some((
                    participant,
                    p.vehicle.entity_id,
                    shot.armor_path(&p.tank, range)?,
                ))
            })
            .min_by(|(_, a, left), (_, b, right)| {
                left.first_distance()
                    .total_cmp(&right.first_distance())
                    .then(a.cmp(b))
            });
        let vehicle = target.and_then(|(participant, id, path)| {
            let hit = shot.resolve_armor(id, path)?;
            Some((
                hit.distance,
                Contact::Vehicle {
                    participant,
                    id,
                    hit,
                },
            ))
        });
        vehicle.or_else(|| {
            scene.map(|(distance, obstacle)| {
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
                        && let Some(mut onward) = shot.continue_from(distance, shot.direction)
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
                hit,
            } => {
                if let Some(victim) = self
                    .players
                    .get_mut(&participant)
                    .filter(|p| p.tank.health > 0)
                {
                    if let Some(onward) = shot.ricochet_from(hit) {
                        continuation = Some((onward, Some(participant)));
                        span.record("output.projectile.ricocheted", true);
                    }
                    let (previous, health) = victim.tank.hit(hit.damage);
                    let damage = previous - health;
                    let effect = ShotImpact::new(
                        attacker,
                        damage,
                        victim.tank.max_health,
                        hit.segment,
                        hit.outcome,
                        shot.effects,
                        shot.ballistics.speed,
                    );
                    let sticker_added = victim.damage_stickers.add(effect);
                    if sticker_added {
                        span.record("output.damage_stickers.count", victim.damage_stickers.len());
                    }
                    let direction = shot.direction.x.atan2(shot.direction.z);
                    span.record("output.target.entity_id", id);
                    span.record("output.damage", damage);
                    span.record("output.hit_direction.radians", direction);
                    if let Some(shell_type) = shot.effects.damage_log_type {
                        span.record("output.damage_log.shell_type", shell_type);
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
                delivery.send_avatar(
                    peer,
                    AvatarUpdate::StopTracer {
                        id: shot_id,
                        endpoint: shot.origin + shot.direction * distance,
                    },
                )?;
                let Some(VehicleHit {
                    victim,
                    entity_id: id,
                    previous_health: previous,
                    health,
                    direction,
                    effect,
                    sticker_added,
                    outcome,
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
                    delivery.send_avatar(
                        peer,
                        AvatarUpdate::ShotResults(ShotResult {
                            vehicleID: id,
                            hitFlags: flags,
                            gunInstallationIndex: GUN_INSTALLATION_INDEX,
                        }),
                    )?;
                }
                wire.show_shot_impact(delivery, peer, id, effect)?;
                if sticker_added && let Some(target) = self.players.get(&victim) {
                    wire.publish_damage_stickers(delivery, peer, id, &target.damage_stickers)?;
                }
                if damage == 0 {
                    return Ok(());
                }
                if peer == victim {
                    delivery.send_avatar(
                        peer,
                        AvatarUpdate::HitDirection {
                            yaw: direction,
                            attacker,
                            damage: damage as u16,
                            explosive: shot.effects.kind == HIGH_EXPLOSIVE_SHELL_KIND,
                            victim: id,
                        },
                    )?;
                }
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
                if peer == victim {
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
        if let Some((shot, bounced_from)) = continuation {
            let shot_id = if bounced_from.is_some() {
                self.shot_sequence = self.shot_sequence.wrapping_add(1);
                let id = self.shot_sequence;
                for (&peer, _) in self.players.iter().filter(|(_, player)| player.entered) {
                    if let Err(error) = wire.show_tracer(delivery, peer, attacker, id, &shot) {
                        span.record("otel.status_code", "ERROR");
                        error!(%error, "Ricochet tracer delivery failed");
                        delivery.disconnect(peer);
                    }
                }
                id
            } else {
                shot_id
            };
            if let Some((distance, contact)) = self.projectile_contact(&shot, team, bounced_from) {
                let flight = distance / shot.ballistics.speed;
                span.record(
                    "output.projectile.flight_seconds",
                    shot.travelled / shot.ballistics.speed + flight,
                );
                self.queue_impact(PendingImpact {
                    due: impact.due + Duration::from_secs_f32(flight),
                    distance,
                    contact,
                    shot,
                    shot_id,
                    shooter,
                    attacker,
                    team,
                    obstacles_pierced,
                    span: span.clone(),
                });
            }
        }
        if defeated {
            self.result = Some((team as i8, ELIMINATION));
            self.round.finish(now);
            self.publish_period(delivery, battle_id, now);
        }
    }
}
