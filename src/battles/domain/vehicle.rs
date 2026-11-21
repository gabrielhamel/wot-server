use super::ammunition::{Ammunition, Reload};
use crate::shared_kernel::{ShellEffects, VehiclePerformance};
use glam::{EulerRot, Quat, Vec2, Vec3, Vec3Swizzles};
use std::time::Instant;

const FORWARD: u8 = 1;
const REVERSE: u8 = 2;
const LEFT: u8 = 4;
const RIGHT: u8 = 8;
const HALF_SPEED: u8 = 16;
const QUARTER_SPEED: u8 = 32;
const BRAKE: u8 = 64;

pub(crate) const SHOT_RANGE: f32 = 800.0;
const HULL_RADIUS: f32 = 2.5;

pub(crate) struct Tank {
    pub position: Vec3,
    pub yaw: f32,
    hull_tilt: Vec2,
    pub health: i16,
    pub max_health: i16,
    pub performance: Option<VehiclePerformance>,
    ammunition: Ammunition,
    speed: f32,
    flags: u8,
    aim: Option<Aim>,
    gun_yaw: f32,
    gun_pitch: f32,
}

pub(crate) enum Aim {
    WorldPoint(Vec3),
    HullAngles { yaw: f32, pitch: f32 },
}

pub(crate) enum AmmunitionCommand {
    SelectInitial(i32),
    QueueShell(i32),
    ChangeShell(i32),
    ReloadMagazine,
}

pub(crate) struct Shot {
    pub origin: Vec3,
    pub direction: Vec3,
    pub damage: u16,
    pub effects: ShellEffects,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct GunRay {
    pub origin: Vec3,
    pub direction: Vec3,
}

impl Tank {
    pub fn new(
        position: Vec3,
        direction: Vec3,
        health: i16,
        ammo: Vec<(i32, u16)>,
        performance: Option<VehiclePerformance>,
    ) -> Self {
        let capacity = performance
            .as_ref()
            .map_or(0, |vehicle| vehicle.gun.loading.capacity);
        let mut tank = Self {
            position,
            yaw: direction.z,
            hull_tilt: direction.xy(),
            health,
            performance,
            max_health: health,
            ammunition: Ammunition::new(ammo, capacity),
            speed: 0.0,
            flags: 0,
            aim: None,
            gun_yaw: 0.0,
            gun_pitch: 0.0,
        };
        tank.advance_gun(0.0);
        tank
    }

    pub fn drive(&mut self, flags: u8) -> bool {
        if flags & 0x80 != 0 {
            return false;
        }
        self.flags = flags;
        true
    }

    pub fn set_hull_tilt(&mut self, direction: Vec3) {
        self.hull_tilt = direction.xy();
    }

    pub fn speed(&self) -> f32 {
        self.speed
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
        let Some(performance) = self.performance.as_ref().filter(|_| self.health > 0) else {
            return false;
        };
        let before = (self.position, self.yaw);
        let throttle = axis(self.flags, FORWARD, REVERSE);
        let speed_limit = if throttle < 0.0 {
            performance.reverse_speed
        } else {
            performance.forward_speed
        };
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
        self.yaw =
            angle(self.yaw + axis(self.flags, RIGHT, LEFT) * performance.hull_traverse * seconds);
        let position =
            self.position.xz() + Vec2::new(self.yaw.sin(), self.yaw.cos()) * self.speed * seconds;
        let [min, max] = bounds;
        let position = position.clamp(min, max);
        self.position.x = position.x;
        self.position.z = position.y;
        before != (self.position, self.yaw)
    }

