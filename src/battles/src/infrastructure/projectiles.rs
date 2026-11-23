use super::{
    avatar::{BattleEvent, ShotResult},
    *,
};
use crate::domain::vehicle::Shot;
use wot_platform::mercury::script::Value as WireValue;

const HIGH_EXPLOSIVE_SHELL_KIND: u8 = 1;
const GUN_INSTALLATION_INDEX: u8 = 0;
const VEHICLE_KILLED: u32 = 1;
const ARMOR_PIERCED_WITH_DAMAGE: u32 = 1 << 4;
const DIRECT_PROJECTILE: u32 = 1 << 20;
const MAX_PENDING_IMPACTS: usize = 256;

pub(super) struct PendingImpact {
    due: Instant,
    distance: f32,
    contact: Contact,
    shot: Shot,
    shot_id: i32,
    shooter: ParticipantId,
    attacker: u32,
    team: u8,
    span: tracing::Span,
}

enum Contact {
    Scene(Option<u32>),
    Vehicle {
        participant: ParticipantId,
        id: u32,
        local_direction: Vec3,
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
        output.shot.id = tracing::field::Empty,
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
                    "shell_damage_unavailable" | "shell_range_unavailable"
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
        let target = self
            .players
            .iter()
            .filter(|(_, p)| p.team != team && p.ready && !p.observer && p.tank.health > 0)
            .filter_map(|(&client, p)| {
                shot.intersection(&p.tank)
                    .map(|dist| (client, p.vehicle.entity_id, dist, p.tank.yaw))
            })
            .min_by(|(_, left, a, _), (_, right, b, _)| a.total_cmp(b).then(left.cmp(right)));
        let range = target.map_or(shot.max_distance, |(_, _, distance, _)| distance);
        let ground = self
            .ground
            .shot_intersection(shot.origin, shot.direction, range);
        let obstacle =
            self.obstacles
                .intersect(shot.origin, shot.direction, ground.unwrap_or(range));
        let scene = obstacle
            .map(|hit| (hit.distance, Some(hit.material)))
            .or_else(|| ground.map(|distance| (distance, None)));
        let contact = if let Some((distance, material)) = scene {
            span.record("output.scene_impact.distance_metres", distance);
            if let Some(material) = material {
                span.record("output.obstacle.material_kind", material);
            }
            Some((distance, Contact::Scene(material)))
        } else {
            target.map(|(participant, id, distance, yaw)| {
                let local_direction = glam::Quat::from_rotation_y(-yaw) * shot.direction;
                (
                    distance,
                    Contact::Vehicle {
                        participant,
                        id,
                        local_direction,
                    },
                )
            })
        };
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
            let seconds = distance / Shot::SPEED_MPS;
            span.record("output.projectile.flight_seconds", seconds);
            let due = now + Duration::from_secs_f32(seconds);
            let index = self
                .projectiles
                .partition_point(|projectile| projectile.due <= due);
            self.projectiles.insert(
                index,
                PendingImpact {
                    due,
                    distance,
                    contact,
                    shot,
                    shot_id,
                    shooter: client,
                    attacker: entity_id,
                    team,
                    span,
                },
            );
        }
        Ok(outcome)
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
        let outcome = match impact.contact {
            Contact::Scene(Some(_)) => "obstacle_impact",
            Contact::Scene(None) => "ground_impact",
            Contact::Vehicle {
                participant,
                id,
                local_direction,
            } => {
                if let Some(victim) = self
                    .players
                    .get_mut(&participant)
                    .filter(|p| p.tank.health > 0)
                {
                    let (previous, health) = victim.tank.hit(shot);
                    let damage = previous - health;
                    let effect = ShotImpact::new(
                        attacker,
                        damage,
                        victim.tank.max_health,
                        local_direction,
                        shot.effects,
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
                    });
                    defeated = self
                        .players
                        .values()
                        .all(|p| p.team == team || p.observer || p.tank.health == 0);
                    "hit"
                } else {
                    "target_unavailable"
                }
            }
        };
        span.record("output.outcome", outcome);
        for (&peer, participant) in self.players.iter().filter(|(_, player)| player.entered) {
            let result = (|| -> Result<()> {
                if let Contact::Scene(material) = impact.contact {
                    return wire
                        .show_scene_impact(delivery, peer, shot_id, shot, distance, material);
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
                }) = vehicle_hit
                else {
                    return Ok(());
                };
                let damage = previous - health;
                if peer == shooter {
                    let mut flags = DIRECT_PROJECTILE;
                    if damage > 0 {
                        flags |= ARMOR_PIERCED_WITH_DAMAGE;
                    }
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
        if defeated {
            self.result = Some((team as i8, ELIMINATION));
            self.round.finish(now);
            self.publish_period(delivery, battle_id, now);
        }
    }
}
