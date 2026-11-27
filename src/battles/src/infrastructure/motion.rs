use super::obstacles::{Breakage, Scenery};
use super::scenery::BreakageCause;
use super::*;
use crate::domain::world::{ContactBox, Hull, vehicle_contact};
use glam::Vec3Swizzles;
use tracing::Span;

mod scenery;
mod update;
use scenery::PRESS_REACH;
pub(super) use scenery::SceneryMotion;

const CONTACT_PASSES: usize = 12;
const PILEUP_TOLERANCE_METRES: f32 = 0.005;
const MAX_SUPPORT_STEP_METRES: f32 = 0.45;

pub(super) struct VehicleMotion {
    pub client: ParticipantId,
    pub previous_direction: Vec3,
    pub previous_tracks: [f32; 2],
    pub drive: Option<Span>,
    pub parent: Option<Span>,
    pub changed: bool,
    hull: Hull,
    travel: Vec2,
    inverse_mass: f32,
    scenery_normal: Option<Vec2>,
    scenery_impulse: Vec2,
    vehicle_normal: Option<Vec2>,
    vehicle_contacts: u32,
    scenery: SceneryMotion,
    drive_pressure: Vec2,
    pushed_pressure: Vec2,
    impulse: Vec2,
    slid: bool,
    pileup_held: bool,
}

impl VehicleMotion {
    #[cfg(test)]
    pub(super) fn breakages(&self) -> (&[Breakage], BreakageCause) {
        (&self.scenery.broken, self.scenery.fall)
    }

    fn is_stationary(&self) -> bool {
        self.travel == Vec2::ZERO && self.hull.yaw == self.previous_direction.z
    }

    fn pressure_towards(&self, direction: Vec2) -> Vec2 {
        direction
            * self
                .drive_pressure
                .dot(direction)
                .max(self.pushed_pressure.dot(direction))
                .max(0.0)
    }

    fn resolve_push(
        &mut self,
        obstacles: &mut Scenery,
        bounds: [Vec2; 2],
        push: Vec2,
        pressure: Vec2,
        can_damage: bool,
    ) {
        let wanted = self.travel + push;
        let breaks_before = self.scenery.broken.len();
        if can_damage {
            let probe = wanted + pressure.normalize_or_zero() * PRESS_REACH;
            let origin = self.hull.position.xz();
            let probe = (origin + probe).clamp(bounds[0], bounds[1]) - origin;
            self.scenery
                .push(obstacles, self.hull, probe, wanted, pressure);
        }
        let (allowed, contact) = constrain(obstacles, self.hull, wanted, bounds);
        self.travel = allowed;
        if self.scenery.broken.len() != breaks_before {
            self.scenery_normal = contact;
            self.scenery_impulse = Vec2::ZERO;
        } else {
            self.scenery_normal = contact.or(self.scenery_normal);
        }
        if pressure.length_squared() > self.pushed_pressure.length_squared() {
            self.pushed_pressure = pressure;
        }
    }
}

