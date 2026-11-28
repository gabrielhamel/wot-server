use super::motion::SceneryMotion;
use super::obstacles::{Breakage, Scenery, SceneryColliders};
use super::scenery::BreakageCause;
use super::terrain::Terrain;
use super::{BattleParameters, Participant, ParticipantId, Result, Tank, Vec2, Vec3};
use crate::domain::parameters::{
    MAX_ANGULAR_SPEED, MAX_LINEAR_SPEED, MIN_COLLIDER_HALF_EXTENT, PHYSICS_HZ,
    PHYSICS_STEP_SECONDS as STEP_SECONDS,
};
use glam::{EulerRot, Quat, Vec3Swizzles};
use rapier3d::parry::shape::{Cuboid, RoundCuboid};
use rapier3d::prelude::{
    ColliderBuilder, ColliderHandle, PhysicsWorld, Pose, QueryFilter, Ray, RigidBody,
    RigidBodyBuilder, RigidBodyHandle, RigidBodyType, Rotation, Vector,
};
use std::collections::{BTreeMap, HashMap};

mod contacts;
mod tracks;

const MAX_STEPS_PER_TICK: u32 = 12;

pub(super) struct ArenaPhysics {
    world: PhysicsWorld,
    scenery: SceneryColliders,
    chassis: BTreeMap<u64, Chassis>,
    parameters: BattleParameters,
    remainder: f64,
    scenery_regions: Vec<(Vec3, Vec3)>,
}

struct Chassis {
    body: RigidBodyHandle,
    collider: ColliderHandle,
    contact_hull: RoundCuboid,
    hull_center: Vector,
    track_half_width: f32,
    track_half_length: f32,
    suspension_travel: f32,
    yaw_inertia_per_kg: f32,
    query_radius: f32,
    supported_by: [Option<ColliderHandle>; tracks::SUPPORT_POINTS],
}

pub(super) struct PhysicsMotion {
    pub client: ParticipantId,
    pub grounded: bool,
    pub scenery_support: bool,
    pub contact_peers: Vec<ParticipantId>,
    pub vehicle_contacts: u32,
    pub scenery_contact: bool,
    pub broken: Vec<Breakage>,
    pub fall: BreakageCause,
    pub scenery_speed_scale: f32,
    pub lateral_speed_mps: Option<f32>,
    pub vertical_speed_mps: Option<f32>,
    pub substeps: u32,
}

struct StepState {
    motion: PhysicsMotion,
    damage: SceneryMotion,
    velocity: Vector,
    angular_velocity: Vector,
    mass_center: Vector,
    press: Vec2,
    pivot_press: f32,
    track_contact: Option<contacts::TrackContact>,
}

fn game_vec(value: Vector) -> Vec3 {
    Vec3::from_array(value.to_array())
}
fn physics_vec(value: Vec3) -> Vector {
    Vector::from_array(value.to_array())
}

fn physics_pose(position: Vec3, direction: Vec3) -> Pose {
    let rotation = Quat::from_euler(EulerRot::YXZ, direction.z, direction.y, direction.x);
    Pose::from_parts(
        physics_vec(position),
        Rotation::from_array(rotation.to_array()),
    )
}

fn game_direction(body: &RigidBody) -> Vec3 {
    let (yaw, pitch, roll) = Quat::from_array(body.rotation().to_array()).to_euler(EulerRot::YXZ);
    Vec3::new(roll, pitch, yaw)
}

impl ArenaPhysics {
    pub(super) fn new(
        ground: &Terrain,
        scenery: &Scenery,
        bounds: [Vec2; 2],
        parameters: BattleParameters,
    ) -> Result<Self> {
        let mut world = PhysicsWorld {
            gravity: Vector::NEG_Y * parameters.gravity_mps2,
            ..Default::default()
        };
        world.integration_parameters.dt = STEP_SECONDS;
        ground.insert_physics_colliders(&mut world.colliders);
        let scenery = scenery.physics_colliders()?;
        let [min, max] = bounds;
        anyhow::ensure!(
            min.is_finite() && max.is_finite() && min.cmplt(max).all(),
            "Invalid physics arena bounds"
        );
        for (axis, distance) in [
            (Vector::X, min.x),
            (Vector::NEG_X, -max.x),
            (Vector::Z, min.y),
            (Vector::NEG_Z, -max.y),
        ] {
            let normal = rapier3d::prelude::nalgebra::Unit::new_unchecked(axis);
            world
                .colliders
                .insert(ColliderBuilder::halfspace(normal).translation(axis * distance));
        }
        world.detect_collisions(&(), &());
        Ok(Self {
            world,
            scenery,
            chassis: BTreeMap::new(),
            parameters,
            remainder: 0.0,
            scenery_regions: Vec::new(),
        })
    }

