use super::*;
use crate::infrastructure::obstacles::Fragment;
use rapier3d::parry::query::{NonlinearRigidMotion, ShapeCastHit, ShapeCastOptions};
use rapier3d::prelude::{Aabb, Collider};

pub(super) struct TrackContact {
    handle: ColliderHandle,
    fragment: Fragment,
    normal: Vec2,
    speed: f32,
    pressure: f32,
}

fn apply_fracture_slowdown(body: &mut RigidBody, speed_scale: f32) {
    let velocity = body.linvel();
    body.set_linvel(
        Vector::new(
            velocity.x * speed_scale,
            velocity.y,
            velocity.z * speed_scale,
        ),
        true,
    );
}

fn sweep(
    world: &PhysicsWorld,
    motion: &NonlinearRigidMotion,
    shape: &RoundCuboid,
    stop_at_penetration: bool,
    filter: QueryFilter<'_>,
) -> Option<(ColliderHandle, ShapeCastHit)> {
    if motion.angvel.length_squared() * STEP_SECONDS * STEP_SECONDS <= f32::EPSILON {
        let options = ShapeCastOptions {
            max_time_of_impact: STEP_SECONDS,
            stop_at_penetration,
            ..Default::default()
        };
        return world.cast_shape(&motion.start, motion.linvel, shape, options, filter);
    }
    world.cast_shape_nonlinear(
        motion,
        shape,
        0.0,
        STEP_SECONDS,
        stop_at_penetration,
        filter,
    )
}

impl StepState {
    pub(super) fn break_approaching_scenery(
        &mut self,
        world: &mut PhysicsWorld,
        geometry: &mut SceneryColliders,
        tank: &Chassis,
        scenery: &mut Scenery,
    ) {
        let Some(body) = world.bodies.get(tank.body) else {
            return;
        };
        if !body.is_dynamic() || !body.is_enabled() || body.is_sleeping() {
            return;
        }
        let velocity = body.linvel();
        let angular = body.angvel();
        if velocity == Vector::ZERO
            && angular == Vector::ZERO
            && self.press == Vec2::ZERO
            && self.pivot_press == 0.0
        {
            return;
        }
        let pose = *body.position() * Pose::from_translation(tank.hull_center);
        let mass_center = body.center_of_mass();
        let local_com = pose.inverse_transform_point(mass_center);
        let radius = Vector::splat(
            tank.contact_hull.inner_shape.half_extents.length()
                + tank.contact_hull.border_radius
                + local_com.length(),
        );
        let end = mass_center + velocity * STEP_SECONDS;
        let bounds = Aabb::new(mass_center.min(end) - radius, mass_center.max(end) + radius);
        let breakable = |handle, _: &Collider| geometry.fragment(handle).is_some();
        if world
            .intersect_aabb_conservative(bounds, QueryFilter::default().predicate(&breakable))
            .next()
            .is_none()
        {
            return;
        }
        let Some(hull) = world.colliders.get(tank.collider) else {
            return;
        };
        let Some(cuboid) = hull.shape().as_cuboid() else {
            return;
        };
        let hull_pose = *hull.position();
        // The tracks reach low props while the raised chassis clears its supporting deck.
        // Rounding by the solver's contact skin also gives continuous normals at mesh edges.
        let blocker = RoundCuboid {
            inner_shape: *cuboid,
            border_radius: hull.contact_skin(),
        };
        let mut hull_motion = NonlinearRigidMotion::new(
            hull_pose,
            hull_pose.inverse_transform_point(mass_center),
            velocity,
            angular,
        );
        let mut motion = NonlinearRigidMotion::new(pose, local_com, velocity, angular);
        for _ in 0..self.damage.remaining_contact_budget() {
            let breakable = |handle, collider: &Collider| {
                collider.is_enabled() && geometry.fragment(handle).is_some()
            };
            let Some((handle, hit)) = sweep(
                world,
                &motion,
                &tank.contact_hull,
                true,
                QueryFilter::default().predicate(&breakable),
            ) else {
                return;
            };
            let Some(fragment) = geometry.fragment(handle) else {
                return;
            };
            let solid = |handle, collider: &Collider| {
                collider.is_enabled() && geometry.fragment(handle).is_none()
            };
            let filter = QueryFilter::default()
                .exclude_rigid_body(tank.body)
                .predicate(&solid);
            if sweep(world, &hull_motion, &blocker, false, filter)
                .is_some_and(|(_, b)| b.time_of_impact <= hit.time_of_impact)
            {
                return;
            }
            let normal = game_vec(hit.normal1).xz().normalize_or_zero();
            let contact_velocity = motion.linvel + angular.cross(hit.witness1 - mass_center);
            let speed = (-contact_velocity.dot(hit.normal1)).clamp(0.0, 25.0);
            let press = (-self.press.dot(normal)).max(self.pivot_press).max(0.0);
            if self.damage.impact_damage(speed, press).1 < scenery.remaining_health(fragment) {
                // Let an actual hull or peer push supply its full impulse before charging partial damage.
                self.track_contact = Some(TrackContact {
                    handle,
                    fragment,
                    normal,
                    speed,
                    pressure: press,
                });
                return;
            }
            let before = self.damage.speed_scale;
            if !self
                .damage
                .damage_contact(scenery, fragment, normal, speed, press)
            {
                return;
            }
            self.motion.scenery_contact = true;
            geometry.sync(scenery, &mut world.colliders);
            if geometry.contains(handle) {
                return;
            }
            if let Some(body) = world.bodies.get_mut(tank.body) {
                apply_fracture_slowdown(body, self.damage.speed_scale / before);
                motion.linvel = body.linvel();
                hull_motion.linvel = body.linvel();
            }
        }
    }

