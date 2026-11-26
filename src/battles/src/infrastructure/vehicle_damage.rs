use super::avatar::{BattleEvent, FIRE_ATTACK_REASON};
use super::*;
use crate::domain::devices::FireOrigin;
use wot_platform::mercury::script::Value as WireValue;

#[derive(Clone)]
pub(super) struct ActiveFire {
    pub attacker: u32,
    pub origin: FireOrigin,
    pub started_at: f64,
    pub span: tracing::Span,
}

impl Participant {
    pub(super) fn apply_damage(&mut self, damage: u16) -> (i16, i16) {
        let result = self.tank.hit(damage);
        if result.1 == 0 {
            self.device_repair_parent = None;
            self.device_hud_repair_parents = Default::default();
        }
        result
    }
}

impl Battle {
    pub(super) fn publish_fire_ignition(
        &mut self,
        delivery: &mut impl ArenaDelivery,
        wire: &ArenaWire,
        victim: ParticipantId,
    ) {
        let Some(target) = self.players.get(&victim) else {
            return;
        };
        let Some(fire) = target.active_fire.clone() else {
            return;
        };
        let vehicle = target.vehicle.entity_id;
        let row = target.roster_row;
        let mut recipients = 0;
        for (&peer, witness) in self.players.iter_mut().filter(|(_, player)| player.entered) {
            let own = peer == victim;
            let missing_root = !own && !witness.fire_roots.contains(&row);
            let result = wire.publish_fire_start(
                delivery,
                peer,
                row,
                own,
                &fire,
                missing_root.then_some(vehicle),
            );
            match result {
                Ok(()) => {
                    recipients += 1;
                    if missing_root {
                        witness.fire_roots.insert(row);
                    }
                }
                Err(error) => {
                    tracing::Span::current().record("otel.status_code", "ERROR");
                    error!(%error, "Fire ignition delivery failed");
                    delivery.disconnect(peer);
                }
            }
        }
        tracing::Span::current().record("output.fire.recipients", recipients);
    }

    pub(super) fn end_round_fires(
        &mut self,
        delivery: &mut impl ArenaDelivery,
        wire: &ArenaWire,
        battle_id: u32,
    ) {
        let mut stopped = Vec::new();
        for player in self.players.values_mut() {
            if let Some(source) = player.active_fire.take() {
                player.tank.condition.extinguish();
                stopped.push((player.vehicle.entity_id, player.roster_row, source));
            }
        }
        for (vehicle, row, source) in stopped {
            let span = tracing::info_span!(parent: &source.span, "battles.stop_vehicle_fire",
                ddd.bounded_context.name = "battles", input.battle.id = battle_id,
                input.entity_id = vehicle, input.fire.attacker_id = source.attacker,
                input.fire.origin = source.origin.name(), output.outcome = "round_finished",
                output.recipients = tracing::field::Empty, otel.status_code = tracing::field::Empty);
            span.in_scope(|| {
                let mut recipients = 0;
                for (&peer, _) in self.players.iter().filter(|(_, player)| player.entered) {
                    match wire.publish_fire_stop(delivery, peer, row) {
                        Ok(()) => recipients += 1,
                        Err(error) => {
                            span.record("otel.status_code", "ERROR");
                            error!(%error, "Fire cleanup delivery failed");
                            delivery.disconnect(peer);
                        }
                    }
                }
                span.record("output.recipients", recipients);
            });
        }
    }