    pub fn advance_gun(&mut self, seconds: f32) -> bool {
        let Some(gun) = self.performance.as_ref().map(|vehicle| &vehicle.gun) else {
            return false;
        };
        let (desired_yaw, desired_pitch) = match self.aim {
            Some(Aim::WorldPoint(target)) => {
                let (pivot, shot_position) = self.gun_mount();
                let target = self.hull_rotation().inverse() * (target - self.position) - pivot;
                let horizontal = target.xz().length();
                let yaw = if horizontal > f32::EPSILON {
                    // At the final yaw, the target and the shot origin share the same lateral coordinate.
                    let offset = (shot_position.x / horizontal).clamp(-1.0, 1.0).asin();
                    angle(target.x.atan2(target.z) - offset)
                } else {
                    self.gun_yaw
                };
                let from_gun = Quat::from_rotation_y(-yaw) * target - shot_position;
                (yaw, -from_gun.y.atan2(from_gun.z.max(0.0)))
            }
            Some(Aim::HullAngles { yaw, pitch }) => (angle(yaw), pitch),
            None => (0.0, 0.0),
        };
        let yaw_step = gun.turret_traverse * seconds;
        let desired_yaw = if let Some([min, max]) = gun.yaw_limits {
            self.gun_yaw = self.gun_yaw.clamp(min, max);
            let nearest = angle(desired_yaw - (min + max) / 2.0) + (min + max) / 2.0;
            nearest.clamp(min, max)
        } else {
            self.gun_yaw + angle(desired_yaw - self.gun_yaw)
        };
        self.gun_yaw = approach(self.gun_yaw, desired_yaw, yaw_step);
        let yaw_pending = yaw_step > 0.0 && self.gun_yaw != desired_yaw;
        if gun.yaw_limits.is_none() {
            self.gun_yaw = angle(self.gun_yaw);
        }

        let revolution = self.gun_yaw.rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU;
        let min = pitch_at(&gun.min_pitch, revolution);
        let max = pitch_at(&gun.max_pitch, revolution).max(min);
        let desired_pitch = desired_pitch.clamp(min, max);
        self.gun_pitch = if gun.gun_traverse > 0.0 {
            approach(self.gun_pitch, desired_pitch, gun.gun_traverse * seconds)
        } else {
            desired_pitch
        }
        .clamp(min, max);

        yaw_pending || self.gun_pitch != desired_pitch
    }

    pub fn packed_gun_angles(&self) -> u16 {
        let yaw = (((self.gun_yaw + std::f32::consts::PI) / std::f32::consts::TAU * 1024.0).round()
            as u16)
            & 1023;
        let pitch = self.performance.as_ref().map_or(0, |vehicle| {
            let min = vehicle
                .gun
                .min_pitch
                .iter()
                .map(|&(_, pitch)| pitch)
                .reduce(f32::min)
                .unwrap_or(0.0);
            let max = vehicle
                .gun
                .max_pitch
                .iter()
                .map(|&(_, pitch)| pitch)
                .reduce(f32::max)
                .unwrap_or(0.0);
            if max > min {
                (((self.gun_pitch - min) / (max - min)).clamp(0.0, 1.0) * 63.0).round() as u16
            } else {
                0
            }
        });
        yaw << 6 | pitch
    }

    pub fn fire(&mut self, now: Instant) -> Result<Shot, &'static str> {
        let Some(performance) = &self.performance else {
            return Err("vehicle_unavailable");
        };
        let shell = self.ammunition.settings()[0];
        let damage = match performance.gun.shell_damage.get(&shell) {
            Some(&damage) => damage,
            None if self.ammunition.loaded(shell) == 0 => 0,
            None => return Err("shell_damage_unavailable"),
        };
        let effects = performance
            .gun
            .shell_effects
            .get(&shell)
            .copied()
            .unwrap_or_default();
        self.ammunition.fire(&performance.gun.loading, now)?;
        let ray = self.gun_ray();
        Ok(Shot {
            origin: ray.origin,
            direction: ray.direction,
            damage,
            effects,
        })
    }

    pub fn gun_ray(&self) -> GunRay {
        let hull = self.hull_rotation();
        let turret = Quat::from_rotation_y(self.gun_yaw);
        let (pivot, shot_position) = self.gun_mount();
        GunRay {
            origin: self.position + hull * (pivot + turret * shot_position),
            direction: hull * turret * Quat::from_rotation_x(self.gun_pitch) * Vec3::Z,
        }
    }

    pub fn base_dispersion_angle(&self) -> Option<f32> {
        self.performance.as_ref()?.gun.base_dispersion_angle
    }

