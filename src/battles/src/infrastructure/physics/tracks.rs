use super::*;
use crate::domain::movement::GroundContact;
use std::f32::consts::TAU;

const CONTACTS_PER_TRACK: usize = 3;
pub(super) const SUPPORT_POINTS: usize = 2 * CONTACTS_PER_TRACK;

pub(super) struct Support {
    pub grounded: bool,
    pub scenery: bool,
}

#[derive(Clone, Copy)]
struct Contact {
    collider: ColliderHandle,
    terrain_normal: Vector,
    position: Vector,
    distance: f32,
    on_scenery: bool,
    side: f32,
}

pub(super) fn drive(
    world: &mut PhysicsWorld,
    scenery: &SceneryColliders,
    chassis: &mut Chassis,
    terrain: &Terrain,
    tank: &mut Tank,
    parameters: &BattleParameters,
) -> Support {
    let mut supported = Support {
        grounded: false,
        scenery: false,
    };
    let Some(body) = world.bodies.get(chassis.body) else {
        return supported;
    };
    if !body.is_dynamic() || !body.is_enabled() {
        return supported;
    }
    let support_lost = body.is_sleeping()
        && chassis.supported_by.iter().flatten().any(|&handle| {
            !world
                .colliders
                .get(handle)
                .is_some_and(|collider| collider.is_enabled())
        });
    if body.is_sleeping() && !tank.is_driving() && !support_lost {
        supported.grounded = chassis.supported_by.iter().any(Option::is_some);
        supported.scenery = chassis
            .supported_by
            .iter()
            .flatten()
            .any(|&h| scenery.contains(h));
        return supported;
    }
    let direction = game_direction(body);
    let rotation = *body.rotation();
    let up = rotation * Vector::Y;
    if up.y <= 0.0 {
        chassis.supported_by.fill(None);
        return supported;
    }
    let travel = parameters.suspension_travel_metres;
    let frequency = TAU * parameters.suspension_frequency_hz;
    let stiffness = frequency * frequency;
    let damping = 2.0 * parameters.suspension_damping_ratio * frequency;
    let reach = travel + (parameters.gravity_mps2 / stiffness).min(travel);
    let geometry_only = |_: ColliderHandle, collider: &rapier3d::prelude::Collider| {
        collider.is_enabled() && collider.parent().is_none()
    };
    let filter = QueryFilter::default().predicate(&geometry_only);
    let mut contacts = [None; SUPPORT_POINTS];
    for (slot, (side, station)) in contacts.iter_mut().zip(
        [-1.0, 1.0]
            .into_iter()
            .flat_map(|side| [-1.0, 0.0, 1.0].map(|station| (side, station))),
    ) {
        let local = Vector::new(
            side * chassis.track_half_width,
            travel,
            station * chassis.track_half_length,
        );
        let origin = body.position().transform_point(local);
        let ray = Ray::new(origin, -up);
        if let Some((collider, hit)) = world.cast_ray_and_get_normal(&ray, reach, false, filter)
            && hit.normal.y > 0.0
            && hit.normal.dot(up) > 0.0
        {
            let on_scenery = scenery.contains(collider);
            *slot = Some(Contact {
                collider,
                terrain_normal: hit.normal,
                position: ray.point_at(hit.time_of_impact),
                distance: hit.time_of_impact,
                on_scenery,
                side,
            });
            supported.grounded = true;
            supported.scenery |= on_scenery;
        }
    }
    chassis.supported_by = contacts.map(|c| c.map(|contact| contact.collider));
    if !supported.grounded {
        if body.is_sleeping()
            && let Some(body) = world.bodies.get_mut(chassis.body)
        {
            body.wake_up(true);
        }
        return supported;
    }
    let forward = rotation * Vector::Z;
    let linear = body.linvel();
    let angular = body.angvel();
    let center = body.center_of_mass();
    let mass = body.mass();
    let ground = if contacts.iter().flatten().all(|c| c.on_scenery) {
        GroundContact {
            surfaces: [4, 0, 0],
            pitch: direction.y,
        }
    } else {
        terrain.ground_contact(
            game_vec(body.translation()).xz(),
            direction.z,
            chassis.track_half_width,
            direction.y,
        )
    };
    let driven = tank.drive_demand(STEP_SECONDS, ground, linear.dot(forward), angular.dot(up));
    let grip = ground.average([
        parameters.hard_surface_grip,
        parameters.medium_surface_grip,
        parameters.soft_surface_grip,
    ]);
    let mass_per_point = mass / SUPPORT_POINTS as f32;
    let Some(body) = world.bodies.get_mut(chassis.body) else {
        return supported;
    };
    if body.is_sleeping() {
        if !support_lost
            && driven.longitudinal_acceleration.abs() + driven.yaw_acceleration.abs() == 0.0
        {
            return supported;
        }
        body.wake_up(true);
    }
    let loads = contacts.map(|contact| {
        contact.map(|contact| {
            let normal = contact.terrain_normal;
            let velocity = linear + angular.cross(contact.position - center);
            let spring =
                parameters.gravity_mps2 * normal.y + stiffness * (travel - contact.distance);
            (contact, (spring - damping * velocity.dot(normal)).max(0.0))
        })
    });
    let average_load =
        loads.iter().flatten().map(|(_, load)| load).sum::<f32>() / SUPPORT_POINTS as f32;
    let drive_reference_load = average_load.max(parameters.gravity_mps2).max(f32::EPSILON);
    let lateral_response = if driven.track_locked {
        parameters.track_lock_heading_response
    } else {
        parameters.lateral_grip_response_seconds.recip()
    };
    let hold_grade = !driven.requests_travel && !driven.track_locked;
    for (contact, normal_acceleration) in loads.into_iter().flatten() {
        let normal = contact.terrain_normal;
        let tangent = (forward - normal * normal.dot(forward)).normalize_or_zero();
        let right = normal.cross(tangent);
        let gravity = Vec2::new(world.gravity.dot(right), world.gravity.dot(tangent));
        let normal_gravity = -world.gravity.dot(normal);
        let load_share = normal_acceleration / average_load.max(normal_gravity).max(f32::EPSILON);
        let cornering = if driven.track_locked {
            0.0
        } else {
            angular.dot(normal) * linear.dot(tangent)
        };
        let lateral = cornering - gravity.x - linear.dot(right) * lateral_response;
        let mut requested_grip = Vec2::new(lateral, 0.0) * load_share;
        let drive_load_share = normal_acceleration / drive_reference_load;
        let steering = -contact.side * driven.yaw_acceleration * chassis.yaw_inertia_per_kg
            / chassis.track_half_width
            * drive_load_share;
        let mut drive_acceleration = driven.longitudinal_acceleration * drive_load_share;
        if hold_grade {
            requested_grip.y -= gravity.y * load_share;
            // Service brakes keep their level-ground response. On a grade, holding
            // against gravity uses part of the same grip that can slow the hull.
            let grade_share =
                (gravity.length() / (grip * normal_gravity).max(f32::EPSILON)).min(1.0);
            requested_grip.y += drive_acceleration * grade_share;
            drive_acceleration *= 1.0 - grade_share;
        }
        let friction = requested_grip.clamp_length_max(grip * normal_acceleration);
        let impulse = (normal * normal_acceleration
            + right * friction.x
            + tangent * (friction.y + drive_acceleration + steering))
            * (STEP_SECONDS * mass_per_point);
        body.apply_impulse_at_point(impulse, contact.position, false);
    }
    supported
}