impl Battle {
    fn advance_standard_world(&mut self, seconds: f32) -> Vec<VehicleMotion> {
        let mut ids: Vec<_> = self
            .players
            .iter()
            .filter_map(|(&id, p)| (p.ready && !p.observer).then_some(id))
            .collect();
        ids.sort_unstable_by_key(|id| id.0);

        let occupants: Vec<_> = ids
            .iter()
            .filter_map(|id| {
                self.players
                    .get(id)
                    .map(|player| (*id, player.tank.collision_hull().contact_box()))
            })
            .collect();
        let mut moves = Vec::with_capacity(ids.len());
        let can_damage = self.round.phase == RoundPhase::Battle;
        for client in ids {
            let Some(player) = self.players.get_mut(&client) else {
                continue;
            };
            let tank = &mut player.tank;
            let mut hull = tank.collision_hull();
            let previous_direction = player.vehicle.direction;
            let previous_tracks = tank.track_speeds();
            let drive = player.pending_drive.take();
            let parent = self.finish_span.clone().or_else(|| drive.clone());
            let inverse_mass = if player.entry.is_none() {
                0.0
            } else {
                tank.collision_inverse_mass()
            };
            let mut travel = Vec2::ZERO;
            let mut scenery_normal = None;
            let mut scenery_impulse = Vec2::ZERO;
            let mut slid = false;
            let mut scenery = SceneryMotion::new(inverse_mass, seconds);
            let mut drive_pressure = Vec2::ZERO;
            if inverse_mass == 0.0 {
                tank.stop_motion();
            } else {
                if self.round.phase == RoundPhase::Finished {
                    tank.drive(0);
                }
                let support = surface_pose(&self.ground, &self.obstacles, hull, Vec2::ZERO)
                    .map_or(previous_direction, |(_, tilt, _)| tilt);
                let ground = self.ground.ground_contact(
                    hull.position.xz(),
                    hull.yaw,
                    hull.half_size.x,
                    support.y,
                );
                if drive.is_some() || tank.is_driving() {
                    let mut step = || tank.step(seconds, self.map.bounds, ground);
                    match &parent {
                        Some(parent) => parent.in_scope(step),
                        None => step(),
                    };
                }
                // Slopes and previous impulses can also move an idle or overturned hull.
                let (passive, sliding) = tank.passive_travel(support.xy(), ground, seconds);
                slid = sliding;
                travel = tank.position.xz() - hull.position.xz() + passive;
                (hull, scenery_normal) = scenery.turn(
                    &mut self.obstacles,
                    hull,
                    tank.yaw,
                    tank.scenery_push_load(seconds, true),
                    can_damage,
                );
                tank.yaw = hull.yaw;
                if can_damage {
                    let pressure = tank.scenery_push_load(seconds, false);
                    let direction = tank.scenery_push_direction();
                    drive_pressure = direction * pressure;
                    let probe = travel + direction * if pressure > 0.0 { PRESS_REACH } else { 0.0 };
                    let probe = (hull.position.xz() + probe)
                        .clamp(self.map.bounds[0], self.map.bounds[1])
                        - hull.position.xz();
                    if probe.length_squared() > 1e-8 {
                        let contact_box = hull.contact_box();
                        let reachable = occupants
                            .iter()
                            .filter(|(id, _)| *id != client)
                            .filter_map(|(_, other)| {
                                vehicle_contact(&contact_box, other, probe, Vec2::ZERO)
                            })
                            .fold(1.0_f32, |fraction, (hit, _)| fraction.min(hit.fraction));
                        scenery.push(
                            &mut self.obstacles,
                            hull,
                            probe * reachable,
                            travel * reachable,
                            drive_pressure,
                        );
                    }
                }
                let (allowed, contact) = constrain(&self.obstacles, hull, travel, self.map.bounds);
                if let Some(normal) = contact {
                    scenery_impulse = normal * (-travel.dot(normal) / seconds).clamp(0.0, 12.0);
                }
                travel = allowed;
                scenery_normal = contact.or(scenery_normal);
            }
            moves.push(VehicleMotion {
                client,
                previous_direction,
                previous_tracks,
                drive,
                parent,
                changed: false,
                hull,
                travel,
                inverse_mass,
                scenery_normal,
                scenery_impulse,
                vehicle_normal: None,
                vehicle_contacts: 0,
                scenery,
                drive_pressure,
                pushed_pressure: Vec2::ZERO,
                impulse: Vec2::ZERO,
                slid,
                pileup_held: false,
            });
        }

        resolve_contacts(
            &mut moves,
            &mut self.obstacles,
            self.map.bounds,
            seconds,
            can_damage,
        );
        for motion in &mut moves {
            let Some(player) = self.players.get_mut(&motion.client) else {
                continue;
            };
            if motion.inverse_mass == 0.0 {
                continue;
            }
            let tank = &mut player.tank;
            tank.apply_scenery_drag(motion.scenery.speed_scale);
            if motion.pileup_held {
                tank.yaw = motion.hull.yaw;
                tank.hold_world_position();
            }
            let before = tank.is_tipped();
            let previous = player.vehicle.position;
            let destination = motion.hull.position.xz() + motion.travel;
            let Some((ground, direction, deck)) =
                surface_pose(&self.ground, &self.obstacles, motion.hull, motion.travel)
            else {
                tank.position = previous;
                tank.yaw = motion.previous_direction.z;
                tank.stop_motion();
                continue;
            };
            for normal in [motion.scenery_normal, motion.vehicle_normal]
                .into_iter()
                .flatten()
            {
                tank.world_contact(normal);
            }
            tank.world_impulse(motion.impulse * 0.4);
            if let Some(normal) = motion.scenery_normal {
                // Tank contact must not build up momentum through a wall.
                tank.world_contact(normal);
            }
            tank.position.x = destination.x;
            tank.position.z = destination.y;
            let direction = tank.follow_terrain(ground, direction, seconds);
            let impulse = motion.impulse + motion.scenery_impulse;
            let stationary = (tank.position.xz() - previous.xz()).length_squared() < 0.0001
                && impulse.length_squared() < 0.01;
            let direction = tank.settle_hull(direction, impulse, stationary, seconds);
            let after = tank.is_tipped();
            motion.changed = tank.position != previous || direction != motion.previous_direction;
            player.vehicle.position = tank.position;
            player.vehicle.direction = direction;
            let event = match (before, after) {
                (false, true) => Some("overturned"),
                (true, false) => Some("righted"),
                _ if motion.pileup_held => Some("pileup_held"),
                _ if motion.scenery_normal.is_some() || motion.vehicle_contacts != 0 => {
                    Some("contact")
                }
                _ if !motion.scenery.broken.is_empty() => Some("scenery_destroyed"),
                _ if motion.slid => Some("slide"),
                _ if deck && motion.changed => Some("supported"),
                _ => None,
            };
            if let Some(outcome) = event {
                let span = tracing::info_span!(
                    parent: motion.parent.as_ref().unwrap_or(&Span::none()),
                    "battles.world_motion",
                    "ddd.bounded_context.name" = "battles",
                    "input.entity_id" = player.vehicle.entity_id,
                    "output.world.scenery_contact" = motion.scenery_normal.is_some(),
                    "output.world.vehicle_contacts" = motion.vehicle_contacts,
                    "output.world.displacement_metres" = motion.travel.length(),
                    "output.world.roll_radians" = direction.x,
                    "output.world.scenery_support" = deck,
                    "output.world.fragments_destroyed" = motion.scenery.broken.len(),
                    "output.world.turn_contact" = motion.scenery.turn_contact,
                    "output.world.scenery_speed_scale" = motion.scenery.speed_scale,
                    "output.outcome" = outcome,
                );
                motion.parent = Some(span);
            }
        }
        moves
    }
}

