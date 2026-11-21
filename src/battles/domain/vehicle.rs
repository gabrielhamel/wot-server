use glam::{Vec2, Vec3};
use std::time::{Duration, Instant};

const FORWARD: u8 = 1;
const REVERSE: u8 = 2;
const LEFT: u8 = 4;
const RIGHT: u8 = 8;
const HALF_SPEED: u8 = 16;
const QUARTER_SPEED: u8 = 32;
const BRAKE: u8 = 64;

pub(crate) const RELOAD: Duration = Duration::from_secs(3);
pub(crate) const SHOT_RANGE: f32 = 800.0;
const DAMAGE: i16 = 75;
const HULL_RADIUS: f32 = 2.5;

pub(crate) struct Tank {
    pub position: Vec3,
    pub yaw: f32,
    pub health: i16,
    pub ammo: Vec<(i32, u16)>,
    pub loaded_shell: usize,
    speed: f32,
    flags: u8,
    aim: Option<Aim>,
    next_shot: Option<Instant>,
}

pub(crate) enum Aim {
    WorldPoint(Vec3),
    HullAngles { yaw: f32, pitch: f32 },
}

pub(crate) struct Shot {
    pub origin: Vec3,
    pub direction: Vec3,
}

impl Tank {
    pub fn new(position: Vec3, yaw: f32, health: i16, ammo: Vec<(i32, u16)>) -> Self {
        let loaded_shell = ammo.iter().position(|&(_, count)| count > 0).unwrap_or(0);
        Self {
            position,
            yaw,
            health,
            ammo,
            loaded_shell,
            speed: 0.0,
            flags: 0,
            aim: None,
            next_shot: None,
        }
    }

    pub fn drive(&mut self, flags: u8) -> bool {
        if flags & 0x80 != 0 {
            return false;
        }
        self.flags = flags;
        true
    }

    pub fn aim_at(&mut self, aim: Aim) -> bool {
        let valid = match aim {
            Aim::WorldPoint(point) => point.is_finite() && point.abs().max_element() <= 100_000.0,
            Aim::HullAngles { yaw, pitch } => {
                yaw.is_finite()
                    && pitch.is_finite()
                    && yaw.abs() <= 4.0 * std::f32::consts::PI
                    && pitch.abs() <= std::f32::consts::FRAC_PI_2
            }
        };
        if valid {
            self.aim = Some(aim);
        }
        valid
    }

    pub fn step(&mut self, seconds: f32, bounds: [Vec2; 2]) -> bool {
        if self.health <= 0 {
            return false;
        }
        let before = (self.position, self.yaw);
        let throttle = axis(self.flags, FORWARD, REVERSE);
        let speed_limit = if throttle < 0.0 { 5.0 } else { 14.0 };
        let gear = if self.flags & QUARTER_SPEED != 0 {
            0.25
        } else if self.flags & HALF_SPEED != 0 {
            0.5
        } else {
            1.0
        };
        let braking = self.flags & BRAKE != 0;
        let desired = if braking {
            0.0
        } else {
            throttle * speed_limit * gear
        };
        let speed_step = if braking { 24.0 } else { 6.0 } * seconds;
        self.speed += (desired - self.speed).clamp(-speed_step, speed_step);
        self.yaw = angle(self.yaw + axis(self.flags, RIGHT, LEFT) * 0.85 * seconds);
        let position =
            self.position.xz() + Vec2::new(self.yaw.sin(), self.yaw.cos()) * self.speed * seconds;
        let [min, max] = bounds;
        let position = position.clamp(min, max);
        self.position.x = position.x;
        self.position.z = position.y;
        before != (self.position, self.yaw)
    }

    pub fn gun_angles(&self) -> (f32, f32) {
        match self.aim {
            Some(Aim::WorldPoint(target)) => {
                let delta = target - self.muzzle();
                (
                    angle(delta.x.atan2(delta.z) - self.yaw),
                    -delta.y.atan2(delta.xz().length()),
                )
            }
            Some(Aim::HullAngles { yaw, pitch }) => (angle(yaw), pitch),
            None => (0.0, 0.0),
        }
    }

