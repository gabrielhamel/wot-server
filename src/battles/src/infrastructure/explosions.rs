use super::{
    avatar::{BattleEvent, ShotResult},
    *,
};
use crate::domain::vehicle::Shot;
use wot_platform::mercury::script::Value as WireValue;

pub(super) struct Explosion<'a> {
    pub shot: &'a Shot,
    pub distance: f32,
    pub attacker: u32,
    pub shooter: ParticipantId,
    pub team: u8,
    pub direct_hit: Option<DirectBlastHit>,
}

pub(super) struct DirectBlastHit {
    pub victim: ParticipantId,
    pub damage: u16,
    pub armor_mm: f32,
}

struct BlastHit {
    victim: ParticipantId,
    entity: u32,
    previous: i16,
    health: i16,
    max_health: i16,
    stunned: Option<Instant>,
    fire_stopped: bool,
    roster_row: usize,
}

impl Battle {
    #[tracing::instrument(name = "battles.explosion", skip_all, fields(
        ddd.bounded_context.name = "battles", input.entity_id = blast.attacker,
        input.projectile.distance_metres = blast.shot.travelled + blast.distance,
        input.explosion.vehicle_splash = vehicles,
        output.explosion.trees = tracing::field::Empty,
        output.explosion.targets = tracing::field::Empty,
        output.explosion.stunned = tracing::field::Empty,
        output.explosion.fires_ended = tracing::field::Empty,
        otel.status_code = tracing::field::Empty,
    ))]
    pub(super) fn resolve_explosion(
        &mut self,
        delivery: &mut impl ArenaDelivery,
        wire: &ArenaWire,
        now: Instant,
        blast: Explosion<'_>,
        vehicles: bool,
    ) {
        let Some(profile) = blast.shot.ballistics.explosive else {
            return;
        };
        let center = blast.shot.origin + blast.shot.direction * blast.distance;
        let damage = blast.shot.damage_at(blast.distance);
        let broken = self
            .obstacles
            .blast_trees(center, profile.scenery_radius, damage);
        let span = tracing::Span::current();
        span.record("output.explosion.trees", broken.len());
        if !broken.is_empty() {
            let direction = blast.shot.direction;
            self.publish_breakages(
                delivery,
                wire,
                &broken,
                scenery::BreakageCause::Shot {
                    yaw: direction.x.atan2(direction.z),
                },
            );
        }
        let radius = profile
            .artillery_radius
            .max(profile.stun.map_or(0.0, |stun| stun.radius));
        if (!vehicles && blast.direct_hit.is_none()) || radius <= 0.0 {
            return;
        }
        let origin = center + Vec3::Y * 0.03;
        let mut hits = Vec::new();
        for (&victim, participant) in &self.players {
            let tank = &participant.tank;
            if !participant.ready
                || participant.observer
                || tank.health <= 0
                || tank.position.distance_squared(center) > (radius + 10.0).powi(2)
            {
                continue;
            }
            let direct = blast.direct_hit.as_ref().filter(|hit| hit.victim == victim);
            if !vehicles && direct.is_none() {
                continue;
            }
            let mut strongest = 0.0_f32;
            let mut closest = f32::INFINITY;
            if let Some(hit) = direct {
                closest = 0.0;
                if vehicles
                    && hit.damage == 0
                    && profile.artillery_radius > 0.0
                    && participant.team != blast.team
                {
                    strongest = (damage * profile.splash_factor
                        - hit.armor_mm * profile.armor_absorption)
                        .max(0.0);
                }
            }
            for height in [0.35, 0.9, 1.5, 2.2]
                .into_iter()
                .filter(|_| direct.is_none())
            {
                let direction = (tank.position + Vec3::Y * height - origin).normalize_or_zero();
                let probe = Shot {
                    origin,
                    direction,
                    ..*blast.shot
                };
                let Some((distance, armor)) = probe.exposed_armor(tank, radius) else {
                    continue;
                };
                let clear_range = (distance - 0.04).max(0.0);
                if self
                    .ground
                    .shot_intersection(origin, direction, clear_range)
                    .is_some()
                    || self
                        .obstacles
                        .intersect(origin, direction, clear_range)
                        .is_some()
                {
                    continue;
                }
                closest = closest.min(distance);
                if distance <= profile.artillery_radius && participant.team != blast.team {
                    let fraction = 1.0 - distance / profile.artillery_radius;
                    let falloff = profile.edge_factor + (1.0 - profile.edge_factor) * fraction;
                    strongest = strongest.max(
                        damage * profile.splash_factor * falloff - armor * profile.armor_absorption,
                    );
                }
            }
            let hp = strongest.round() as u16;
            let duration = profile
                .stun
                .filter(|stun| closest <= stun.radius)
                .map_or(0.0, |stun| {
                    let damage_fraction = (f32::from(direct.map_or(hp, |hit| hp.max(hit.damage)))
                        / damage.max(1.0))
                    .min(1.0);
                    let seconds = stun.duration
                        * (stun.guaranteed + stun.damage_coefficient * damage_fraction)
                        * (1.0 - closest / stun.radius);
                    if seconds >= stun.minimum {
                        seconds
                    } else {
                        0.0
                    }
                });
            if hp != 0 || duration > 0.0 {
                hits.push((victim, hp, duration));
            }
        }
        let mut applied = Vec::new();
        for (victim, damage, stun) in hits {
            let Some(target) = self.players.get_mut(&victim) else {
                continue;
            };
            let (previous, health) = target.apply_damage(damage);
            let fire_stopped = health == 0 && target.active_fire.take().is_some();
            let stunned = (health > 0 && stun > 0.0)
                .then(|| now + Duration::from_secs_f32(stun))
                .filter(|end| {
                    target
                        .pending_stun
                        .as_ref()
                        .is_none_or(|(current, _)| end > current)
                });
            if let Some(end) = stunned {
                target.pending_stun = Some((end, span.clone()));
                target
                    .tank
                    .set_stun_traverse(profile.stun.map(|stun| stun.turret_traverse));
            }
            if health != previous || stunned.is_some() {
                applied.push(BlastHit {
                    victim,
                    entity: target.vehicle.entity_id,
                    previous,
                    health,
                    max_health: target.tank.max_health,
                    stunned,
                    fire_stopped,
                    roster_row: target.roster_row,
                });
            }
        }
        span.record("output.explosion.targets", applied.len());
        span.record(
            "output.explosion.stunned",
            applied.iter().filter(|hit| hit.stunned.is_some()).count(),
        );
        span.record(
            "output.explosion.fires_ended",
            applied.iter().filter(|hit| hit.fire_stopped).count(),
        );
        for (&peer, player) in self.players.iter().filter(|(_, player)| player.entered) {
            for hit in &applied {
                if !player.visible_vehicles.contains(&hit.entity) && peer != blast.shooter {
                    continue;
                }
                if let Err(error) = self
                    .publish_blast_hit(delivery, wire, peer, center, &blast, hit)
                    .and_then(|()| {
                        if hit.fire_stopped {
                            wire.publish_fire_stop(delivery, peer, hit.roster_row)?;
                        }
                        Ok(())
                    })
                {
                    span.record("otel.status_code", "ERROR");
                    error!(%error, "Explosion delivery failed");
                    delivery.disconnect(peer);
                    break;
                }
            }
        }
    }

    fn publish_blast_hit(
        &self,
        delivery: &mut impl ArenaDelivery,
        wire: &ArenaWire,
        peer: ParticipantId,
        center: Vec3,
        blast: &Explosion<'_>,
        hit: &BlastHit,
    ) -> Result<()> {
        let visible = self
            .players
            .get(&peer)
            .is_some_and(|player| player.visible_vehicles.contains(&hit.entity));
        if visible && let Some(end) = hit.stunned {
            let end = end.saturating_duration_since(self.started_at).as_secs_f64();
            wire.vehicle_properties(
                delivery,
                peer,
                hit.entity,
                [("stunInfo", WireValue::Float64(end))],
            )?;
        }
        let damage = hit.previous - hit.health;
        if peer == blast.shooter {
            let flags = (1 << 21)
                | (u32::from(damage > 0) << 13)
                | (u32::from(hit.stunned.is_some()) << 22)
                | u32::from(hit.health == 0);
            delivery.send_avatar(
                peer,
                AvatarUpdate::ShotResults(ShotResult {
                    vehicleID: hit.entity,
                    hitFlags: flags,
                    gunInstallationIndex: 0,
                }),
            )?;
        }
        if damage == 0 {
            return Ok(());
        }
        if visible {
            let factor =
                (i32::from(damage) * 255 / i32::from(hit.max_health.max(1))).clamp(0, 255) as u8;
            delivery.arena_vehicle_method(
                peer,
                hit.entity,
                "showDamageFromExplosion",
                vec![
                    WireValue::Int32(blast.attacker as i32),
                    WireValue::Vector3(center),
                    WireValue::UInt8(blast.shot.effects.legacy_index),
                    WireValue::Int32(i32::from(damage)),
                    WireValue::UInt8(factor),
                    WireValue::UInt8(0),
                ],
            )?;
            wire.vehicle_properties(
                delivery,
                peer,
                hit.entity,
                [("health", WireValue::Int16(hit.health))],
            )?;
            delivery.arena_vehicle_method(
                peer,
                hit.entity,
                "onHealthChanged",
                vec![
                    WireValue::Int16(hit.health),
                    WireValue::Int16(hit.previous),
                    WireValue::Int32(blast.attacker as i32),
                    WireValue::UInt8(0),
                    WireValue::Int8(-1),
                ],
            )?;
        }
        if peer == hit.victim || peer == blast.shooter {
            let received = peer == hit.victim;
            delivery.send_avatar(
                peer,
                AvatarUpdate::BattleEvents(BattleEvent::shot_damage(
                    if received { blast.attacker } else { hit.entity },
                    damage as u16,
                    received,
                    blast.shot.effects,
                )),
            )?;
        }
        Ok(())
    }

    pub(super) fn expire_stuns(
        &mut self,
        delivery: &mut impl ArenaDelivery,
        wire: &ArenaWire,
        now: Instant,
    ) {
        let mut expired = Vec::new();
        for participant in self.players.values_mut() {
            if participant
                .pending_stun
                .as_ref()
                .is_some_and(|(end, _)| *end <= now || participant.tank.health <= 0)
                && let Some((_, span)) = participant.pending_stun.take()
            {
                participant.tank.set_stun_traverse(None);
                expired.push((participant.vehicle.entity_id, span));
            }
        }
        for (entity, span) in expired {
            span.in_scope(|| {
                for (&peer, _) in self.players.iter().filter(|(_, player)| {
                    player.entered && player.visible_vehicles.contains(&entity)
                }) {
                    if let Err(error) = wire.vehicle_properties(
                        delivery,
                        peer,
                        entity,
                        [("stunInfo", WireValue::Float64(0.0))],
                    ) {
                        span.record("otel.status_code", "ERROR");
                        error!(%error, "Stun expiry delivery failed");
                        delivery.disconnect(peer);
                    }
                }
            });
        }
    }
}