fn surface_pose(
    terrain: &terrain::Terrain,
    scenery: &Scenery,
    hull: Hull,
    travel: Vec2,
) -> Option<(Vec3, Vec3, bool)> {
    let point = hull.position.xz() + travel;
    let floor = terrain.height_at(point);
    let ceiling = hull.position.y + MAX_SUPPORT_STEP_METRES + travel.length().min(3.0) * 0.75;
    let bottom = floor.unwrap_or(hull.position.y - 30.0);
    let Some(deck) = scenery.support_height(point, bottom + 0.001, ceiling) else {
        return terrain
            .vehicle_pose(point, hull.yaw)
            .map(|(pos, dir)| (pos, dir, false));
    };
    let (position, direction) =
        terrain::pose_from_samples(point, hull.yaw, hull.half_size, deck, |sample| {
            let lower = terrain
                .height_at(sample)
                .unwrap_or(deck - 1.5)
                .max(deck - 1.5);
            scenery
                .support_height(sample, lower + 0.001, deck + 1.5)
                .or_else(|| {
                    terrain
                        .height_at(sample)
                        .filter(|&y| (y - deck).abs() <= 1.5)
                })
                .unwrap_or(deck)
        });
    Some((position, direction, true))
}

fn resolve_contacts(
    moves: &mut [VehicleMotion],
    obstacles: &mut Scenery,
    bounds: [Vec2; 2],
    seconds: f32,
    can_damage: bool,
) {
    if moves.len() < 2 || moves.iter().all(VehicleMotion::is_stationary) {
        return;
    }
    let contact_boxes: Vec<_> = moves
        .iter()
        .map(|motion| motion.hull.contact_box())
        .collect();
    let mut groups: Vec<_> = (0..moves.len()).collect();
    let mut seen = HashSet::new();
    for pass in 0..CONTACT_PASSES {
        let mut any = false;
        for offset in 1..moves.len() {
            let i = if pass % 2 == 0 {
                offset
            } else {
                moves.len() - offset
            };
            let (prior, rest) = moves.split_at_mut(i);
            let (Some(other), Some((other_box, prior_boxes))) = (
                rest.first_mut(),
                contact_boxes.get(..=i).and_then(<[ContactBox]>::split_last),
            ) else {
                continue;
            };
            for (j, (first, first_box)) in prior.iter_mut().zip(prior_boxes).enumerate() {
                if first.is_stationary() && other.is_stationary() {
                    continue;
                }
                let total = first.inverse_mass + other.inverse_mass;
                if total <= 0.0 {
                    continue;
                }
                let Some((hit, correction)) =
                    vehicle_contact(first_box, other_box, first.travel, other.travel)
                else {
                    continue;
                };
                any = true;
                join_group(&mut groups, i, j);
                let new_contact = seen.insert((i, j));
                let first_share = first.inverse_mass / total;
                let other_share = other.inverse_mass / total;
                let closing = -(first.travel - other.travel).dot(hit.normal) / seconds;
                let cause = first.parent.clone().or_else(|| other.parent.clone());
                let first_load = other.pressure_towards(hit.normal);
                let other_load = first.pressure_towards(-hit.normal);
                for (motion, normal, share, load) in [
                    (&mut *first, hit.normal, first_share, first_load),
                    (&mut *other, -hit.normal, other_share, other_load),
                ] {
                    if share == 0.0 {
                        continue;
                    }
                    motion.parent = motion.parent.take().or_else(|| cause.clone());
                    if new_contact {
                        motion.impulse += normal * closing.clamp(0.0, 24.0) * share;
                        motion.vehicle_contacts += 1;
                    }
                    motion.vehicle_normal = Some(normal);
                    motion.resolve_push(
                        obstacles,
                        bounds,
                        normal * correction * share,
                        load,
                        can_damage,
                    );
                }

                // If a wall, wreck or boundary denied one body's share, the other
                // body stops at the remaining contact instead of pushing it through.
                if let Some((residual, remainder)) =
                    vehicle_contact(first_box, other_box, first.travel, other.travel)
                    && remainder > 0.001
                {
                    for (motion, normal) in [
                        (&mut *first, residual.normal),
                        (&mut *other, -residual.normal),
                    ] {
                        if motion.inverse_mass == 0.0 {
                            continue;
                        }
                        let wanted = motion.travel + normal * remainder;
                        let (allowed, contact) = constrain(obstacles, motion.hull, wanted, bounds);
                        if (allowed - wanted).length_squared() < 0.0001 {
                            motion.travel = allowed;
                            motion.scenery_normal = contact.or(motion.scenery_normal);
                            break;
                        }
                    }
                }
            }
        }
        if !any {
            return;
        }
    }
    hold_unresolved_groups(moves, &contact_boxes, &mut groups);
}