    pub(super) fn warm_spawns(
        &mut self,
        scenery: &Scenery,
        players: &HashMap<ParticipantId, Participant>,
    ) {
        self.scenery_regions.clear();
        self.scenery_regions
            .extend(
                players
                    .values()
                    .filter(|player| !player.observer)
                    .map(|player| {
                        let (half, center) = Chassis::bounds(&player.tank);
                        let travel = self.parameters.suspension_travel_metres.min(half.y);
                        let radius = Vec3::splat(Chassis::query_radius(half, center, travel));
                        let pose = physics_pose(player.tank.position, player.vehicle.direction);
                        let origin = game_vec(pose * (center + Vector::Y * (travel * 0.5)));
                        (origin - radius, origin + radius)
                    }),
            );
        self.scenery.update_near(
            &mut self.world,
            scenery,
            self.scenery_regions.iter().copied(),
        );
    }

    pub(super) fn advance(
        &mut self,
        seconds: f32,
        ground: &Terrain,
        scenery: &mut Scenery,
        players: &mut HashMap<ParticipantId, Participant>,
        can_damage: bool,
    ) -> Vec<PhysicsMotion> {
        self.sync_participants(players);
        let seconds = if seconds.is_finite() {
            seconds.max(0.0)
        } else {
            0.0
        };
        let dt = 1.0 / f64::from(PHYSICS_HZ);
        self.remainder += f64::from(seconds).clamp(0.0, f64::from(MAX_STEPS_PER_TICK) * dt);
        let substeps = ((self.remainder / dt + 1e-7).floor() as u32).min(MAX_STEPS_PER_TICK);
        self.remainder = (self.remainder - f64::from(substeps) * dt).max(0.0);
        let mut states: Vec<_> = self
            .chassis
            .keys()
            .filter_map(|&id| {
                let client = ParticipantId(id);
                let tank = &players.get(&client)?.tank;
                Some(StepState::new(client, tank, substeps))
            })
            .collect();
        let mut rejected_track_impulse = false;
        for _ in 0..substeps {
            self.scenery_regions.clear();
            self.scenery_regions
                .extend(self.chassis.values().filter_map(|chassis| {
                    let body = self.world.bodies.get(chassis.body)?;
                    let start = game_vec(body.center_of_mass());
                    let end = start
                        + game_vec(body.linvel() + self.world.gravity * STEP_SECONDS)
                            * STEP_SECONDS;
                    let radius = Vec3::splat(chassis.query_radius);
                    Some((start.min(end) - radius, start.max(end) + radius))
                }));
            self.scenery.update_near(
                &mut self.world,
                scenery,
                self.scenery_regions.iter().copied(),
            );
            self.scenery.sync(scenery, &mut self.world.colliders);
            for state in &mut states {
                let id = state.motion.client;
                let (Some(player), Some(chassis)) =
                    (players.get_mut(&id), self.chassis.get_mut(&id.0))
                else {
                    continue;
                };
                state.press = player.tank.scenery_push_direction()
                    * player.tank.scenery_push_load(seconds, false);
                state.pivot_press = player.tank.scenery_push_load(seconds, true);
                if can_damage && player.entry.is_some() && player.tank.health > 0 {
                    state.break_approaching_scenery(
                        &mut self.world,
                        &mut self.scenery,
                        chassis,
                        scenery,
                    );
                }
                let supported = tracks::drive(
                    &mut self.world,
                    &self.scenery,
                    chassis,
                    ground,
                    &mut player.tank,
                    &self.parameters,
                );
                rejected_track_impulse |= supported.rejected_impulse;
                state.motion.grounded = supported.grounded;
                state.motion.scenery_support = supported.scenery;
                if let Some(body) = self.world.bodies.get_mut(chassis.body) {
                    bound_solver_velocity(body);
                    state.velocity = body.linvel();
                    state.angular_velocity = body.angvel();
                    state.mass_center = body.center_of_mass();
                }
            }
            self.world.step();
            let quarantine = self.world.quarantine();
            if !quarantine.is_empty() {
                tracing::Span::current().record("otel.status_code", "ERROR");
                tracing::error!(
                    bodies = quarantine.bodies().len(),
                    colliders = quarantine.colliders().len(),
                    "Rapier disabled non-finite arena physics objects"
                );
            }
            for state in &mut states {
                let id = state.motion.client;
                let (Some(player), Some(chassis)) = (players.get_mut(&id), self.chassis.get(&id.0))
                else {
                    continue;
                };
                state.contacts(
                    &mut self.world,
                    &mut self.scenery,
                    chassis,
                    scenery,
                    (f64::from(self.parameters.scenery_push_force_scale) * f64::from(seconds)
                        / f64::from(STEP_SECONDS))
                    .min(f64::from(f32::MAX)) as f32,
                    can_damage && player.entry.is_some() && player.tank.health > 0,
                );
                let Some(body) = self.world.bodies.get_mut(chassis.body) else {
                    continue;
                };
                if !body.is_dynamic() || !body.is_enabled() {
                    continue;
                }
                bound_solver_velocity(body);
                let position = game_vec(body.translation());
                let direction = game_direction(body);
                let stationary = body.is_sleeping() || body.activation().time_since_can_sleep > 0.0;
                let grounded = state.motion.grounded;
                let rotation = *body.rotation();
                player.tank.sync_physics(
                    position,
                    direction,
                    body.linvel().dot(rotation * Vector::Z),
                    body.angvel().dot(rotation * Vector::Y),
                    grounded,
                );
                player.vehicle.position = position;
                player.vehicle.direction = direction;
                let settled = player.tank.is_tipped()
                    && stationary
                    && has_recovery_support(
                        &self.world,
                        chassis,
                        self.parameters.tip_terrain_roll_radians,
                    );
                if player.tank.physical_recovery_due(settled, STEP_SECONDS) {
                    recover_upright(&mut self.world, chassis, player);
                }
            }
        }
        if rejected_track_impulse {
            tracing::Span::current().record("otel.status_code", "ERROR");
            tracing::error!("Skipped non-finite arena track impulse");
        }
        states
            .into_iter()
            .map(|mut state| {
                state.motion.broken = state.damage.broken;
                state.motion.fall = state.damage.fall;
                state.motion.scenery_speed_scale = state.damage.speed_scale;
                state.motion.vehicle_contacts = state.motion.contact_peers.len() as u32;
                if let Some(body) = self
                    .chassis
                    .get(&state.motion.client.0)
                    .and_then(|chassis| self.world.bodies.get(chassis.body))
                    .filter(|body| body.is_dynamic() && body.is_enabled())
                {
                    state.motion.lateral_speed_mps =
                        Some(body.linvel().dot(*body.rotation() * Vector::X));
                    state.motion.vertical_speed_mps = Some(body.linvel().y);
                }
                state.motion
            })
            .collect()
    }

