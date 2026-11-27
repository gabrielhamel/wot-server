use crate::infrastructure::obstacles::{Breakage, Fragment, Scenery};
use crate::infrastructure::scenery::BreakageCause;
use glam::Vec2;

const MAX_CONTACTS_PER_STEP: usize = 4;
const SCENERY_BREAK_ENERGY_LOSS: f32 = 0.30;

pub(in crate::infrastructure) struct SceneryMotion {
    inverse_mass: f32,
    pub(in crate::infrastructure) broken: Vec<Breakage>,
    attempted: Vec<Fragment>,
    pub(in crate::infrastructure) speed_scale: f32,
    pub(in crate::infrastructure) fall: BreakageCause,
}

impl SceneryMotion {
    pub(in crate::infrastructure) fn new(inverse_mass: f32) -> Self {
        Self {
            inverse_mass,
            broken: Vec::new(),
            attempted: Vec::new(),
            speed_scale: 1.0,
            fall: BreakageCause::Contact {
                yaw: 0.0,
                speed: 0.0,
            },
        }
    }

    pub(in crate::infrastructure) fn damage_contact(
        &mut self,
        scenery: &mut Scenery,
        fragment: Fragment,
        normal: Vec2,
        speed: f32,
        pressure: f32,
    ) -> bool {
        self.damage_fragment(scenery, fragment, normal, speed, pressure)
    }

    pub(in crate::infrastructure) fn remaining_contact_budget(&self) -> usize {
        MAX_CONTACTS_PER_STEP.saturating_sub(self.attempted.len())
    }

    pub(in crate::infrastructure) fn impact_damage(&self, speed: f32, pressure: f32) -> (f32, u32) {
        // The client predicts instantaneous scenery damage from a fraction of kinetic energy.
        let kinetic = 0.000075 * speed * speed / self.inverse_mass;
        let damage = (kinetic + pressure).clamp(1.0, 250.0) as u32;
        (kinetic, damage)
    }

    fn damage_fragment(
        &mut self,
        scenery: &mut Scenery,
        fragment: Fragment,
        normal: Vec2,
        speed: f32,
        pressure: f32,
    ) -> bool {
        if self.attempted.len() >= MAX_CONTACTS_PER_STEP
            || self.attempted.iter().any(|part| part.same_part(fragment))
            || (speed <= 0.02 && pressure <= 0.0)
        {
            return false;
        }
        self.attempted.push(fragment);

        let (kinetic, damage) = self.impact_damage(speed, pressure);
        let health = scenery.remaining_health(fragment) as f32;
        let broken = scenery.damage(fragment, damage);
        if broken.is_empty() {
            return false;
        }
        self.broken.extend(broken);
        let direction = -normal;
        self.fall = BreakageCause::Contact {
            yaw: direction.x.atan2(direction.y),
            speed,
        };

        if kinetic > 0.0 {
            let energy_share = ((health - pressure) / kinetic).clamp(0.0, 1.0);
            self.speed_scale *= (1.0 - SCENERY_BREAK_ENERGY_LOSS * energy_share).sqrt();
        }
        true
    }
}
