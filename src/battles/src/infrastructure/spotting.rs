use super::*;
use crate::domain::parameters::SpottingSystem;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct MinimapContact {
    row: usize,
    position: [i16; 2],
    health: i16,
}

impl Battle {
    pub(super) fn advance_spotting(
        &mut self,
        delivery: &mut impl ArenaDelivery,
        wire: &ArenaWire,
        battle_id: u32,
        now: Instant,
        movement: &[(u32, tracing::Span)],
    ) {
        let tick = self.ticks(now);
        let everyone_visible = self.parameters.spotting_system == SpottingSystem::EveryoneVisible;
        let mut combatants = Vec::new();
        let mut causes: Vec<_> = movement.iter().map(|(_, span)| span.clone()).collect();
        for player in self.players.values_mut() {
            if player.observer
                || player.tank.health <= 0
                || (!player.entered && player.entry.is_some())
            {
                continue;
            }
            let pose = (player.vehicle.position, player.vehicle.direction);
            let moving = player
                .last_spotting_pose
                .replace(pose)
                .is_some_and(|previous| previous != pose);
            if let Some(shot) = player.spotting_shot.take() {
                causes.push(shot);
            }
            combatants.push(player.tank.spotting_state(
                player.vehicle.entity_id,
                player.team,
                moving,
                now,
            ));
        }
        let span = tracing::info_span!(parent: causes.first().unwrap_or(&tracing::Span::none()),
            "battles.spotting", ddd.bounded_context.name = "battles",
            input.battle.id = battle_id, input.arena.tick = tick, input.vehicles.count = combatants.len(),
            input.battle.spotting_system = self.parameters.spotting_system.name(),
            output.pairs.count = tracing::field::Empty, output.rays.count = tracing::field::Empty,
            output.radio_groups.count = tracing::field::Empty, output.vehicles.entered = tracing::field::Empty,
            output.vehicles.left = tracing::field::Empty, output.alerts.count = tracing::field::Empty,
            otel.status_code = tracing::field::Empty);
        for cause in causes.iter().skip(1) {
            span.follows_from(cause.id());
        }
        span.in_scope(|| {
            let changes = self.spotting.update(
                &self.parameters,
                tick,
                &combatants,
                |observer, target, origin, end| {
                    let ray = end - origin;
                    let distance = ray.length();
                    if !distance.is_finite() || distance > 500.0 {
                        return None;
                    }
                    if distance <= f32::EPSILON {
                        return Some(0.0);
                    }
                    let direction = ray / distance;
                    if self
                        .ground
                        .shot_intersection(origin, direction, distance)
                        .is_some()
                        || self.obstacles.blocks_sight(origin, direction, distance)
                    {
                        None
                    } else {
                        Some(self.obstacles.foliage_cover(
                            &self.parameters,
                            observer,
                            target,
                            origin,
                            direction,
                            distance,
                        ))
                    }
                },
            );
            span.record("output.pairs.count", changes.checked_pairs);
            span.record("output.rays.count", changes.vision_rays);
            span.record("output.radio_groups.count", changes.radio_groups);
            self.pending_spotting_alerts.retain(|entity, _| {
                combatants.iter().any(|combatant| combatant.entity == *entity)
                    && !changes.went_dark.contains(entity)
            });
            for entity in changes.newly_spotted {
                self.pending_spotting_alerts.insert(entity, span.clone());
            }
            let alerts_due: HashMap<_, _> = changes.alerts.into_iter().filter_map(|entity| {
                self.pending_spotting_alerts.remove(&entity).map(|parent| (entity, parent))
            }).collect();
            let mut entered = 0;
            let mut left = 0;
            let mut alerts = 0;
            let mut revealed = HashSet::new();
            let mut projections = Vec::new();
            for (&peer, viewer) in self.players.iter().filter(|(_, player)| player.entered) {
                let own = viewer.vehicle.entity_id;
                let visible: HashSet<_> = self
                    .players
                    .values()
                    .filter(|target| {
                        let id = target.vehicle.entity_id;
                        if id == own || target.observer {
                            return id == own;
                        }
                        if everyone_visible {
                            return target.entered || target.entry.is_none();
                        }
                        viewer.tank.position.distance_squared(target.tank.position)
                            <= self.parameters.spotting_draw_metres.powi(2)
                            && (viewer.observer
                                || (target.team == viewer.team && self.spotting.linked(own, id))
                                || (target.team != viewer.team && self.spotting.knows_enemy(own, id))
                                || (target.tank.health <= 0 && viewer.visible_vehicles.contains(&id)))
                    })
                    .map(|target| target.vehicle.entity_id)
                    .collect();
                let mut minimap = HashMap::new();
                for target in self
                    .players
                    .values()
                    // Visible entities already provide minimap positions and health.
                    .filter(|target| !everyone_visible && target.vehicle.entity_id != own && !target.observer)
                {
                    let id = target.vehicle.entity_id;
                    let allied = target.team == viewer.team;
                    let linked = self.spotting.linked(own, id);
                    if allied && !linked && !viewer.observer {
                        if let Some(&last_position) = viewer.minimap_positions.get(&id) {
                            minimap.insert(id, last_position);
                        }
                    } else if viewer.observer || linked || self.spotting.knows_enemy(own, id) {
                        let pack =
                            |coordinate: f32| coordinate.round().clamp(-32767.0, 32767.0) as i16;
                        let position = target.vehicle.position;
                        minimap.insert(
                            id,
                            MinimapContact {
                                row: target.roster_row,
                                position: [pack(position.x), pack(position.z)],
                                health: target.tank.health,
                            },
                        );
                    }
                }
                let mut fire_roots = viewer.fire_roots.clone();
                let result = (|| -> Result<()> {
                    for (&id, contact) in &minimap {
                        if visible.contains(&id) {
                            continue;
                        }
                        let previous = viewer.minimap_positions.get(&id);
                        let just_left = viewer.visible_vehicles.contains(&id);
                        if just_left
                            || previous.is_none_or(|last| last.position != contact.position)
                        {
                            wire.roster_position(
                                delivery,
                                peer,
                                self.roster_count,
                                contact.row,
                                contact.position,
                            )?;
                        }
                        if just_left || previous.is_none_or(|last| last.health != contact.health) {
                            delivery.send_avatar(
                                peer,
                                AvatarUpdate::ContactHealth {
                                    vehicle: id,
                                    health: contact.health,
                                },
                            )?;
                        }
                    }
                    for (&id, contact) in &viewer.minimap_positions {
                        if !minimap.contains_key(&id) {
                            wire.roster_position(
                                delivery,
                                peer,
                                self.roster_count,
                                contact.row,
                                [i16::MIN; 2],
                            )?;
                        }
                    }
                    for &id in viewer.visible_vehicles.difference(&visible) {
                        if let Some(target) = self
                            .players
                            .values()
                            .find(|player| player.vehicle.entity_id == id)
                            && fire_roots.remove(&target.roster_row)
                        {
                            wire.unlink_remote_fire(delivery, peer, target.roster_row)?;
                        }
                        delivery.send_arena_element(peer, LeaveArenaEntity { entity_id: id })?;
                        left += 1;
                    }
                    for target in self.players.values().filter(|player| {
                        let id = player.vehicle.entity_id;
                        visible.contains(&id) && !viewer.visible_vehicles.contains(&id)
                    }) {
                        wire.publish_visible_vehicle(delivery, peer, target, self.started_at)?;
                        revealed.insert(target.vehicle.entity_id);
                        if let Some(fire) = &target.active_fire {
                            wire.publish_fire_start(
                                delivery,
                                peer,
                                target.roster_row,
                                false,
                                fire,
                                Some(target.vehicle.entity_id),
                            )?;
                            fire_roots.insert(target.roster_row);
                        }
                        entered += 1;
                    }
                    if let Some(parent) = alerts_due.get(&own) {
                        let alert = tracing::info_span!(parent: parent, "battles.sixth_sense",
                            ddd.bounded_context.name = "battles", input.battle.id = battle_id,
                            input.entity_id = own, input.arena.tick = tick,
                            output.outcome = tracing::field::Empty, otel.status_code = tracing::field::Empty);
                        alert.in_scope(|| {
                            let result = wire.publish_sixth_sense(delivery, peer);
                            if let Err(error) = &result {
                                alert.record("otel.status_code", "ERROR");
                                alert.record("output.outcome", "delivery_failed");
                                error!(%error, "Could not publish Sixth Sense");
                            } else {
                                alert.record("output.outcome", "enqueued");
                            }
                            result
                        })?;
                        alerts += 1;
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    span.record("otel.status_code", "ERROR");
                    error!(%error, "Could not publish spotting state");
                    delivery.disconnect(peer);
                } else {
                    projections.push((peer, visible, minimap, fire_roots));
                }
            }
            for (peer, visible, minimap, fire_roots) in projections {
                if let Some(player) = self.players.get_mut(&peer) {
                    player.visible_vehicles = visible;
                    player.minimap_positions = minimap;
                    player.fire_roots = fire_roots;
                }
            }
            for player in self.players.values_mut() {
                if revealed.contains(&player.vehicle.entity_id) {
                    player.pose_cadence.refresh(tick);
                }
            }
            span.record("output.vehicles.entered", entered);
            span.record("output.vehicles.left", left);
            span.record("output.alerts.count", alerts);
        });
    }
}