    pub fn rounds(&self) -> impl Iterator<Item = (i32, u16, u16)> + '_ {
        self.ammunition
            .rounds
            .iter()
            .map(|&(shell, total)| (shell, total, self.ammunition.loaded(shell)))
    }

    pub fn shell_settings(&self) -> [i32; 2] {
        self.ammunition.settings()
    }

    pub fn reload_timers(&self) -> [Option<Reload>; 2] {
        self.performance.as_ref().map_or([None; 2], |vehicle| {
            self.ammunition.timers(&vehicle.gun.loading)
        })
    }

    pub fn advance_ammunition(&mut self, now: Instant) -> bool {
        self.performance
            .as_ref()
            .is_some_and(|vehicle| self.ammunition.advance(&vehicle.gun.loading, now))
    }

    pub fn ammunition_command(&mut self, now: Instant, command: AmmunitionCommand) -> &'static str {
        let Some(vehicle) = &self.performance else {
            return "vehicle_unavailable";
        };
        let gun = &vehicle.gun.loading;
        match command {
            AmmunitionCommand::SelectInitial(shell) => self.ammunition.select_initial(gun, shell),
            AmmunitionCommand::QueueShell(shell) => self.ammunition.select(gun, now, shell, false),
            AmmunitionCommand::ChangeShell(shell) => self.ammunition.select(gun, now, shell, true),
            AmmunitionCommand::ReloadMagazine => self.ammunition.manual_reload(gun, now),
        }
    }

    pub fn hit(&mut self, shot: &Shot) -> (i16, i16) {
        let previous = self.health;
        self.health = previous.saturating_sub_unsigned(shot.damage).max(0);
        (previous, self.health)
    }

    fn hull_rotation(&self) -> Quat {
        Quat::from_euler(EulerRot::YXZ, self.yaw, self.hull_tilt.y, self.hull_tilt.x)
    }

    fn gun_mount(&self) -> (Vec3, Vec3) {
        self.performance
            .as_ref()
            .and_then(|profile| profile.turret_pivot.zip(profile.gun.shot_position))
            .unwrap_or((Vec3::Y * 1.35, Vec3::ZERO))
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

fn approach(current: f32, target: f32, step: f32) -> f32 {
    let difference = target - current;
    if difference.abs() <= step {
        target
    } else {
        current + difference.signum() * step
    }
}

fn pitch_at(points: &[(f32, f32)], yaw: f32) -> f32 {
    for pair in points.windows(2) {
        let [(start, first), (end, last)] = pair else {
            continue;
        };
        if yaw <= *end {
            return first + (last - first) * ((yaw - start) / (end - start)).clamp(0.0, 1.0);
        }
    }
    points.last().map_or(0.0, |&(_, pitch)| pitch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared_kernel::{GunLoading, GunPerformance};
    use std::time::Duration;

    fn tank(ammo: Vec<(i32, u16)>) -> Tank {
        Tank::new(
            Vec3::ZERO,
            Vec3::ZERO,
            100,
            ammo,
            Some(VehiclePerformance {
                forward_speed: 12.0,
                reverse_speed: 4.0,
                hull_traverse: 0.8,
                turret_pivot: None,
                gun: GunPerformance {
                    turret_traverse: 0.5,
                    gun_traverse: 0.0,
                    yaw_limits: None,
                    min_pitch: vec![(0.0, -0.4), (1.0, -0.4)],
                    max_pitch: vec![(0.0, 0.2), (1.0, 0.2)],
                    aiming_time: 2.0,
                    base_dispersion_angle: Some(0.005),
                    shot_position: None,
                    shell_damage: [(10, 36), (20, 90)].into(),
                    shell_effects: [
                        (
                            10,
                            ShellEffects {
                                kind: 2,
                                caliber_mm: 37.0,
                                legacy_index: 17,
                                prefab_index: 2,
                            },
                        ),
                        (
                            20,
                            ShellEffects {
                                kind: 1,
                                caliber_mm: 37.0,
                                legacy_index: 19,
                                prefab_index: 1,
                            },
                        ),
                    ]
                    .into(),
                    loading: GunLoading {
                        capacity: 1,
                        reload_time: 3.0,
                        intra_clip_time: 0.0,
                        forced_reload_time: 0.0,
                        autoreload_times: vec![],
                    },
                },
            }),
        )
    }

    #[test]
    fn driving_is_bounded_and_braking_stops_translation() {
        let mut tank = tank(vec![]);
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
        // These are physical turn commands. The client swaps the bits for inverted reverse controls.
        assert!(tank.drive(REVERSE | LEFT));
        assert!(tank.step(0.1, bounds));
        assert!(tank.position.z < 2.0 && tank.yaw < 0.0);
    }

    #[test]
    fn vehicle_reaches_its_forward_and_reverse_speed_limits() {
        let mut tank = tank(vec![]);
        let bounds = [Vec2::splat(-500.0), Vec2::splat(500.0)];
        for (flags, expected) in [
            (FORWARD, 12.0),
            (REVERSE, -4.0),
            (REVERSE | HALF_SPEED, -2.0),
        ] {
            assert!(tank.drive(flags));
            for _ in 0..40 {
                tank.step(0.1, bounds);
            }
            let before = tank.position;
            tank.step(1.0, bounds);
            assert!((tank.position.z - before.z - expected).abs() < 0.001);
        }
    }

    #[test]
    fn gun_traverses_at_its_configured_rate_and_stops_at_the_arc_limit() {
        let mut tank = tank(vec![]);
        tank.performance.as_mut().unwrap().gun.yaw_limits = Some([-0.1, 0.3]);
        assert!(tank.aim_at(Aim::HullAngles {
            yaw: 1.2,
            pitch: -0.9
        }));
        assert!(tank.advance_gun(0.2));
        assert!((tank.gun_yaw - 0.1).abs() < 0.001);
        assert!(!tank.advance_gun(1.0));
        assert_eq!(tank.gun_yaw, 0.3);
        assert_eq!(tank.gun_pitch, -0.4);
        assert_eq!(tank.packed_gun_angles() & 63, 0);
    }

    #[test]
    fn nearby_crosshair_target_stays_locked_with_an_offset_gun_on_a_slope() {
        let mut tank = tank(vec![]);
        let profile = tank.performance.as_mut().unwrap();
        profile.turret_pivot = Some(Vec3::new(0.0, 2.0, -0.6));
        profile.gun.shot_position = Some(Vec3::new(0.4, 0.5, 1.2));
        tank.position = Vec3::new(16.0, 0.63, -10.0);
        tank.yaw = 0.8;
        tank.set_hull_tilt(Vec3::new(0.12, -0.2, 0.8));
        tank.gun_yaw = 0.82;
        tank.gun_pitch = 0.15;
        let expected = tank.gun_ray();
        let target = expected.origin + expected.direction * 3.0;
        tank.gun_yaw = -0.5;
        assert!(tank.aim_at(Aim::WorldPoint(target)));
        for _ in 0..40 {
            tank.advance_gun(0.1);
        }
        let settled = tank.packed_gun_angles();
        assert!(tank.gun_ray().origin.distance(expected.origin) < 0.001);
        assert!(tank.gun_ray().direction.distance(expected.direction) < 0.001);
        for _ in 0..20 {
            tank.advance_gun(0.1);
            assert_eq!(tank.packed_gun_angles(), settled);
        }
    }

    #[test]
    fn shot_uses_server_origin_and_aim_and_consumes_one_round_after_reload() {
        let now = Instant::now();
        let mut shooter = tank(vec![(10, 1), (20, 1)]);
        let mut target = tank(vec![]);
        target.position = Vec3::X * 25.0;
        assert!(!shooter.aim_at(Aim::WorldPoint(Vec3::splat(f32::NAN))));
        assert!(shooter.aim_at(Aim::WorldPoint(target.position + Vec3::Y)));
        shooter.advance_gun(4.0);
        let shot = shooter.fire(now).unwrap();
        assert!(shot.intersection(&target).is_some());
        assert_eq!(shot.damage, 36);
        assert_eq!((shot.effects.kind, shot.effects.legacy_index), (2, 17));
        assert_eq!(target.hit(&shot), (100, 64));
        assert_eq!(shooter.rounds().next(), Some((10, 0, 0)));
        assert_eq!(shooter.shell_settings(), [20, 20]);
        assert_eq!(
            shooter.fire(now + Duration::from_secs(1)).err(),
            Some("reloading")
        );
        let ready = now + Duration::from_secs(3);
        assert!(shooter.advance_ammunition(ready));
        let shot = shooter.fire(ready).unwrap();
        assert_eq!(shot.damage, 90);
        assert_eq!((shot.effects.kind, shot.effects.legacy_index), (1, 19));
        assert_eq!(target.hit(&shot), (64, 0));
        assert_eq!(shooter.fire(ready).err(), Some("empty"));
    }

    #[test]
    fn a_shell_without_damage_data_is_rejected_without_consuming_a_round() {
        let mut shooter = tank(vec![(30, 1)]);
        assert_eq!(
            shooter.fire(Instant::now()).err(),
            Some("shell_damage_unavailable")
        );
        assert_eq!(shooter.rounds().next(), Some((30, 1, 1)));
    }
}
