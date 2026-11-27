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
    let normal_load = (loads.iter().flatten().map(|(_, load)| load).sum::<f32>()
        / SUPPORT_POINTS as f32)
        .max(parameters.gravity_mps2)
        .max(f32::EPSILON);
    for (contact, normal_acceleration) in loads.into_iter().flatten() {
        let normal = contact.terrain_normal;
        let tangent = (forward - normal * normal.dot(forward)).normalize_or_zero();
        let right = normal.cross(tangent);
        let lateral_response = if driven.track_locked {
            parameters.track_lock_heading_response
        } else {
            parameters.lateral_grip_response_seconds.recip()
        };
        let lateral_acceleration = (-linear.dot(right) * lateral_response)
            .clamp(-grip * normal_acceleration, grip * normal_acceleration);
        let longitudinal_acceleration = driven.longitudinal_acceleration
            - contact.side * driven.yaw_acceleration * chassis.yaw_inertia_per_kg
                / chassis.track_half_width;
        let impulse = (normal * normal_acceleration
            + tangent * (longitudinal_acceleration * normal_acceleration / normal_load)
            + right * lateral_acceleration)
            * (STEP_SECONDS * mass_per_point);
        body.apply_impulse_at_point(impulse, contact.position, false);
    }
    supported
}