    pub(super) fn contacts(
        &mut self,
        world: &mut PhysicsWorld,
        geometry: &mut SceneryColliders,
        tank: &Chassis,
        scenery: &mut Scenery,
        pressure_per_impulse: f32,
        can_damage: bool,
    ) {
        let mut broken_contacts = Vec::new();
        let received_push = world
            .narrow_phase
            .contact_pairs_with(tank.collider)
            .filter(|pair| pair.has_any_active_contact())
            .filter_map(|pair| {
                let first = pair.collider1 == tank.collider;
                let other = if first {
                    pair.collider2
                } else {
                    pair.collider1
                };
                world.colliders.get(other)?.parent()?;
                Some(if first {
                    -pair.total_impulse()
                } else {
                    pair.total_impulse()
                })
            })
            .sum::<Vector>();
        for pair in world
            .narrow_phase
            .contact_pairs_with(tank.collider)
            .filter(|pair| pair.has_any_active_contact())
        {
            let first = pair.collider1 == tank.collider;
            let other = if first {
                pair.collider2
            } else {
                pair.collider1
            };
            let Some(collider) = world.colliders.get(other) else {
                continue;
            };
            if let Some(id) = collider
                .user_data
                .checked_sub(1)
                .and_then(|id| u64::try_from(id).ok())
                .filter(|_| collider.parent().is_some())
            {
                let id = ParticipantId(id);
                if !self.motion.contact_peers.contains(&id) {
                    self.motion.contact_peers.push(id);
                }
                continue;
            }
            if !geometry.contains(other) {
                continue;
            }
            self.motion.scenery_contact = true;
            if !can_damage {
                continue;
            }
            let Some(fragment) = geometry.fragment(other) else {
                continue;
            };
            let Some((manifold, contact)) = pair.find_deepest_contact() else {
                continue;
            };
            let incoming_normal = if first {
                -manifold.data.normal
            } else {
                manifold.data.normal
            };
            let local = if first {
                contact.local_p1
            } else {
                contact.local_p2
            };
            let Some(hull) = world.colliders.get(tank.collider) else {
                continue;
            };
            let point = hull.position().transform_point(local);
            let velocity = self.velocity + self.angular_velocity.cross(point - self.mass_center);
            let speed = (-velocity.dot(incoming_normal)).clamp(0.0, 25.0);
            let normal = game_vec(incoming_normal).xz().normalize_or_zero();
            let press = (-self.press.dot(normal))
                .max(self.pivot_press)
                .max(-game_vec(received_push).xz().dot(normal) * pressure_per_impulse)
                .max(0.0);
            let before = self.damage.speed_scale;
            if self
                .damage
                .damage_contact(scenery, fragment, normal, speed, press)
            {
                let mut impulse = Vector::ZERO;
                let mut torque = Vector::ZERO;
                for manifold in pair.solver_manifolds() {
                    let normal = if first {
                        manifold.data.normal
                    } else {
                        -manifold.data.normal
                    };
                    for point in &manifold.points {
                        let reaction = normal * point.data.impulse;
                        let arm = if first {
                            point.data.solver_dp1
                        } else {
                            point.data.solver_dp2
                        };
                        impulse += reaction;
                        torque += arm.cross(reaction);
                    }
                }
                broken_contacts.push((other, impulse, torque, self.damage.speed_scale / before));
            }
        }
        if let Some(hit) = self.track_contact.take() {
            let before = self.damage.speed_scale;
            if self.damage.damage_contact(
                scenery,
                hit.fragment,
                hit.normal,
                hit.speed,
                hit.pressure,
            ) {
                broken_contacts.push((
                    hit.handle,
                    Vector::ZERO,
                    Vector::ZERO,
                    self.damage.speed_scale / before,
                ));
            }
        }
        if !broken_contacts.is_empty() {
            geometry.sync(scenery, &mut world.colliders);
            for (other, impulse, torque, speed_scale) in broken_contacts {
                if !geometry.contains(other)
                    && let Some(body) = world.bodies.get_mut(tank.body)
                {
                    // Contacts from another vehicle can only supply their pushing force after the solve.
                    body.apply_impulse(impulse, true);
                    body.apply_torque_impulse(torque, true);
                    apply_fracture_slowdown(body, speed_scale);
                }
            }
        }
    }
}