    fn sync_participants(&mut self, players: &mut HashMap<ParticipantId, Participant>) {
        self.chassis.retain(|id, chassis| {
            if players
                .get(&ParticipantId(*id))
                .is_some_and(|p| p.ready && !p.observer)
            {
                return true;
            }
            self.world.remove_body(chassis.body);
            false
        });
        let mut ids: Vec<_> = players
            .iter()
            .filter_map(|(&id, p)| (p.ready && !p.observer).then_some(id))
            .collect();
        ids.sort_unstable_by_key(|id| id.0);
        for id in ids {
            let Some(player) = players.get_mut(&id) else {
                continue;
            };
            let kind = if player.entry.is_none() || player.tank.health <= 0 {
                RigidBodyType::Fixed
            } else {
                RigidBodyType::Dynamic
            };
            if kind == RigidBodyType::Fixed {
                player.tank.stop_motion();
            }
            if let Some(chassis) = self.chassis.get(&id.0) {
                if let Some(body) = self.world.bodies.get_mut(chassis.body)
                    && body.body_type() != kind
                {
                    body.set_body_type(kind, true);
                }
                continue;
            }
            let chassis = Chassis::insert(
                &mut self.world,
                id,
                &player.tank,
                player.vehicle.direction,
                kind,
            );
            self.chassis.insert(id.0, chassis);
        }
    }
}

