use super::*;

pub(in crate::infrastructure) struct MotionUpdate {
    pub client: ParticipantId,
    pub previous_direction: Vec3,
    pub previous_tracks: [f32; 2],
    pub drive: Option<Span>,
    pub parent: Option<Span>,
    pub changed: bool,
    pub broken: Vec<Breakage>,
    pub cause: BreakageCause,
}

struct PreviousMotion {
    position: Vec3,
    direction: Vec3,
    tracks: [f32; 2],
    drive: Option<Span>,
}

impl Battle {
    pub(in crate::infrastructure) fn advance_world(&mut self, seconds: f32) -> Vec<MotionUpdate> {
        let Some(physics) = self.physics.as_mut() else {
            return self
                .advance_standard_world(seconds)
                .into_iter()
                .map(|motion| MotionUpdate {
                    client: motion.client,
                    previous_direction: motion.previous_direction,
                    previous_tracks: motion.previous_tracks,
                    drive: motion.drive,
                    parent: motion.parent,
                    changed: motion.changed,
                    broken: motion.scenery.broken,
                    cause: motion.scenery.fall,
                })
                .collect();
        };

        let mut previous = HashMap::new();
        let mut parents = HashMap::new();
        for (&id, player) in self
            .players
            .iter_mut()
            .filter(|(_, p)| p.ready && !p.observer)
        {
            if self.round.phase == RoundPhase::Finished {
                player.tank.drive(0);
            }
            let drive = player.pending_drive.take();
            if player.tank.drive_readback().0 == 0 || player.tank.health <= 0 {
                player.held_drive = None;
            } else if drive.is_some() {
                player.held_drive.clone_from(&drive);
            }
            if let Some(parent) = drive.as_ref().or(player.held_drive.as_ref()) {
                parents.insert(id, parent.clone());
            }
            previous.insert(
                id,
                PreviousMotion {
                    position: player.vehicle.position,
                    direction: player.vehicle.direction,
                    tracks: player.tank.track_speeds(),
                    drive,
                },
            );
        }
        let command = parents
            .iter()
            .min_by_key(|(id, _)| id.0)
            .map(|(_, span)| span);
        let step = tracing::info_span!(
            parent: command.or(self.finish_span.as_ref()).unwrap_or(&Span::none()),
            "battles.hull_physics",
            "ddd.bounded_context.name" = "battles",
            "input.arena_type_id" = self.arena_type,
            "input.players.count" = previous.len(),
            "input.physics.tick_seconds" = seconds,
            "output.physics.substeps" = tracing::field::Empty,
            "otel.status_code" = tracing::field::Empty,
        );
        let motions = step.in_scope(|| {
            physics.advance(
                seconds,
                &self.ground,
                &mut self.obstacles,
                &mut self.players,
                self.round.phase == RoundPhase::Battle,
            )
        });
        step.record(
            "output.physics.substeps",
            motions.first().map_or(0, |motion| motion.substeps),
        );
        // Follow contact chains so that the pushed tank's delivery retains the initiating drive.
        for _ in 0..motions.len() {
            let before = parents.len();
            for motion in &motions {
                if !parents.contains_key(&motion.client)
                    && let Some(parent) = motion
                        .contact_peers
                        .iter()
                        .find_map(|id| parents.get(id))
                        .cloned()
                {
                    parents.insert(motion.client, parent);
                }
            }
            if parents.len() == before {
                break;
            }
        }
        let mut updates = Vec::with_capacity(motions.len());
        for motion in motions {
            let (Some(previous), Some(player)) = (
                previous.remove(&motion.client),
                self.players.get(&motion.client),
            ) else {
                continue;
            };
            let changed = previous.position != player.vehicle.position
                || previous.direction != player.vehicle.direction;
            let mut parent = self
                .finish_span
                .clone()
                .or_else(|| parents.remove(&motion.client));
            if changed || parent.is_some() || !motion.broken.is_empty() {
                let outcome = if !motion.broken.is_empty() {
                    "scenery_destroyed"
                } else if motion.vehicle_contacts > 0 || motion.scenery_contact {
                    "contact"
                } else if motion.grounded {
                    "supported"
                } else {
                    "airborne"
                };
                let (flags, speed, yaw_rate) = player.tank.drive_readback();
                let span = tracing::info_span!(
                    parent: parent.as_ref().unwrap_or(&step), "battles.world_motion",
                    "ddd.bounded_context.name" = "battles",
                    "input.entity_id" = player.vehicle.entity_id,
                    "input.drive.flags" = flags,
                    "output.drive.speed_mps" = speed,
                    "output.drive.yaw_rate_rad_per_second" = yaw_rate,
                    "output.world.scenery_contact" = motion.scenery_contact,
                    "output.world.vehicle_contacts" = motion.vehicle_contacts,
                    "output.world.displacement_metres" = previous.position.xz().distance(player.vehicle.position.xz()),
                    "output.world.roll_radians" = player.vehicle.direction.x,
                    "output.world.pitch_radians" = player.vehicle.direction.y,
                    "output.world.lateral_speed_mps" = motion.lateral_speed_mps,
                    "output.world.vertical_speed_mps" = motion.vertical_speed_mps,
                    "output.world.scenery_support" = motion.scenery_support,
                    "output.world.grounded" = motion.grounded,
                    "output.world.fragments_destroyed" = motion.broken.len(),
                    "output.world.scenery_speed_scale" = motion.scenery_speed_scale,
                    "output.outcome" = outcome,
                );
                parent = Some(span);
            }
            updates.push(MotionUpdate {
                client: motion.client,
                previous_direction: previous.direction,
                previous_tracks: previous.tracks,
                drive: previous.drive,
                parent,
                changed,
                broken: motion.broken,
                cause: motion.fall,
            });
        }
        updates
    }
}
