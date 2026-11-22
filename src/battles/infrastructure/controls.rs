use super::avatar::{BattleEvent, ShotResult};
use super::*;
use crate::{
    battles::domain::vehicle::{Aim, AmmunitionCommand, SHOT_RANGE},
    platform::mercury::script::Value as WireValue,
};
use glam::Vec3Swizzles;

const ELIMINATION: u8 = 1;
const HIGH_EXPLOSIVE_SHELL_KIND: u8 = 1;
const GUN_INSTALLATION_INDEX: u8 = 0;
const VEHICLE_KILLED: u32 = 1;
const ARMOR_PIERCED_WITH_DAMAGE: u32 = 1 << 4;
const DIRECT_PROJECTILE: u32 = 1 << 20;

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
        input.ammunition.shell = tracing::field::Empty, input.aim.kind = tracing::field::Empty,
        input.aim.server_marker = tracing::field::Empty, output.outcome = tracing::field::Empty,
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
        let outcome = if let ("setServerMarker", [WireValue::UInt8(enabled @ (0 | 1))]) =
            (&*event.name, event.args.as_slice())
            && avatar_target
        {
            let enabled = *enabled != 0;
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
        let seconds = TICK.as_secs_f32();
        for (&client, player) in self
            .players
            .iter_mut()
            .filter(|(_, player)| player.ready && !player.observer)
        {
            let previous = player.tank.position;
            let previous_direction = player.vehicle.direction;
            let previous_tracks = player.tank.track_speeds();
            let drive = player.pending_drive.take();
            if player.tank.health <= 0 {
                player.tank.stop_motion();
            } else if drive.is_some() || player.tank.is_driving() {
                let half_width = player
                    .tank
                    .performance
                    .as_ref()
                    .map_or(1.25, |profile| profile.mobility.track_half_width);
                let ground = self.ground.ground_contact(
                    previous.xz(),
                    player.tank.yaw,
                    half_width,
                    previous_direction.y,
                );
                let mut step = || player.tank.step(seconds, self.map.bounds, ground);
                let moved = match &drive {
                    Some(parent) => parent.in_scope(step),
                    None => step(),
                };
                if moved {
                    if let Some((position, direction)) = self
                        .ground
                        .vehicle_pose(player.tank.position.xz(), player.tank.yaw)
                    {
                        player.tank.position = position;
                        player.tank.set_hull_tilt(direction);
                        player.vehicle.position = position;
                        player.vehicle.direction = direction;
                        if let Some(parent) = &drive {
                            movement.push((player.vehicle.entity_id, parent.clone()));
                        }
                    } else {
                        player.tank.position = previous;
                        player.tank.yaw = previous_direction.z;
                        player.tank.stop_motion();
                    }
                }
            }

            let tracks = player.tank.track_speeds();
            if drive.is_some()
                || previous_direction != player.vehicle.direction
                || previous_tracks != tracks
            {
                deliver_state(
                    drive.clone(),
                    "hull_aim",
                    player.avatar_id,
                    &[client],
                    |client| {
                        wire.publish_hull_aim(
                            base,
                            client,
                            player.vehicle.direction,
                            tracks,
                            player.tank.rpm(),
                        )
                    },
                );
            }
            player.publish_chassis_motion(base, wire, &peers, drive.as_ref());
            if player.tank.health <= 0 {
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
            player.advance_ammunition(base, wire, client, now, time);
            let parent = aim.or(drive);
            if player.last_gun_angles != gun {
                player.last_gun_angles = gun;
                deliver_state(parent.clone(), "gun_angles", entity_id, &peers, |client| {
                    wire.publish_gun_angles(base, client, entity_id, gun)
                });
            }
            if player.server_marker
                && let Some(dispersion) = player.tank.base_dispersion_angle()
            {
                let ray = player.tank.gun_ray();
                if player.last_gun_marker != Some(ray) {
                    let parent = player.pending_marker.clone().or(parent);
                    deliver_state(parent, "gun_marker", entity_id, &[client], |client| {
                        wire.publish_gun_marker(base, client, ray, dispersion)?;
                        player.last_gun_marker = Some(ray);
                        player.pending_marker = None;
                        Ok(())
                    });
                }
            }
        }
        movement
    }

    #[tracing::instrument(name = "battles.shot", skip_all, fields(
        ddd.bounded_context.name = "battles", input.battle.id = battle_id,
        input.client.address = %client, input.entity_id = tracing::field::Empty,
        input.ammunition.shell = tracing::field::Empty, input.shell.damage = tracing::field::Empty,
        input.shell.kind = tracing::field::Empty, input.shell.caliber_mm = tracing::field::Empty,
        output.scene_impact.distance_metres = tracing::field::Empty,
        output.obstacle.material_kind = tracing::field::Empty,
        output.shot.id = tracing::field::Empty,
        output.target.entity_id = tracing::field::Empty, output.damage = tracing::field::Empty,
        output.hit_direction.radians = tracing::field::Empty,
        output.damage_log.shell_type = tracing::field::Empty,
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
        span.record("input.shell.kind", shot.effects.kind);
        span.record("input.shell.caliber_mm", shot.effects.caliber_mm);
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
        let range = target.map_or(SHOT_RANGE, |(_, _, distance)| distance);
        let ground = self
            .ground
            .shot_intersection(shot.origin, shot.direction, range);
        let obstacle =
            self.obstacles
                .intersect(shot.origin, shot.direction, ground.unwrap_or(range));
        let scene = obstacle
            .map(|hit| (hit.distance, Some(hit.material)))
            .or_else(|| ground.map(|distance| (distance, None)));
        let target = target.filter(|_| scene.is_none());
        if let Some((distance, material)) = scene {
            span.record("output.scene_impact.distance_metres", distance);
            if let Some(material) = material {
                span.record("output.obstacle.material_kind", material);
            }
        }
        let hit_direction = shot.direction.x.atan2(shot.direction.z);
        let impact = target.and_then(|(target, id, distance)| {
            self.players.get_mut(&target).map(|victim| {
                let (prev, health) = victim.tank.hit(&shot);
                let damage = prev - health;
                let local_direction =
                    glam::Quat::from_rotation_y(-victim.tank.yaw) * shot.direction;
                let effect = ShotImpact::new(
                    entity_id,
                    damage,
                    victim.tank.max_health,
                    local_direction,
                    shot.effects,
                );
                span.record("output.target.entity_id", id);
                span.record("output.damage", damage);
                span.record("output.hit_direction.radians", hit_direction);
                if let Some(shell_type) = shot.effects.damage_log_type {
                    span.record("output.damage_log.shell_type", shell_type);
                }
                (target, id, distance, prev, health, effect)
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
        let outcome = if impact.is_some() {
            "hit"
        } else if obstacle.is_some() {
            "obstacle_impact"
        } else if ground.is_some() {
            "ground_impact"
        } else {
            "miss"
        };
        span.record("output.outcome", outcome);
        for (&peer, participant) in &self.players {
            let Some(avatar) = participant.avatar else {
                continue;
            };
            let result = (|| -> Result<()> {
                wire.show_tracer(
                    base,
                    avatar,
                    entity_id,
                    shot_id,
                    &shot,
                    impact.map(|(_, _, dist, ..)| dist),
                )?;
                base.arena_vehicle_method(
                    peer,
                    entity_id,
                    "showShooting",
                    vec![
                        WireValue::UInt8(1),
                        WireValue::Int8(0),
                        WireValue::UInt8(shot.effects.kind),
                    ],
                )?;
                if let Some((distance, material)) = scene {
                    wire.show_scene_impact(base, avatar, shot_id, &shot, distance, material)?;
                }
                if let Some((victim, id, distance, previous, health, effect)) = impact {
                    let damage = previous - health;
                    let endpoint = shot.origin + shot.direction * distance;
                    base.entity(avatar)?.stopTracer(shot_id, endpoint)?;
                    if peer == client {
                        let mut flags = DIRECT_PROJECTILE;
                        if damage > 0 {
                            flags |= ARMOR_PIERCED_WITH_DAMAGE;
                        }
                        if health == 0 {
                            flags |= VEHICLE_KILLED;
                        }
                        base.entity(avatar)?.showShotResults(vec![ShotResult {
                            vehicleID: id,
                            hitFlags: flags,
                            gunInstallationIndex: GUN_INSTALLATION_INDEX,
                        }])?;
                    }
                    wire.show_shot_impact(base, peer, id, effect)?;
                    if peer == victim {
                        base.entity(avatar)?.showOwnVehicleHitDirection(
                            hit_direction,
                            EntityId(entity_id),
                            damage as u16,
                            0,
                            false,
                            shot.effects.kind == HIGH_EXPLOSIVE_SHELL_KIND,
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
                    if damage > 0 && (peer == client || peer == victim) {
                        let received = peer == victim;
                        let other_vehicle = if received { entity_id } else { id };
                        base.entity(avatar)?
                            .onBattleEvents(vec![BattleEvent::shot_damage(
                                other_vehicle,
                                damage as u16,
                                received,
                                shot.effects,
                            )])?;
                    }
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

impl Participant {
    fn publish_chassis_motion(
        &mut self,
        base: &mut base::App,
        wire: &ArenaWire,
        peers: &[ClientKey],
        drive: Option<&tracing::Span>,
    ) {
        let entity_id = self.vehicle.entity_id;
        let engine_mode = self.tank.engine_mode();
        if engine_mode != self.last_engine_mode {
            deliver_state(drive.cloned(), "engine_mode", entity_id, peers, |client| {
                wire.publish_engine_mode(base, client, entity_id, engine_mode)
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
                    wire.publish_wheels(base, client, entity_id, index == 1, &values)
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
