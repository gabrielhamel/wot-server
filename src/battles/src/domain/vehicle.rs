use super::{
    ammunition::{Ammunition, Reload},
    movement::{Drive, GRAVITY, GroundContact, approach},
    world::{Hull, WorldMotion},
};
use glam::{EulerRot, Quat, Vec2, Vec3, Vec3Swizzles};
use std::time::Instant;
use wot_shared_kernel::{ShellEffects, VehiclePerformance};

const HULL_RADIUS: f32 = 2.5;
const TERRAIN_CONTACT_TOLERANCE_METRES: f32 = 0.35;

pub(crate) struct Tank {
    pub position: Vec3,
    pub yaw: f32,
    hull_tilt: Vec2,
    vertical_speed: Option<f32>,
    world: WorldMotion,
    pub health: i16,
    pub max_health: i16,
    pub performance: Option<VehiclePerformance>,
    ammunition: Ammunition,
    drive: Drive,
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
    pub max_distance: f32,
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
            vertical_speed: None,
            world: WorldMotion::default(),
            health,
            performance,
            max_health: health,
            ammunition: Ammunition::new(ammo, capacity),
            drive: Drive::default(),
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
        self.drive.flags = flags;
        true
    }

    pub fn set_hull_tilt(&mut self, direction: Vec3) {
        self.hull_tilt = direction.xy();
    }

    pub fn follow_terrain(
        &mut self,
        ground_position: Vec3,
        mut direction: Vec3,
        seconds: f32,
    ) -> Vec3 {
        let vertical_speed = if self.world.is_tipped() {
            None
        } else {
            self.vertical_speed.or_else(|| {
                (self.position.y - ground_position.y > TERRAIN_CONTACT_TOLERANCE_METRES)
                    .then(|| -self.drive.speed * self.hull_tilt.y.sin())
            })
        };
        if let Some(speed) = vertical_speed {
            let next_height = self.position.y + (speed - 0.5 * GRAVITY * seconds) * seconds;
            if next_height > ground_position.y {
                self.vertical_speed = Some(speed - GRAVITY * seconds);
                self.position.y = next_height;
                direction.x = self.hull_tilt.x;
                direction.y = self.hull_tilt.y;
                return direction;
            }
        }
        self.vertical_speed = None;
        self.position = ground_position;
        self.set_hull_tilt(direction);
        direction
    }

    pub fn collision_hull(&self) -> Hull {
        let half_size = self.performance.as_ref().map_or(Vec2::new(1.25, 2.5), |p| {
            Vec2::new(p.mobility.track_half_width, p.mobility.track_half_length)
        });
        let mut position = self.position;
        if self.world.is_tipped() {
            position.y -= (half_size.x - 0.35).max(0.0);
        }
        Hull {
            position,
            yaw: self.yaw,
            half_size,
        }
    }

    pub fn collision_inverse_mass(&self) -> f32 {
        if self.health <= 0 {
            0.0
        } else {
            self.performance
                .as_ref()
                .map_or(1.0 / 30_000.0, |p| 1.0 / p.mobility.mass_kg.max(1.0))
        }
    }

    pub fn tree_push_load(&self, seconds: f32) -> f32 {
        if !self.drive.has_throttle() || self.vertical_speed.is_some() || self.is_tipped() {
            return 0.0;
        }
        let power = self.performance.as_ref().map_or(300_000.0, |profile| {
            profile.mobility.mass_kg * profile.mobility.power_per_kg
        });
        power * (1.13 / 2.0) * 0.0002 * seconds
    }

    pub fn passive_travel(
        &mut self,
        tilt: Vec2,
        ground: GroundContact,
        seconds: f32,
    ) -> (Vec2, bool) {
        if self.vertical_speed.is_some() {
            return (Vec2::ZERO, false);
        }
        let grip = ground.average([0.58, 0.50, 0.42]);
        self.world.advance(self.yaw, tilt, grip, seconds)
    }

    pub fn world_contact(&mut self, normal: Vec2) {
        self.world.block(normal);
        let forward = Vec2::new(self.yaw.sin(), self.yaw.cos());
        let into_surface = forward.dot(normal) * self.drive.speed.signum();
        self.drive.speed *= 1.0 - into_surface.min(0.0).powi(2);
    }

    pub fn world_impulse(&mut self, impulse: Vec2) {
        self.world.push(impulse);
    }

    pub fn hold_world_position(&mut self) {
        self.drive.stop();
        self.world.stop();
    }

    pub fn is_tipped(&self) -> bool {
        self.world.is_tipped()
    }

    pub fn settle_hull(
        &mut self,
        mut direction: Vec3,
        impulse: Vec2,
        stationary: bool,
        seconds: f32,
    ) -> Vec3 {
        if self.vertical_speed.is_some() {
            return direction;
        }
        let right = Vec2::new(self.yaw.cos(), -self.yaw.sin());
        let side_speed = impulse.dot(right) - 0.28 * self.drive.speed * self.drive.yaw_rate;
        direction.x = self
            .world
            .attitude(direction.x, side_speed, stationary, seconds);
        if self.world.is_tipped() {
            self.drive.stop();
            self.position.y += (self.collision_hull().half_size.x - 0.35).max(0.0);
        }
        self.set_hull_tilt(direction);
        direction
    }

    pub fn track_speeds(&self) -> [f32; 2] {
        let half_width = self
            .performance
            .as_ref()
            .map_or(1.25, |profile| profile.mobility.track_half_width);
        self.drive.track_speeds(half_width)
    }

    pub fn engine_mode(&self) -> [u8; 2] {
        self.drive.engine_mode()
    }

    pub fn rpm(&self) -> f32 {
        self.performance
            .as_ref()
            .map_or(0.2, |profile| self.drive.rpm(profile))
    }

    pub fn wheel_motion(&self) -> impl Iterator<Item = (f32, Option<f32>)> {
        self.performance
            .iter()
            .flat_map(|profile| self.drive.wheel_motion(profile))
    }

    pub fn is_driving(&self) -> bool {
        self.drive.is_active() || self.vertical_speed.is_some()
    }

    pub fn stop_motion(&mut self) {
        self.drive.stop();
        self.vertical_speed = None;
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

    #[tracing::instrument(name = "battles.drive_vehicle", skip_all, fields(
        ddd.bounded_context.name = "battles", input.drive.flags = self.drive.flags,
        input.ground.hard_contacts = ground.surfaces[0], input.ground.medium_contacts = ground.surfaces[1],
        input.ground.soft_contacts = ground.surfaces[2], input.ground.pitch_radians = ground.pitch,
        output.drive.speed_mps = tracing::field::Empty, output.drive.yaw_rate_rad_per_second = tracing::field::Empty,
        output.drive.track_locked = tracing::field::Empty,
        input.drive.airborne = self.vertical_speed.is_some(),
    ))]
    pub fn step(&mut self, seconds: f32, bounds: [Vec2; 2], ground: GroundContact) -> bool {
        let Some(performance) = self
            .performance
            .as_ref()
            .filter(|_| self.health > 0 && !self.is_tipped())
        else {
            return false;
        };
        let before = (self.position, self.yaw);
        if self.vertical_speed.is_some() {
            self.drive
                .coast_in_air(&mut self.position, self.yaw, seconds, bounds);
        } else {
            self.drive.step(
                performance,
                ground,
                &mut self.position,
                &mut self.yaw,
                seconds,
                bounds,
            );
        }
        let span = tracing::Span::current();
        span.record("output.drive.speed_mps", self.drive.speed);
        span.record("output.drive.yaw_rate_rad_per_second", self.drive.yaw_rate);
        span.record(
            "output.drive.track_locked",
            self.vertical_speed.is_none() && self.drive.is_track_locked(),
        );
        self.vertical_speed.is_some() || before != (self.position, self.yaw)
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
        if self.is_tipped() {
            return Err("vehicle_overturned");
        }
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
        let max_distance = match performance.gun.shell_ranges.get(&shell) {
            Some(&distance) => distance,
            None if self.ammunition.loaded(shell) == 0 => 0.0,
            None => return Err("shell_range_unavailable"),
        };
        self.ammunition.fire(&performance.gun.loading, now)?;
        let ray = self.gun_ray();
        Ok(Shot {
            origin: ray.origin,
            direction: ray.direction,
            damage,
            max_distance,
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
        if self.health == 0 {
            self.drive.flags = 0;
        }
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
    pub const SPEED_MPS: f32 = 450.0;

    pub fn intersection(&self, target: &Tank) -> Option<f32> {
        let offset = target.position + Vec3::Y * 1.0 - self.origin;
        let projected = offset.dot(self.direction);
        let perpendicular = (offset - self.direction * projected).length_squared();
        if perpendicular > HULL_RADIUS * HULL_RADIUS {
            return None;
        }
        let distance = projected - (HULL_RADIUS * HULL_RADIUS - perpendicular).sqrt();
        (0.0..=self.max_distance)
            .contains(&distance)
            .then_some(distance)
    }
}

pub(super) fn angle(radians: f32) -> f32 {
    (radians + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
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
    use std::time::Duration;
    use wot_shared_kernel::{GunLoading, GunPerformance};

    const FORWARD: u8 = 1;
    const REVERSE: u8 = 2;
    const LEFT: u8 = 4;
    const HALF_SPEED: u8 = 16;
    const BRAKE: u8 = 64;
    const HARD: GroundContact = GroundContact {
        surfaces: [4, 0, 0],
        pitch: 0.0,
    };

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
                mobility: Default::default(),
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
                    shell_ranges: [(10, 720.0), (20, 600.0)].into_iter().collect(),
                    shell_effects: [
                        (
                            10,
                            ShellEffects {
                                kind: 2,
                                caliber_mm: 37.0,
                                legacy_index: 17,
                                prefab_index: 2,
                                damage_log_type: Some(1),
                                premium: false,
                                pierced_sticker: None,
                            },
                        ),
                        (
                            20,
                            ShellEffects {
                                kind: 1,
                                caliber_mm: 37.0,
                                legacy_index: 19,
                                prefab_index: 1,
                                damage_log_type: Some(7),
                                premium: false,
                                pierced_sticker: None,
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
            tank.step(0.1, bounds, HARD);
        }
        assert_eq!(tank.position, Vec3::Z * 2.0);
        assert!(tank.drive(BRAKE));
        for _ in 0..10 {
            tank.step(0.1, bounds, HARD);
        }
        assert!(!tank.step(0.1, bounds, HARD));
        assert!(tank.drive(REVERSE | LEFT));
        assert!(tank.step(0.1, bounds, HARD));
        assert!(tank.position.z < 2.0 && tank.yaw > 0.0);
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
            for _ in 0..100 {
                tank.step(0.1, bounds, HARD);
            }
            let before = tank.position;
            tank.step(1.0, bounds, HARD);
            assert!((tank.position.z - before.z - expected).abs() < 0.001);
        }
    }

    #[test]
    fn acceleration_depends_on_surface_and_slope_and_releases_with_inertia() {
        let bounds = [Vec2::splat(-100.0), Vec2::splat(100.0)];
        let mut hard = tank(vec![]);
        let mut soft = tank(vec![]);
        let mut downhill = tank(vec![]);
        let soft_ground = GroundContact {
            surfaces: [0, 0, 4],
            pitch: 0.0,
        };
        let grade = GroundContact { pitch: 0.2, ..HARD };
        for (vehicle, ground) in [
            (&mut hard, HARD),
            (&mut soft, soft_ground),
            (&mut downhill, grade),
        ] {
            assert!(vehicle.drive(FORWARD));
            for _ in 0..10 {
                vehicle.step(0.1, bounds, ground);
            }
        }
        assert!(downhill.drive.speed > hard.drive.speed && hard.drive.speed > soft.drive.speed);
        let speed = hard.drive.speed;
        assert!(hard.drive(REVERSE));
        hard.step(0.1, bounds, HARD);
        assert!((0.0..speed).contains(&hard.drive.speed));
        hard.drive(0);
        let before = hard.position.z;
        hard.step(0.1, bounds, HARD);
        assert!(hard.position.z > before);
        for _ in 0..10 {
            hard.step(0.1, bounds, HARD);
        }
        assert_eq!(hard.track_speeds(), [0.0; 2]);
    }

    #[test]
    fn a_pivot_rolls_tracks_in_opposite_directions_and_wheels_follow_a_turning_arc() {
        let bounds = [Vec2::splat(-100.0), Vec2::splat(100.0)];
        let mut tracked = tank(vec![]);
        tracked.drive(LEFT);
        assert!(tracked.step(0.1, bounds, HARD));
        let [left, right] = tracked.track_speeds();
        assert!(left < 0.0 && right > 0.0);
        assert_eq!(tracked.position, Vec3::ZERO);

        let mut wheeled = tank(vec![]);
        let mobility = &mut wheeled.performance.as_mut().unwrap().mobility;
        mobility.turns_in_place = false;
        mobility.wheels = [-1.0, 1.0]
            .map(|x| wot_shared_kernel::Wheel {
                center: Vec2::new(x, 2.0),
                steering_limit: Some(0.4),
            })
            .to_vec();
        wheeled.drive(FORWARD | LEFT);
        for _ in 0..5 {
            wheeled.step(0.1, bounds, HARD);
        }
        let wheels: Vec<_> = wheeled.wheel_motion().collect();
        let [(inner, inner_angle), (outer, outer_angle)] = wheels.as_slice() else {
            panic!("expected two wheels");
        };
        assert!(*outer > *inner && *inner > 0.0);
        assert!(inner_angle.unwrap() < outer_angle.unwrap() && outer_angle.unwrap() < 0.0);
    }

    #[test]
    fn tracked_chassis_pivots_about_one_track_when_center_rotation_is_disabled() {
        let mut tank = tank(vec![]);
        tank.performance
            .as_mut()
            .unwrap()
            .mobility
            .rotation_around_center = false;
        tank.drive(LEFT);
        assert!(tank.step(0.1, [Vec2::splat(-100.0), Vec2::splat(100.0)], HARD));
        let [left, right] = tank.track_speeds();
        assert_eq!(left, 0.0);
        assert!(right > 0.0 && tank.position.z > 0.0);
    }

    #[test]
    fn downhill_speed_and_track_lock_depend_on_the_ground_and_chassis() {
        let bounds = [Vec2::splat(-1000.0), Vec2::splat(1000.0)];
        let mut tracked = tank(vec![]);
        tracked.performance.as_mut().unwrap().mobility.track_lock = true;
        let mut fixed = tank(vec![]);
        let slope = GroundContact { pitch: 0.2, ..HARD };
        for tank in [&mut tracked, &mut fixed] {
            tank.drive(FORWARD);
            for _ in 0..160 {
                tank.step(0.1, bounds, slope);
            }
            assert!(tank.drive.speed > 12.0 && tank.drive.speed <= 18.0);
            tank.drive(BRAKE | LEFT);
            tank.step(0.1, bounds, HARD);
        }
        assert!(tracked.drive.is_track_locked());
        assert!(!fixed.drive.is_track_locked());
        assert!(tracked.drive.speed > fixed.drive.speed);
        assert!(tracked.yaw.abs() > fixed.yaw.abs());
    }

    #[test]
    fn steep_terrain_blocks_drive_power_above_the_chassis_climb_angle() {
        let bounds = [Vec2::splat(-100.0), Vec2::splat(100.0)];
        let uphill = GroundContact {
            pitch: -0.25,
            ..HARD
        };
        let mut limited = tank(vec![]);
        limited
            .performance
            .as_mut()
            .unwrap()
            .mobility
            .max_climb_angle = Some(0.2);
        let mut capable = tank(vec![]);
        capable
            .performance
            .as_mut()
            .unwrap()
            .mobility
            .max_climb_angle = Some(0.3);
        limited.drive(FORWARD);
        capable.drive(FORWARD);
        assert!(!limited.step(0.1, bounds, uphill));
        assert!(capable.step(0.1, bounds, uphill));
    }

    #[test]
    fn leaving_a_drop_preserves_height_until_gravity_brings_the_hull_to_ground() {
        let mut tank = tank(vec![]);
        tank.position.y = 4.0;
        let ground_direction = Vec3::new(0.1, 0.1, 0.0);
        assert_eq!(
            tank.follow_terrain(Vec3::ZERO, ground_direction, 0.1),
            Vec3::ZERO
        );
        assert!((3.9..4.0).contains(&tank.position.y));
        let bounds = [Vec2::splat(-100.0), Vec2::splat(100.0)];
        while tank.is_driving() {
            assert!(tank.step(0.1, bounds, HARD));
            tank.follow_terrain(Vec3::ZERO, ground_direction, 0.1);
        }
        assert_eq!(tank.position.y, 0.0);
        assert_eq!(tank.hull_tilt, ground_direction.xy());
    }

    #[test]
    fn overturned_tank_keeps_its_health_and_can_drive_after_righting() {
        let mut tank = tank(vec![]);
        let tipped = tank.settle_hull(Vec3::ZERO, Vec2::X * 7.0, false, 0.1);
        assert!(tank.is_tipped());
        assert_eq!(tipped.x, -std::f32::consts::FRAC_PI_2);
        assert_eq!(tank.health, tank.max_health);
        assert_eq!(tank.fire(Instant::now()).err(), Some("vehicle_overturned"));
        let bounds = [Vec2::splat(-100.0), Vec2::splat(100.0)];
        tank.drive(FORWARD);
        assert!(!tank.step(0.1, bounds, HARD));
        let ground = tank.follow_terrain(Vec3::ZERO, Vec3::ZERO, 0.1);
        assert_eq!(
            tank.settle_hull(ground, -Vec2::X * 2.0, false, 0.1),
            Vec3::ZERO
        );
        assert!(!tank.is_tipped());
        assert_eq!(tank.health, tank.max_health);
        assert!(tank.step(0.1, bounds, HARD));
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
        assert_eq!(shot.max_distance, 720.0);
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
        assert_eq!(shot.max_distance, 600.0);
        let mut distant = tank(vec![]);
        distant.position = shot.origin + shot.direction * 650.0 - Vec3::Y;
        assert_eq!(shot.intersection(&distant), None);
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
