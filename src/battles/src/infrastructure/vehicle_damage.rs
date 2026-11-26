use super::*;
use crate::domain::devices::TrackEvent;
use wot_platform::mercury::script::Value as WireValue;

#[derive(Clone)]
pub(super) struct DamageSource {
    pub attacker: u32,
    pub span: tracing::Span,
}

impl Participant {
    pub(super) fn apply_damage(&mut self, damage: u16) -> (i16, i16) {
        let result = self.tank.hit(damage);
        if result.1 == 0 {
            self.pending_condition = None;
            self.track_repair_parents = [None, None];
        }
        result
    }
}

impl Battle {
    pub(super) fn advance_damage(
        &mut self,
        delivery: &mut impl ArenaDelivery,
        wire: &ArenaWire,
        battle: u32,
        now: Instant,
    ) {
        let mut updates = Vec::new();
        let mut repaired_tracks = Vec::new();
        for (&peer, participant) in self.players.iter_mut().filter(|(_, participant)| {
            participant.tank.health > 0 && participant.tank.condition.active()
        }) {
            let (repairs, burn, tracks) = participant.tank.condition.tick(now, &self.parameters);
            let repairs = repairs - tracks.iter().flatten().count() as u8;
            for (side, event) in tracks.into_iter().enumerate() {
                if let Some(event) = event
                    && let Some(parent) = participant.track_repair_parents[side].take()
                {
                    repaired_tracks.push((peer, side, event, parent));
                }
            }
            let Some(source) = participant.pending_condition.clone() else {
                continue;
            };
            if !participant.tank.condition.active() {
                participant.pending_condition = None;
            }
            if repairs > 0 || burn {
                let (previous, health) = if burn {
                    let damage = (f32::from(participant.tank.max_health)
                        * self.parameters.fire_damage_percent
                        / 100.0) as u16;
                    participant.apply_damage(damage.max(1))
                } else {
                    (participant.tank.health, participant.tank.health)
                };
                updates.push((
                    participant.vehicle.entity_id,
                    source,
                    repairs,
                    previous,
                    health,
                ));
            }
        }
        repaired_tracks.sort_unstable_by_key(|(peer, side, ..)| (peer.0, *side));
        for (peer, side, event, parent) in repaired_tracks {
            let Some(player) = self
                .players
                .get(&peer)
                .filter(|player| player.tank.health > 0)
            else {
                continue;
            };
            let id = player.vehicle.entity_id;
            let span = tracing::info_span!(parent: &parent, "battles.repair_track",
                ddd.bounded_context.name = "battles", input.entity_id = id,
                input.track.side = ["left", "right"][side], output.outcome = event.name(),
                output.recipients = tracing::field::Empty, otel.status_code = tracing::field::Empty);
            span.in_scope(|| {
                let mut recipients = 0;
                for (&recipient, _) in self.players.iter().filter(|(_, player)| player.entered) {
                    let result = (|| -> Result<()> {
                        wire.publish_broken_tracks(delivery, recipient, id, &player.tank)?;
                        if recipient == peer {
                            let mut events: [Option<TrackEvent>; 2] = [None; 2];
                            events[side] = Some(event);
                            let time = now.saturating_duration_since(self.started_at).as_secs_f32();
                            wire.publish_track_damage(
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
                            error!(%error, "Track repair delivery failed");
                            delivery.disconnect(recipient);
                        }
                    }
                }
                span.record("output.recipients", recipients);
            });
        }
        updates.sort_unstable_by_key(|(entity, ..)| *entity);
        for (id, source, repairs, previous, health) in updates {
            let span = tracing::info_span!(parent: &source.span, "battles.vehicle_damage_tick",
                "ddd.bounded_context.name" = "battles", "input.entity_id" = id,
                "output.devices.repaired" = repairs, "output.damage" = previous - health,
                "otel.status_code" = tracing::field::Empty);
            span.in_scope(|| {
                if health < previous {
                    for (&peer, _) in self.players.iter().filter(|(_, player)| player.entered) {
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
                                    WireValue::UInt8(1),
                                    WireValue::Int8(-1),
                                ],
                            )?;
                            Ok(())
                        })();
                        if let Err(error) = result {
                            span.record("otel.status_code", "ERROR");
                            error!(%error, "Fire damage delivery failed");
                            delivery.disconnect(peer);
                        }
                    }
                }
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