fn bound_solver_velocity(body: &mut RigidBody) {
    let linear = body.linvel();
    let angular = body.angvel();
    if linear.is_finite() && linear.length_squared() > MAX_LINEAR_SPEED * MAX_LINEAR_SPEED {
        let limit = Vector::splat(MAX_LINEAR_SPEED);
        body.set_linvel(
            linear
                .clamp(-limit, limit)
                .clamp_length_max(MAX_LINEAR_SPEED),
            false,
        );
    }
    if angular.is_finite() && angular.length_squared() > MAX_ANGULAR_SPEED * MAX_ANGULAR_SPEED {
        let limit = Vector::splat(MAX_ANGULAR_SPEED);
        body.set_angvel(
            angular
                .clamp(-limit, limit)
                .clamp_length_max(MAX_ANGULAR_SPEED),
            false,
        );
    }
}

impl Chassis {
    fn query_radius(half: Vec3, center: Vector, travel: f32) -> f32 {
        (center.length() + half.z * 2.0).max(half.length() + travel * 2.0)
    }

    fn bounds(tank: &Tank) -> (Vec3, Vector) {
        let fallback_half = Vec3::new(
            tank.parameters.fallback_hull_half_width_metres,
            tank.parameters.hull_height_metres * 0.5,
            tank.parameters.fallback_hull_half_length_metres,
        );
        let (half, center) = tank.collision_extents();
        if center.is_finite() && half.is_finite() && half.min_element() >= MIN_COLLIDER_HALF_EXTENT
        {
            (half, physics_vec(center))
        } else {
            (fallback_half, Vector::Y * fallback_half.y)
        }
    }

    fn insert(
        world: &mut PhysicsWorld,
        id: ParticipantId,
        tank: &Tank,
        direction: Vec3,
        kind: RigidBodyType,
    ) -> Self {
        let (half, center) = Self::bounds(tank);
        let mobility = tank.performance.as_ref().map(|profile| &profile.mobility);
        let mass = mobility
            .map(|profile| profile.mass_kg)
            .filter(|mass| mass.is_finite() && *mass > 0.0)
            .unwrap_or(tank.parameters.fallback_mass_kg);
        let travel = tank.parameters.suspension_travel_metres.min(half.y);
        let (body, collider) = world.insert(
            RigidBodyBuilder::new(kind)
                .pose(physics_pose(tank.position, direction))
                .ccd_enabled(kind == RigidBodyType::Dynamic),
            ColliderBuilder::cuboid(half.x, half.y - travel * 0.5, half.z)
                .translation(center + Vector::Y * (travel * 0.5))
                .mass(mass)
                .contact_skin(tank.parameters.contact_skin_metres)
                .user_data(u128::from(id.0) + 1),
        );
        let track_width = mobility.map(|profile| profile.track_half_width);
        let track_length = mobility.map(|profile| profile.track_half_length);
        let track_extent = |value: Option<f32>, extent: f32| {
            value
                .filter(|v| v.is_finite() && *v >= MIN_COLLIDER_HALF_EXTENT)
                .unwrap_or(extent)
                .min(extent)
        };
        Self {
            body,
            collider,
            contact_hull: RoundCuboid {
                inner_shape: Cuboid::new(physics_vec(half)),
                border_radius: tank.parameters.contact_skin_metres,
            },
            hull_center: center,
            track_half_width: track_extent(track_width, half.x),
            track_half_length: track_extent(track_length, half.z),
            suspension_travel: travel,
            supported_by: [None; tracks::SUPPORT_POINTS],
            query_radius: Self::query_radius(half, center, travel),
            yaw_inertia_per_kg: (half.x * half.x + half.z * half.z) / 3.0,
        }
    }
}