    pub(super) fn advance_damage(
        &mut self,
        delivery: &mut impl ArenaDelivery,
        wire: &ArenaWire,
        battle: u32,
        now: Instant,
    ) {
        let mut updates = Vec::new();
        let mut repaired_devices = Vec::new();
        for (&peer, participant) in self.players.iter_mut().filter(|(_, participant)| {
            participant.tank.health > 0 && participant.tank.condition.active()
        }) {
            let (repairs, burn, events) = participant.tank.condition.tick(now, &self.parameters);
            let device_repairs = repairs - events.iter().flatten().count() as u8;
            if device_repairs > 0
                && let Some(parent) = &participant.device_repair_parent
            {
                tracing::info_span!(parent: parent, "battles.repair_devices",
                    ddd.bounded_context.name = "battles", input.battle.id = battle,
                    input.entity_id = participant.vehicle.entity_id,
                    output.devices.repaired = device_repairs)
                .in_scope(|| {});
            }
            if !participant.tank.condition.has_other_device_repair() {
                participant.device_repair_parent = None;
            }
            for (index, event) in events.into_iter().enumerate() {
                if let Some(event) = event
                    && let Some(parent) = participant.device_hud_repair_parents[index].take()
                {
                    repaired_devices.push((peer, index, event, parent));
                }
            }
            if let (Some(ticks), Some(source)) = (burn, participant.active_fire.as_ref()) {
                let source = source.clone();
                let damage = participant.tank.fire_tick_damage(ticks);
                let (previous, health) = participant.apply_damage(damage);
                let ended = !participant.tank.condition.burning();
                if ended {
                    participant.active_fire = None;
                }
                updates.push((
                    peer,
                    participant.vehicle.entity_id,
                    participant.roster_row,
                    source,
                    ticks,
                    previous,
                    health,
                    ended,
                ));
            }
        }
        repaired_devices.sort_unstable_by_key(|(peer, index, ..)| (peer.0, *index));
        for (peer, index, event, parent) in repaired_devices {
            let Some(player) = self
                .players
                .get(&peer)
                .filter(|player| player.tank.health > 0)
            else {
                continue;
            };
            let id = player.vehicle.entity_id;
            let is_track = index < 2;
            let span = if is_track {
                tracing::info_span!(parent: &parent, "battles.repair_track",
                    ddd.bounded_context.name = "battles", input.entity_id = id,
                    input.track.side = ["left", "right"][index], output.outcome = event.name(),
                    output.recipients = tracing::field::Empty, otel.status_code = tracing::field::Empty)
            } else {
                tracing::info_span!(parent: &parent, "battles.repair_device",
                    ddd.bounded_context.name = "battles", input.entity_id = id,
                    input.device = ["engine", "fuel_tank"][index - 2], output.outcome = event.name(),
                    output.recipients = tracing::field::Empty, otel.status_code = tracing::field::Empty)
            };
            span.in_scope(|| {
                let mut recipients = 0;
                for (&recipient, _) in self.players.iter().filter(|(recipient, player)| {
                    player.entered && (is_track || **recipient == peer)
                }) {
                    let result = (|| -> Result<()> {
                        if is_track {
                            wire.publish_broken_tracks(delivery, recipient, id, &player.tank)?;
                        }
                        if recipient == peer {
                            let mut events = [None; 4];
                            events[index] = Some(event);
                            let time = now.saturating_duration_since(self.started_at).as_secs_f32();
                            wire.publish_device_damage(
                                delivery,
                                peer,
                                &player.tank,
                                events,
                                0,
                                (now, time),
                            )?;
                        }
                        Ok(())
                    })();
                    match result {
                        Ok(()) => recipients += 1,
                        Err(error) => {
                            span.record("otel.status_code", "ERROR");
                            error!(%error, "Device repair delivery failed");
                            delivery.disconnect(recipient);
                        }
                    }
                }
                span.record("output.recipients", recipients);
            });
        }
        updates.sort_unstable_by_key(|(_, entity, ..)| *entity);
        for (victim, id, row, source, ticks, previous, health, ended) in updates {
            let span = tracing::info_span!(parent: &source.span, "battles.vehicle_damage_tick",
                ddd.bounded_context.name = "battles", input.battle.id = battle,
                input.entity_id = id, input.fire.attacker_id = source.attacker,
                input.fire.origin = source.origin.name(), input.fire.ticks_remaining = ticks,
                output.damage = previous - health, output.fire.ended = ended,
                output.recipients = tracing::field::Empty, otel.status_code = tracing::field::Empty);
            span.in_scope(|| {
                let mut recipients = 0;
                for (&peer, player) in self.players.iter().filter(|(_, player)| player.entered) {
                    let result = (|| -> Result<()> {
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
                                WireValue::Int32(source.attacker as i32),
                                WireValue::UInt8(FIRE_ATTACK_REASON),
                                WireValue::Int8(-1),
                            ],
                        )?;
                        let own = peer == victim;
                        if own {
                            wire.publish_own_health(
                                delivery,
                                peer,
                                id,
                                health,
                                FIRE_ATTACK_REASON as i8,
                            )?;
                        }
                        if own || player.vehicle.entity_id == source.attacker {
                            delivery.send_avatar(
                                peer,
                                AvatarUpdate::BattleEvents(BattleEvent::fire_damage(
                                    if own { source.attacker } else { id },
                                    (previous - health) as u16,
                                    own,
                                )),
                            )?;
                        }
                        if ended {
                            wire.publish_fire_stop(delivery, peer, row)?;
                        }
                        Ok(())
                    })();
                    match result {
                        Ok(()) => recipients += 1,
                        Err(error) => {
                            span.record("otel.status_code", "ERROR");
                            error!(%error, "Fire damage delivery failed");
                            delivery.disconnect(peer);
                        }
                    }
                }
                span.record("output.recipients", recipients);
                if self.round.phase == RoundPhase::Battle && health == 0 {
                    let alive = |team| {
                        self.players.values().any(|player| {
                            player.team == team && !player.observer && player.tank.health > 0
                        })
                    };
                    let winner = match (alive(1), alive(2)) {
                        (true, true) => return,
                        (true, false) => 1,
                        (false, true) => 2,
                        (false, false) => 0,
                    };
                    self.result = Some((winner, ELIMINATION));
                    self.round.finish(now);
                    self.publish_period(delivery, battle, now);
                }
            });
        }
    }
}