    pub fn fire(&mut self, now: Instant) -> Result<Shot, &'static str> {
        if self.next_shot.is_some_and(|ready| ready > now) {
            return Err("reloading");
        }
        let Some((_, count)) = self
            .ammo
            .get_mut(self.loaded_shell)
            .filter(|(_, count)| *count > 0)
        else {
            return Err("empty");
        };
        *count -= 1;
        self.next_shot = Some(now + RELOAD);
        let (turret, pitch) = self.gun_angles();
        let yaw = self.yaw + turret;
        let direction = Vec3::new(
            yaw.sin() * pitch.cos(),
            -pitch.sin(),
            yaw.cos() * pitch.cos(),
        );
        Ok(Shot {
            origin: self.muzzle(),
            direction,
        })
    }

    pub fn reload_remaining(&self, now: Instant) -> f32 {
        self.next_shot.map_or(0.0, |deadline| {
            deadline.saturating_duration_since(now).as_secs_f32()
        })
    }

    pub fn select_shell(&mut self, descriptor: i32) -> bool {
        let Some(index) = self
            .ammo
            .iter()
            .position(|&(shell, count)| shell == descriptor && count > 0)
        else {
            return false;
        };
        self.loaded_shell = index;
        true
    }

    pub fn hit(&mut self) -> (i16, i16) {
        let previous = self.health;
        self.health = previous.saturating_sub(DAMAGE).max(0);
        (previous, self.health)
    }

    fn muzzle(&self) -> Vec3 {
        self.position + Vec3::Y * 1.35
    }
}

impl Shot {
    pub fn intersection(&self, target: &Tank) -> Option<f32> {
        let offset = target.position + Vec3::Y * 1.0 - self.origin;
        let projected = offset.dot(self.direction);
        let perpendicular = (offset - self.direction * projected).length_squared();
        if perpendicular > HULL_RADIUS * HULL_RADIUS {
            return None;
        }
        let distance = projected - (HULL_RADIUS * HULL_RADIUS - perpendicular).sqrt();
        (0.0..=SHOT_RANGE).contains(&distance).then_some(distance)
    }
}

fn axis(flags: u8, positive: u8, negative: u8) -> f32 {
    f32::from(u8::from(flags & positive != 0)) - f32::from(u8::from(flags & negative != 0))
}

fn angle(radians: f32) -> f32 {
    (radians + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

use glam::Vec3Swizzles;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driving_is_bounded_and_braking_stops_translation() {
        let mut tank = Tank::new(Vec3::ZERO, 0.0, 100, vec![]);
        let bounds = [Vec2::splat(-2.0), Vec2::splat(2.0)];
        assert!(!tank.drive(0x80));
        assert!(tank.drive(FORWARD));
        for _ in 0..50 {
            tank.step(0.1, bounds);
        }
        assert_eq!(tank.position, Vec3::Z * 2.0);
        assert!(tank.drive(BRAKE));
        for _ in 0..10 {
            tank.step(0.1, bounds);
        }
        assert!(!tank.step(0.1, bounds));
        assert!(tank.drive(REVERSE | LEFT));
        assert!(tank.step(0.1, bounds));
        assert!(tank.position.z < 2.0 && tank.yaw < 0.0);
    }

    #[test]
    fn shot_uses_server_origin_and_aim_and_consumes_one_round_after_reload() {
        let now = Instant::now();
        let mut shooter = Tank::new(Vec3::ZERO, 0.0, 100, vec![(10, 2)]);
        let mut target = Tank::new(Vec3::X * 25.0, 0.0, 100, vec![]);
        assert!(!shooter.aim_at(Aim::WorldPoint(Vec3::splat(f32::NAN))));
        assert!(shooter.aim_at(Aim::WorldPoint(target.position + Vec3::Y)));
        let shot = shooter.fire(now).unwrap();
        assert!(shot.intersection(&target).is_some());
        assert_eq!(target.hit(), (100, 25));
        assert_eq!(shooter.ammo[0].1, 1);
        assert_eq!(
            shooter.fire(now + Duration::from_secs(1)).err(),
            Some("reloading")
        );
        assert!(shooter.fire(now + RELOAD).is_ok());
        assert_eq!(target.hit(), (25, 0));
        assert_eq!(shooter.fire(now + RELOAD * 2).err(), Some("empty"));
    }
}