impl StepState {
    fn new(client: ParticipantId, tank: &Tank, substeps: u32) -> Self {
        let damage = SceneryMotion::new(tank.collision_inverse_mass());
        Self {
            motion: PhysicsMotion {
                client,
                grounded: false,
                scenery_support: false,
                contact_peers: Vec::new(),
                vehicle_contacts: 0,
                scenery_contact: false,
                broken: Vec::new(),
                fall: damage.fall,
                scenery_speed_scale: 1.0,
                lateral_speed_mps: None,
                vertical_speed_mps: None,
                substeps,
            },
            damage,
            velocity: Vector::ZERO,
            angular_velocity: Vector::ZERO,
            mass_center: Vector::ZERO,
            press: Vec2::ZERO,
            pivot_press: 0.0,
            track_contact: None,
        }
    }
}

fn has_recovery_support(world: &PhysicsWorld, chassis: &Chassis, tip_threshold: f32) -> bool {
    let required_normal_y = (tip_threshold * 0.5).cos();
    world
        .narrow_phase
        .contact_pairs_with(chassis.collider)
        .any(|pair| {
            let first = pair.collider1 == chassis.collider;
            let other = if first {
                pair.collider2
            } else {
                pair.collider1
            };
            pair.has_any_active_contact()
                && world
                    .colliders
                    .get(other)
                    .is_some_and(|c| c.parent().is_none())
                && pair.manifolds().iter().any(|manifold| {
                    let normal = if first {
                        -manifold.data.normal
                    } else {
                        manifold.data.normal
                    };
                    normal.y >= required_normal_y
                })
        })
}

fn recover_upright(world: &mut PhysicsWorld, chassis: &Chassis, player: &mut Participant) {
    let Some(pose) = upright_pose(
        world,
        chassis,
        player.tank.parameters.tip_terrain_roll_radians,
    ) else {
        return;
    };
    if let Some(body) = world.bodies.get_mut(chassis.body) {
        body.set_position(pose, true);
        body.set_linvel(Vector::ZERO, true);
        body.set_angvel(Vector::ZERO, true);
        let position = game_vec(body.translation());
        let direction = game_direction(body);
        player
            .tank
            .sync_physics(position, direction, 0.0, 0.0, true);
        player.vehicle.position = position;
        player.vehicle.direction = direction;
    }
}

fn upright_pose(world: &PhysicsWorld, chassis: &Chassis, tip_threshold: f32) -> Option<Pose> {
    let body = world.bodies.get(chassis.body)?;
    let center = body.center_of_mass();
    let filter = QueryFilter::default().exclude_rigid_body(chassis.body);
    let geometry_only = |_: ColliderHandle, collider: &rapier3d::prelude::Collider| {
        collider.is_enabled() && collider.parent().is_none()
    };
    let ray = Ray::new(center, Vector::NEG_Y);
    let (_, hit) = world.cast_ray_and_get_normal(
        &ray,
        chassis.query_radius,
        false,
        filter.predicate(&geometry_only),
    )?;
    let normal = game_vec(hit.normal);
    if normal.y < (tip_threshold * 0.5).cos() {
        return None;
    }
    let (x, z) = game_direction(body).z.sin_cos();
    let forward = Vec3::new(x, -(normal.x * x + normal.z * z) / normal.y, z).normalize();
    let rotation = Quat::from_mat3(&glam::Mat3::from_cols(
        normal.cross(forward),
        normal,
        forward,
    ));
    let rotation = Rotation::from_array(rotation.to_array());
    let mut origin = center - rotation * body.local_center_of_mass();
    origin.y -= (origin - ray.point_at(hit.time_of_impact)).dot(hit.normal) / hit.normal.y;
    let pose = Pose::from_parts(origin, rotation);
    let collider = world.colliders.get(chassis.collider)?;
    let relative = collider.position_wrt_parent()?;
    let clear = world
        .intersect_shape(pose * *relative, collider.shape(), filter)
        .next()
        .is_none();
    clear.then_some(pose)
}

#[cfg(test)]
mod tests;
