use super::*;
use crate::{
    battles::domain::vehicle::{Aim, AmmunitionCommand, SHOT_RANGE, Shot},
    platform::mercury::script::Value as WireValue,
};
use glam::Vec3Swizzles;

const ELIMINATION: u8 = 1;
const SHELL_SPEED: f32 = 450.0;
const DEFAULT_SHELL_KIND: u8 = 2;

impl BattleRuntime {
    pub(crate) fn cell_method(&mut self, base: &mut base::App, event: base::CellMethodEvent) {
        let span = event.span.clone();
        span.in_scope(|| {
            let (Some(wire), Some(id)) = (&self.wire, self.engagements.get(&event.client)) else {
                return;
            };
            let Some(battle) = self.active.get_mut(id) else {
                return;
            };
            battle.control(base, wire, *id, event, Instant::now());
        });
    }
}

impl Battle {
    #[tracing::instrument(name = "battles.control", skip_all, fields(
        ddd.bounded_context.name = "battles", input.battle.id = battle_id,
        input.entity_id = event.entity_id, rpc.method = %event.name, input.args.count = event.args.len(),
        input.drive.flags = tracing::field::Empty, input.ammunition.setting = tracing::field::Empty,
        input.ammunition.shell = tracing::field::Empty, input.aim.kind = tracing::field::Empty, output.outcome = tracing::field::Empty,
        otel.status_code = tracing::field::Empty
    ))]
    fn control(
        &mut self,
        base: &mut base::App,
        wire: &ArenaWire,
        battle_id: u32,
        event: base::CellMethodEvent,
        now: Instant,
    ) {
        let operation = tracing::Span::current();
        let Some(player) = self.players.get_mut(&event.client) else {
            return;
        };
        let avatar_target = event.entity_id == player.avatar_id;
        let vehicle_target = event.entity_id == player.vehicle.entity_id;
        let outcome = if !player.ready || player.observer || player.tank.health == 0 {
            "vehicle_unavailable"
        } else {
            match (&*event.name, event.args.as_slice()) {
                ("vehicle_moveWith", [WireValue::UInt8(flags)]) if avatar_target => {
                    operation.record("input.drive.flags", flags);
                    if !matches!(self.round.phase, RoundPhase::Battle) {
                        "round_inactive"
                    } else if player.tank.drive(*flags) {
                        player.pending_drive = Some(operation.clone());
                        "accepted"
                    } else {
                        "invalid_flags"
                    }
                }
                (
                    "trackRelativePointWithGun" | "vehicle_trackRelativePointWithGun",
                    [WireValue::Vector3(offset)],
                ) if avatar_target || vehicle_target => {
                    player.aim(Aim::WorldPoint(player.tank.position + *offset), &operation)
                }
                ("vehicle_trackWorldPointWithGun", [WireValue::Vector3(point)])
                    if avatar_target =>
                {
                    player.aim(Aim::WorldPoint(*point), &operation)
                }
                (
                    "stopTrackingWithGun" | "vehicle_stopTrackingWithGun",
                    [WireValue::Float32(yaw), WireValue::Float32(pitch)],
                ) if avatar_target || vehicle_target => player.aim(
                    Aim::HullAngles {
                        yaw: *yaw,
                        pitch: *pitch,
                    },
                    &operation,
                ),
                ("vehicle_shoot", []) if avatar_target => {
                    if matches!(self.round.phase, RoundPhase::Battle) {
                        match self.fire(base, wire, battle_id, event.client, now) {
                            Ok(outcome) => outcome,
                            Err(error) => {
                                operation.record("otel.status_code", "ERROR");
                                error!(%error, "Could not publish shot");
                                "delivery_failed"
                            }
                        }
                    } else {
                        "round_inactive"
                    }
                }
                (
                    "vehicle_changeSetting",
                    [
                        WireValue::UInt8(setting @ (0 | 1 | 17 | 18)),
                        WireValue::Int32(shell),
                    ],
                ) if avatar_target => {
                    operation.record("input.ammunition.setting", setting);
                    if matches!(setting, 0 | 1) {
                        operation.record("input.ammunition.shell", shell);
                    }
                    let command = match (&self.round.phase, setting) {
                        (RoundPhase::Battle, 0) => Some(AmmunitionCommand::ChangeShell(*shell)),
                        (RoundPhase::Battle, 1) => Some(AmmunitionCommand::QueueShell(*shell)),
                        (RoundPhase::Battle, 17 | 18) => Some(AmmunitionCommand::ReloadMagazine),
                        (RoundPhase::Loading | RoundPhase::Countdown, 0 | 1) => {
                            Some(AmmunitionCommand::SelectInitial(*shell))
                        }
                        _ => None,
                    };
                    if let Some(command) = command {
                        let time = now.saturating_duration_since(self.started_at).as_secs_f32();
                        player.advance_ammunition(base, wire, event.client, now, time);
                        let outcome = player.tank.ammunition_command(now, command);
                        if matches!(outcome, "shell_change_started" | "reload_started") {
                            player.pending_reload = Some(operation.clone());
                        }
                        if let Err(error) = wire.publish_own_state(
                            base,
                            event.client,
                            player.vehicle.entity_id,
                            &player.tank,
                            now,
                            time,
                        ) {
                            operation.record("otel.status_code", "ERROR");
                            error!(%error, "Could not publish ammunition state");
                            "delivery_failed"
                        } else {
                            outcome
                        }
                    } else {
                        "round_inactive"
                    }
                }
                _ => {
                    operation.record("otel.status_code", "ERROR");
                    error!("Arena cell operation is not implemented for this target");
                    "unimplemented"
                }
            }
        };
        operation.record("output.outcome", outcome);
        event.span.record("output.outcome", outcome);
        if matches!(
            outcome,
            "unimplemented" | "delivery_failed" | "shell_damage_unavailable"
        ) {
            operation.record("otel.status_code", "ERROR");
            event.span.record("otel.status_code", "ERROR");
        }
    }

    pub(super) fn advance_vehicles(
        &mut self,
        base: &mut base::App,
        wire: &ArenaWire,
        now: Instant,
    ) -> Vec<(u32, tracing::Span)> {
        let peers: Vec<_> = self
            .players
            .iter()
            .filter(|(_, p)| p.avatar.is_some())
            .map(|(&key, _)| key)
            .collect();
        let mut movement = Vec::new();
        let time = now.saturating_duration_since(self.started_at).as_secs_f32();
        for (&client, player) in self
            .players
            .iter_mut()
            .filter(|(_, player)| player.ready && !player.observer && player.tank.health > 0)
        {
            let previous = player.tank.position;
            let drive = player.pending_drive.take();
            player.tank.step(TICK.as_secs_f32(), self.map.bounds);
            if let Some((position, direction)) = self
                .ground
                .vehicle_pose(player.tank.position.xz(), player.tank.yaw)
            {
                player.tank.position = position;
                if (position, direction) != (player.vehicle.position, player.vehicle.direction) {
                    player.vehicle.position = position;
                    player.vehicle.direction = direction;
                    if let Some(parent) = &drive {
                        movement.push((player.vehicle.entity_id, parent.clone()));
                    }
                }
            } else {
                player.tank.position = previous;
            }
            let aiming = player.tank.advance_gun(TICK.as_secs_f32());
            let gun = player.tank.packed_gun_angles();
            let entity_id = player.vehicle.entity_id;
            let aim = if aiming {
                player.pending_aim.clone()
            } else {
                player.pending_aim.take()
            };
            player.advance_ammunition(base, wire, client, now, time);
            if player.last_gun_angles != gun {
                player.last_gun_angles = gun;
                deliver_state(aim.or(drive), "gun_angles", entity_id, &peers, |client| {
                    wire.vehicle_properties(
                        base,
                        client,
                        entity_id,
                        [("gunAnglesPacked", WireValue::UInt16(gun))],
                    )
                });
            }
        }
        movement
    }

    #[tracing::instrument(name = "battles.shot", skip_all, fields(
        ddd.bounded_context.name = "battles", input.battle.id = battle_id,
        input.client.address = %client, input.entity_id = tracing::field::Empty,
        input.ammunition.shell = tracing::field::Empty, input.shell.damage = tracing::field::Empty,
        output.shot.id = tracing::field::Empty,
        output.target.entity_id = tracing::field::Empty, output.damage = tracing::field::Empty,
        output.hit_direction.radians = tracing::field::Empty,
        output.outcome = tracing::field::Empty, otel.status_code = tracing::field::Empty
    ), err)]
    fn fire(
        &mut self,
        base: &mut base::App,
        wire: &ArenaWire,
        battle_id: u32,
        client: ClientKey,
        now: Instant,
    ) -> Result<&'static str> {
        let span = tracing::Span::current();
        let Some(shooter) = self.players.get_mut(&client) else {
            return Ok("vehicle_unavailable");
        };
        let time = now.saturating_duration_since(self.started_at).as_secs_f32();
        shooter.advance_ammunition(base, wire, client, now, time);
        span.record("input.entity_id", shooter.vehicle.entity_id);
        let shell = shooter.tank.shell_settings()[0];
        span.record("input.ammunition.shell", shell);
        let shot = match shooter.tank.fire(now) {
            Ok(shot) => shot,
            Err(reason) => {
                span.record("output.outcome", reason);
                if reason == "shell_damage_unavailable" {
                    span.record("otel.status_code", "ERROR");
                    error!(shell, "Loaded shell has no valid damage definition");
                }
                return Ok(reason);
            }
        };
        span.record("input.shell.damage", shot.damage);
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
                    .map(|dist| (client, p.vehicle.entity_id, dist))
            })
            .min_by(|(_, left, a), (_, right, b)| a.total_cmp(b).then(left.cmp(right)));
        let hit_direction = (-shot.direction.x).atan2(-shot.direction.z);
        let impact = target.and_then(|(target, id, distance)| {
            self.players.get_mut(&target).map(|victim| {
                let (prev, health) = victim.tank.hit(&shot);
                span.record("output.target.entity_id", id);
                span.record("output.damage", prev - health);
                span.record("output.hit_direction.radians", hit_direction);
                (target, id, distance, prev, health)
            })
        });
        let defeated = impact.is_some()
            && self
                .players
                .values()
                .all(|p| p.team == team || p.observer || p.tank.health == 0);
        if defeated {
            self.result = Some((team as i8, ELIMINATION));
            self.round.finish(now);
        }
        let outcome = if impact.is_some() { "hit" } else { "miss" };
        span.record("output.outcome", outcome);
        for (&peer, participant) in &self.players {
            let Some(avatar) = participant.avatar else {
                continue;
            };
            let result = (|| -> Result<()> {
                base.arena_vehicle_method(
                    peer,
                    entity_id,
                    "showShooting",
                    vec![WireValue::UInt8(1), WireValue::Int8(0), WireValue::UInt8(0)],
                )?;
                self.tracer(
                    base,
                    avatar,
                    entity_id,
                    shot_id,
                    &shot,
                    impact.map(|(_, _, dist, _, _)| dist),
                )?;
                if let Some((victim, id, _, previous, health)) = impact {
                    if peer == victim {
                        base.entity(avatar)?.showOwnVehicleHitDirection(
                            hit_direction,
                            EntityId(entity_id),
                            (previous - health) as u16,
                            0,
                            false,
                            false,
                            EntityId(id),
                            0,
                        )?;
                    }

                    wire.vehicle_properties(
                        base,
                        peer,
                        id,
                        [("health", WireValue::Int16(health))],
                    )?;
                    base.arena_vehicle_method(
                        peer,
                        id,
                        "onHealthChanged",
                        vec![
                            WireValue::Int16(health),
                            WireValue::Int16(previous),
                            WireValue::Int32(entity_id as i32),
                            WireValue::UInt8(0),
                            WireValue::Int8(-1),
                        ],
                    )?;
                }
                if peer == client || impact.is_some_and(|(victim, ..)| victim == peer) {
                    wire.publish_own_state(
                        base,
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
                base.disconnect_client(peer);
            }
        }
        if defeated {
            self.publish_period(base, battle_id, now);
        }
        Ok(outcome)
    }

    fn tracer(
        &self,
        base: &mut base::App,
        avatar: EntityKey<Avatar>,
        shooter: u32,
        shot_id: i32,
        shot: &Shot,
        distance: Option<f32>,
    ) -> Result<()> {
        base.entity(avatar)?.showTracer(
            EntityId(shooter),
            shot_id,
            0,
            0,
            0,
            DEFAULT_SHELL_KIND,
            45.0,
            shot.origin,
            shot.direction * SHELL_SPEED,
            0.0,
            distance.unwrap_or(SHOT_RANGE).max(0.1),
            0,
            0,
        )?;
        Ok(())
    }
}

impl Participant {
    fn advance_ammunition(
        &mut self,
        base: &mut base::App,
        wire: &ArenaWire,
        client: ClientKey,
        now: Instant,
        time: f32,
    ) {
        if self.tank.advance_ammunition(now) {
            let reload = if self.tank.reload_timers().iter().any(Option::is_some) {
                self.pending_reload.clone()
            } else {
                self.pending_reload.take()
            };
            let entity = self.vehicle.entity_id;
            deliver_state(reload, "ammunition", entity, &[client], |client| {
                wire.publish_own_state(base, client, entity, &self.tank, now, time)
            });
        }
    }

    fn aim(&mut self, aim: Aim, span: &tracing::Span) -> &'static str {
        span.record(
            "input.aim.kind",
            match aim {
                Aim::WorldPoint(_) => "world_point",
                Aim::HullAngles { .. } => "hull_angles",
            },
        );
        if self.tank.aim_at(aim) {
            self.pending_aim = Some(span.clone());
            "accepted"
        } else {
            "invalid_aim"
        }
    }
}

fn deliver_state(
    parent: Option<tracing::Span>,
    name: &'static str,
    entity_id: u32,
    peers: &[ClientKey],
    mut send: impl FnMut(ClientKey) -> Result<()>,
) {
    let mut deliver = || {
        let span = tracing::Span::current();
        let mut delivered = 0;
        for &client in peers {
            match send(client) {
                Ok(()) => delivered += 1,
                Err(error) => {
                    span.record("otel.status_code", "ERROR");
                    error!(%error, "Vehicle state delivery failed");
                }
            }
        }
        span.record("output.delivered.count", delivered);
    };
    match parent {
        Some(parent) => {
            let span = tracing::info_span!(parent: &parent, "battles.replication",
                "ddd.bounded_context.name" = "battles", "input.entity_id" = entity_id,
                "input.replication.kind" = name, "output.recipients.count" = peers.len(),
                "output.delivered.count" = tracing::field::Empty, "otel.status_code" = tracing::field::Empty);
            span.in_scope(deliver);
        }
        None => deliver(),
    }
}
