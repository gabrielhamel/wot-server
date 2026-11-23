use super::*;
use wot_platform::mercury::script::Value as WireValue;

#[derive(Clone)]
pub(super) struct DamageSource {
    pub attacker: u32,
    pub span: tracing::Span,
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
        for (_, participant) in self
            .players
            .iter_mut()
            .filter(|(_, participant)| participant.tank.health > 0)
        {
            let Some(source) = participant.pending_condition.clone() else {
                continue;
            };
            let (repairs, burn) = participant.tank.condition.tick(now);
            if !participant.tank.condition.active() {
                participant.pending_condition = None;
            }
            if repairs > 0 || burn {
                let (previous, health) = if burn {
                    let damage = (participant.tank.max_health as u16 / 100).max(1);
                    let result = participant.tank.hit(damage);
                    if result.1 <= 0 {
                        participant.pending_condition = None;
                    }
                    result
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