fn group(groups: &mut [usize], mut index: usize) -> usize {
    while groups[index] != index {
        groups[index] = groups[groups[index]];
        index = groups[index];
    }
    index
}

fn join_group(groups: &mut [usize], a: usize, b: usize) {
    let left = group(groups, a);
    let right = group(groups, b);
    groups[right] = left;
}

fn hold_unresolved_groups(
    moves: &mut [VehicleMotion],
    contact_boxes: &[ContactBox],
    groups: &mut [usize],
) {
    let mut unresolved = Vec::new();
    for (i, (other, other_box)) in moves.iter().zip(contact_boxes).enumerate() {
        for (j, (first, first_box)) in moves.iter().zip(contact_boxes).take(i).enumerate() {
            if first.is_stationary() && other.is_stationary() {
                continue;
            }
            if vehicle_contact(first_box, other_box, first.travel, other.travel)
                .is_some_and(|(_, depth)| depth > PILEUP_TOLERANCE_METRES)
            {
                join_group(groups, i, j);
                unresolved.push(i);
            }
        }
    }
    if unresolved.is_empty() {
        return;
    }
    // Also group paths that would become blocked if their neighbour held its original pose.
    for (i, (other, other_box)) in moves.iter().zip(contact_boxes).enumerate() {
        for (j, (first, first_box)) in moves.iter().zip(contact_boxes).take(i).enumerate() {
            if (first.travel != Vec2::ZERO
                && vehicle_contact(first_box, other_box, first.travel, Vec2::ZERO).is_some())
                || (other.travel != Vec2::ZERO
                    && vehicle_contact(first_box, other_box, Vec2::ZERO, other.travel).is_some())
            {
                join_group(groups, i, j);
            }
        }
    }
    let blocked: HashSet<_> = unresolved.into_iter().map(|i| group(groups, i)).collect();
    for (i, motion) in moves.iter_mut().enumerate() {
        if blocked.contains(&group(groups, i)) {
            motion.travel = Vec2::ZERO;
            motion.hull.yaw = motion.previous_direction.z;
            motion.impulse = Vec2::ZERO;
            motion.pileup_held = true;
        }
    }
}

fn constrain(
    obstacles: &Scenery,
    mut hull: Hull,
    wanted: Vec2,
    [min, max]: [Vec2; 2],
) -> (Vec2, Option<Vec2>) {
    let origin = hull.position.xz();
    let mut remaining = (origin + wanted).clamp(min, max) - origin;
    let mut normal = None;
    for _ in 0..2 {
        if remaining.length_squared() < 1e-8 {
            break;
        }
        let Some(hit) = obstacles.sweep_hull(hull, remaining) else {
            hull.position.x += remaining.x;
            hull.position.z += remaining.y;
            break;
        };
        let hit = hit.contact;
        let fraction =
            (hit.fraction - hull.parameters.contact_skin_metres / remaining.length()).max(0.0);
        hull.position.x += remaining.x * fraction;
        hull.position.z += remaining.y * fraction;
        remaining *= 1.0 - fraction;
        remaining -= hit.normal * remaining.dot(hit.normal).min(0.0);
        normal = Some(hit.normal);
    }
    (hull.position.xz().clamp(min, max) - origin, normal)
}

#[cfg(test)]
mod tests;
