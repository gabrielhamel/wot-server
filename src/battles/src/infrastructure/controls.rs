use super::*;
use crate::{
    application::{VehicleCommand, VehicleControl},
    domain::vehicle::{Aim, AmmunitionCommand},
};

const AUTOROTATION_ENABLED: u8 = 2;

impl BattleSimulation {
    pub(super) fn vehicle_control_with_delivery(
        &mut self,
        delivery: &mut impl ArenaDelivery,
        event: VehicleControl,
    ) {
        let (Some(wire), Some(id)) = (&self.wire, self.engagements.get(&event.participant)) else {
            return;
        };
        let Some(battle) = self.active.get_mut(id) else {
            return;
        };
        let request = tracing::Span::current();
        let now = Instant::now();
        battle.advance_projectiles(delivery, wire, *id, now);
        battle.control(delivery, wire, *id, event, now, &request);
    }
}

impl Battle {
    #[tracing::instrument(name = "battles.control", skip_all, fields(
        ddd.bounded_context.name = "battles", input.battle.id = battle_id,
        input.participant.id = event.participant.0, input.entity_id = event.entity_id, rpc.method = %event.method, input.args.count = event.argument_count,
        input.drive.flags = tracing::field::Empty, input.ammunition.setting = tracing::field::Empty,
        input.ammunition.shell = tracing::field::Empty, input.aim.kind = tracing::field::Empty,
        input.vehicle.setting = tracing::field::Empty,
        input.aim.server_marker = tracing::field::Empty, output.outcome = tracing::field::Empty,
        otel.status_code = tracing::field::Empty
    ))]
    fn control(
        &mut self,
        delivery: &mut impl ArenaDelivery,
        wire: &ArenaWire,
        battle_id: u32,
        event: VehicleControl,
        now: Instant,
        request: &tracing::Span,
    ) {
        let operation = tracing::Span::current();
        let Some(player) = self.players.get_mut(&event.participant) else {
            return;
        };
        let avatar_target = event.entity_id == player.avatar_id;
        let vehicle_target = event.entity_id == player.vehicle.entity_id;
        if let VehicleCommand::ChangeSetting { setting, .. } = event.command {
            operation.record("input.vehicle.setting", setting);
        }
        let outcome = if let VehicleCommand::SetServerMarker(enabled) = event.command
            && avatar_target
        {
            operation.record("input.aim.server_marker", enabled);
            if enabled && (player.observer || player.tank.base_dispersion_angle().is_none()) {
                "vehicle_unavailable"
            } else {
                player.server_marker = enabled;
                player.last_gun_marker = None;
                player.pending_marker = enabled.then(|| operation.clone());
                "accepted"
            }
        } else if !player.ready || player.observer || player.tank.health == 0 {
            "vehicle_unavailable"
        } else {
            match event.command {
                VehicleCommand::Drive(flags) if avatar_target => {
                    operation.record("input.drive.flags", flags);
                    if !matches!(self.round.phase, RoundPhase::Battle) {
                        "round_inactive"
                    } else if player.tank.drive(flags) {
                        player.pending_drive = Some(operation.clone());
                        "accepted"
                    } else {
                        "invalid_flags"
                    }
                }
                VehicleCommand::AimRelative(offset) if avatar_target || vehicle_target => {
                    player.aim(Aim::WorldPoint(player.tank.position + offset), &operation)
                }
                VehicleCommand::AimWorld(point) if avatar_target => {
                    player.aim(Aim::WorldPoint(point), &operation)
                }
                VehicleCommand::AimAngles { yaw, pitch } if avatar_target || vehicle_target => {
                    player.aim(Aim::HullAngles { yaw, pitch }, &operation)
                }
                VehicleCommand::Shoot if avatar_target => {
                    if self.round.phase == RoundPhase::Battle && now < self.round.deadline {
                        match self.fire(delivery, wire, battle_id, event.participant, now) {
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
                VehicleCommand::ChangeSetting {
                    setting: AUTOROTATION_ENABLED,
                    value,
                } if avatar_target && !matches!(value, 0 | 1) => "invalid_setting_value",
                VehicleCommand::ChangeSetting {
                    setting: AUTOROTATION_ENABLED,
                    ..
                } if avatar_target
                    && player
                        .tank
                        .performance
                        .as_ref()
                        .is_some_and(|vehicle| vehicle.gun.yaw_limits.is_none()) =>
                {
                    "autorotation_not_applicable"
                }
                VehicleCommand::ChangeSetting {
                    setting,
                    value: shell,
                } if avatar_target && matches!(setting, 0 | 1 | 17 | 18) => {
                    operation.record("input.ammunition.setting", setting);
                    if matches!(setting, 0 | 1) {
                        operation.record("input.ammunition.shell", shell);
                    }
                    let command = match (&self.round.phase, setting) {
                        (RoundPhase::Battle, 0) => Some(AmmunitionCommand::ChangeShell(shell)),
                        (RoundPhase::Battle, 1) => Some(AmmunitionCommand::QueueShell(shell)),
                        (RoundPhase::Battle, 17 | 18) => Some(AmmunitionCommand::ReloadMagazine),
                        (RoundPhase::Loading | RoundPhase::Countdown, 0 | 1) => {
                            Some(AmmunitionCommand::SelectInitial(shell))
                        }
                        _ => None,
                    };
                    if let Some(command) = command {
                        let time = now.saturating_duration_since(self.started_at).as_secs_f32();
                        player.advance_ammunition(delivery, wire, event.participant, now, time);
                        let outcome = player.tank.ammunition_command(now, command);
                        if matches!(outcome, "shell_change_started" | "reload_started") {
                            player.pending_reload = Some(operation.clone());
                        }
                        if let Err(error) = wire.publish_own_state(
                            delivery,
                            event.participant,
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
        request.record("output.outcome", outcome);
        if matches!(
            outcome,
            "unimplemented"
                | "delivery_failed"
                | "shell_damage_unavailable"
                | "shell_range_unavailable"
                | "shell_ballistics_unavailable"
        ) {
            operation.record("otel.status_code", "ERROR");
            request.record("otel.status_code", "ERROR");
        }
    }

    pub(super) fn advance_vehicles(
        &mut self,
        delivery: &mut impl ArenaDelivery,
        wire: &ArenaWire,
        now: Instant,
    ) -> Vec<(u32, tracing::Span)> {
        let mut viewers = HashMap::<u32, Vec<ParticipantId>>::new();
        for (&peer, player) in self.players.iter().filter(|(_, player)| player.entered) {
            for &entity in &player.visible_vehicles {
                viewers.entry(entity).or_default().push(peer);
            }
        }
        let mut movement = Vec::new();
        let time = now.saturating_duration_since(self.started_at).as_secs_f32();
        let seconds = TICK.as_secs_f32();
        let motions = self.advance_world(seconds);
        for motion in motions {
            let client = motion.client;
            if !motion.broken.is_empty() {
                let mut publish =
                    || self.publish_breakages(delivery, wire, &motion.broken, motion.cause);
                match &motion.parent {
                    Some(parent) => parent.in_scope(publish),
                    None => publish(),
                };
            }
            let Some(player) = self.players.get_mut(&client) else {
                continue;
            };
            if player.entry.is_none() {
                continue;
            }
            let movement_parent = motion.parent.as_ref();
            if motion.changed
                && let Some(parent) = movement_parent
            {
                movement.push((player.vehicle.entity_id, parent.clone()));
            }
            let drive = motion.drive;
            let previous_direction = motion.previous_direction;
            let previous_tracks = motion.previous_tracks;

            let tracks = player.tank.track_speeds();
            if drive.is_some()
                || previous_direction != player.vehicle.direction
                || previous_tracks != tracks
            {
                deliver_state(
                    movement_parent.cloned(),
                    "hull_aim",
                    player.avatar_id,
                    &[client],
                    |client| {
                        wire.publish_hull_aim(
                            delivery,
                            client,
                            player.vehicle.direction,
                            tracks,
                            player.tank.rpm(),
                        )
                    },
                );
            }
            let peers = viewers
                .get(&player.vehicle.entity_id)
                .map_or(&[][..], Vec::as_slice);
            player.publish_chassis_motion(delivery, wire, peers, movement_parent);
            if player.tank.health <= 0 || self.round.phase == RoundPhase::Finished {
                continue;
            }
            let aiming = player.tank.advance_gun(seconds);
            let gun = player.tank.packed_gun_angles();
            let entity_id = player.vehicle.entity_id;
            let aim = if aiming {
                player.pending_aim.clone()
            } else {
                player.pending_aim.take()
            };
            player.advance_ammunition(delivery, wire, client, now, time);
            let parent = aim.or(drive);
            if player.last_gun_angles != gun {
                player.last_gun_angles = gun;
                deliver_state(parent.clone(), "gun_angles", entity_id, peers, |client| {
                    wire.publish_gun_angles(delivery, client, entity_id, gun)
                });
            }
            if player.server_marker
                && let Some(dispersion) = player.tank.base_dispersion_angle()
            {
                let ray = player.tank.gun_ray();
                if player.last_gun_marker != Some(ray) {
                    let parent = player.pending_marker.clone().or(parent);
                    deliver_state(parent, "gun_marker", entity_id, &[client], |client| {
                        wire.publish_gun_marker(delivery, client, ray, dispersion)?;
                        player.last_gun_marker = Some(ray);
                        player.pending_marker = None;
                        Ok(())
                    });
                }
            }
        }
        movement
    }
}

impl Participant {
    pub(super) fn advance_ammunition(
        &mut self,
        delivery: &mut impl ArenaDelivery,
        wire: &ArenaWire,
        client: ParticipantId,
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
                wire.publish_own_state(delivery, client, entity, &self.tank, now, time)
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

impl Participant {
    fn publish_chassis_motion(
        &mut self,
        delivery: &mut impl ArenaDelivery,
        wire: &ArenaWire,
        peers: &[ParticipantId],
        drive: Option<&tracing::Span>,
    ) {
        let entity_id = self.vehicle.entity_id;
        let engine_mode = self.tank.engine_mode();
        if engine_mode != self.last_engine_mode {
            deliver_state(drive.cloned(), "engine_mode", entity_id, peers, |client| {
                wire.publish_engine_mode(delivery, client, entity_id, engine_mode)
            });
            self.last_engine_mode = engine_mode;
        }

        let mut scroll = Vec::new();
        let mut steering = Vec::new();
        for (speed, angle) in self.tank.wheel_motion() {
            scroll.push(wire::pack_wheel_speed(speed));
            if let Some(angle) = angle {
                steering.push(wire::pack_wheel_steering(angle));
            }
        }
        for (index, (kind, values)) in [("wheels_scroll", scroll), ("steering_angles", steering)]
            .into_iter()
            .enumerate()
        {
            if values != self.last_wheels[index] {
                deliver_state(drive.cloned(), kind, entity_id, peers, |client| {
                    wire.publish_wheels(delivery, client, entity_id, index == 1, &values)
                });
                self.last_wheels[index] = values;
            }
        }
    }
}

fn deliver_state(
    parent: Option<tracing::Span>,
    name: &'static str,
    entity_id: u32,
    peers: &[ParticipantId],
    mut send: impl FnMut(ParticipantId) -> Result<()>,
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
