use super::*;
use crate::infrastructure::obstacles::{Fragment, HullHit};
use std::f32::consts::{PI, TAU};

const MAX_CONTACTS_PER_STEP: usize = 4;
const MAX_TURN_SAMPLE_RADIANS: f32 = 0.035;
pub(super) const PRESS_REACH: f32 = 0.05;

pub(super) struct SceneryMotion {
    inverse_mass: f32,
    seconds: f32,
    pub(super) broken: Vec<Breakage>,
    attempted: Vec<Fragment>,
    pub(super) speed_scale: f32,
    pub(super) turn_contact: bool,
    pub(super) fall: BreakageCause,
}

impl SceneryMotion {
    pub(super) fn new(inverse_mass: f32, seconds: f32) -> Self {
        Self {
            inverse_mass,
            seconds,
            broken: Vec::new(),
            attempted: Vec::new(),
            speed_scale: 1.0,
            turn_contact: false,
            fall: BreakageCause::Contact {
                yaw: 0.0,
                speed: 0.0,
            },
        }
    }

    pub(super) fn turn(
        &mut self,
        scenery: &mut Scenery,
        mut hull: Hull,
        requested_yaw: f32,
        pressure: f32,
        can_damage: bool,
    ) -> (Hull, Option<Vec2>) {
        let delta = (requested_yaw - hull.yaw + PI).rem_euclid(TAU) - PI;
        if delta.abs() < 1e-6 {
            return (hull, None);
        }
        let from = hull.yaw;
        let samples = ((delta.abs() / MAX_TURN_SAMPLE_RADIANS).ceil() as u8).clamp(1, 32);
        for step in 1..=samples {
            let candidate = Hull {
                yaw: (from + delta * (f32::from(step) / f32::from(samples)) + PI).rem_euclid(TAU)
                    - PI,
                ..hull
            };
            while let Some(hit) = scenery.sweep_hull(candidate, Vec2::ZERO) {
                self.turn_contact = true;
                let normal = hit.contact.normal;
                let closing = candidate.reach(-normal) - hull.reach(-normal);
                if closing <= 0.0 && scenery.sweep_hull(hull, Vec2::ZERO).is_some() {
                    break;
                }
                let speed = (closing * f32::from(samples) / self.seconds).clamp(0.0, 25.0);
                if closing <= 0.0
                    || !can_damage
                    || !self.damage(scenery, hit, speed, pressure, false)
                {
                    return (hull, Some(normal));
                }
            }
            hull = candidate;
        }
        (hull, None)
    }

    pub(super) fn push(
        &mut self,
        scenery: &mut Scenery,
        hull: Hull,
        reachable: Vec2,
        actual_travel: Vec2,
        pressing: Vec2,
    ) {
        if reachable.length_squared() < 1e-8 {
            return;
        }
        while let Some(hit) = scenery.sweep_hull(hull, reachable) {
            let normal = hit.contact.normal;
            let speed = (-actual_travel.dot(normal) / self.seconds).clamp(0.0, 25.0);
            let pressure = -pressing.dot(normal);
            if !self.damage(scenery, hit, speed, pressure.max(0.0), true) {
                break;
            }
        }
    }

    fn damage(
        &mut self,
        scenery: &mut Scenery,
        hit: HullHit,
        speed: f32,
        pressure: f32,
        moving: bool,
    ) -> bool {
        let Some(fragment) = hit.fragment else {
            return false;
        };
        if self.attempted.len() >= MAX_CONTACTS_PER_STEP
            || self.attempted.iter().any(|part| part.same_part(fragment))
            || (speed <= 0.02 && pressure <= 0.0)
        {
            return false;
        }
        self.attempted.push(fragment);

        // The client predicts instantaneous scenery damage from a fraction of kinetic energy.
        let kinetic = 0.000075 * speed * speed / self.inverse_mass;
        let health = scenery.remaining_health(fragment) as f32;
        let broken = scenery.damage(fragment, (kinetic + pressure).clamp(1.0, 250.0) as u32);
        if broken.is_empty() {
            return false;
        }
        self.broken.extend(broken);
        let direction = -hit.contact.normal;
        self.fall = BreakageCause::Contact {
            yaw: direction.x.atan2(direction.y),
            speed,
        };

        if moving && kinetic > 0.0 {
            let energy_share = (health - pressure).max(0.0) / kinetic;
            let retained = (1.0 - 0.35 * energy_share).clamp(0.64, 1.0).sqrt();
            self.speed_scale = (self.speed_scale * retained).max(0.7);
        }
        true
    }
}
