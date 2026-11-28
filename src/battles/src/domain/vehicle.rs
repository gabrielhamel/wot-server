use super::{
    ammunition::{Ammunition, Reload},
    armor::roll,
    ballistics::elevation,
    devices::{Condition, Criticals, DeviceStrike},
    movement::{Drive, DriveDemand, GroundContact, approach},
    parameters::{BattleParameters, MAX_VEHICLE_MASS_KG, MIN_VEHICLE_MASS_KG},
    spotting::{Combatant, SightGeometry, local_checkpoints},
};
use glam::{EulerRot, Quat, Vec2, Vec3, Vec3Swizzles};
use std::time::Instant;
use wot_shared_kernel::{ShellBallistics, ShellEffects, VehiclePerformance};

pub(crate) struct Tank {
    pub position: Vec3,
    pub yaw: f32,
    hull_tilt: Vec2,
    physics_grounded: bool,
    physics_tipped: bool,
    physics_settled_seconds: f64,
    pub health: i16,
    pub max_health: i16,
    pub performance: Option<VehiclePerformance>,
    ammunition: Ammunition,
    drive: Drive,
    aim: Option<Aim>,
    gun_yaw: f32,
    gun_pitch: f32,
    stun_traverse: Option<f32>,
    last_shot: Option<Instant>,
    pub(crate) condition: Condition,
    pub(crate) parameters: BattleParameters,
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

#[derive(Clone, Copy)]
pub(crate) struct Shot {
    pub origin: Vec3,
    pub direction: Vec3,
    pub damage: u16,
    pub max_distance: f32,
    pub effects: ShellEffects,
    pub ballistics: ShellBallistics,
    pub penetration_roll: f32,
    pub damage_roll: f32,
    pub travelled: f32,
    pub penetration_loss: f32,
    pub ricocheted: bool,
    pub heat_jet: bool,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct GunRay {
    pub origin: Vec3,
    pub direction: Vec3,
    pub speed: f32,
}

impl Tank {
    pub fn new(
        parameters: BattleParameters,
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
            physics_grounded: true,
            physics_tipped: false,
            physics_settled_seconds: 0.0,
            health,
            performance,
            max_health: health,
            ammunition: Ammunition::new(ammo, capacity, parameters.unlimited_ammo),
            drive: Drive::default(),
            aim: None,
            gun_yaw: 0.0,
            gun_pitch: 0.0,
            stun_traverse: None,
            last_shot: None,
            condition: Condition::default(),
            parameters,
        };
        tank.advance_gun(0.0);
        tank
    }

    pub fn drive(&mut self, flags: u8) -> bool {
        if flags & 0x80 != 0 {
            return false;
        }
        self.drive.set_flags(flags);
        true
    }

    pub fn set_hull_tilt(&mut self, direction: Vec3) {
        self.hull_tilt = direction.xy();
    }

    pub fn collision_extents(&self) -> (Vec3, Vec3) {
        let mobility = self.performance.as_ref().map(|p| &p.mobility);
        let [min, max] = mobility
            .and_then(|profile| profile.collision_bounds)
            .unwrap_or_else(|| {
                let size = mobility.map_or(
                    Vec2::new(
                        self.parameters.fallback_hull_half_width_metres,
                        self.parameters.fallback_hull_half_length_metres,
                    ),
                    |profile| Vec2::new(profile.track_half_width, profile.track_half_length),
                );
                [
                    Vec3::new(-size.x, 0.0, -size.y),
                    Vec3::new(size.x, self.parameters.hull_height_metres, size.y),
                ]
            });
        ((max - min) * 0.5, (max + min) * 0.5)
    }

    pub fn is_tipped(&self) -> bool {
        self.physics_tipped
    }

    fn base_mass_kg(&self) -> f32 {
        self.performance
            .as_ref()
            .map(|profile| profile.mobility.mass_kg)
            .filter(|mass| mass.is_finite() && *mass > 0.0)
            .unwrap_or(self.parameters.fallback_mass_kg)
    }

    pub fn effective_mass_kg(&self) -> f32 {
        (f64::from(self.base_mass_kg()) * f64::from(self.parameters.weight_multiplier)).clamp(
            f64::from(MIN_VEHICLE_MASS_KG),
            f64::from(MAX_VEHICLE_MASS_KG),
        ) as f32
    }

    fn effective_power_watts(&self) -> f64 {
        let base_power = self.performance.as_ref().map_or(
            f64::from(self.parameters.fallback_engine_power_watts),
            |profile| f64::from(self.base_mass_kg()) * f64::from(profile.mobility.power_per_kg),
        );
        base_power * f64::from(self.parameters.horsepower_multiplier)
    }

    pub fn collision_inverse_mass(&self) -> f32 {
        if self.health <= 0 {
            0.0
        } else {
            self.effective_mass_kg().recip()
        }
    }

    pub fn scenery_push_load(&self, seconds: f32, turning: bool) -> f32 {
        let powered = if turning {
            self.drive.has_steering()
        } else {
            self.drive.throttle() != 0.0
        };
        if !powered || !self.physics_grounded || self.is_tipped() || self.condition.immobilized() {
            return 0.0;
        }
        (f64::from(self.parameters.traction_multiplier)
            * self.effective_power_watts()
            * f64::from(self.parameters.scenery_push_force_scale)
            * f64::from(seconds)
            * 0.5)
            .min(f64::from(f32::MAX)) as f32
    }

    pub fn scenery_push_direction(&self) -> Vec2 {
        let (sin, cos) = self.yaw.sin_cos();
        Vec2::new(sin, cos) * self.drive.throttle()
    }

    pub fn drive_readback(&self) -> (u8, f32, f32) {
        (self.drive.flags, self.drive.speed, self.drive.yaw_rate)
    }

    pub fn track_speeds(&self) -> [f32; 2] {
        let half_width = self
            .performance
            .as_ref()
            .map_or(1.25, |profile| profile.mobility.track_half_width);
        self.drive.track_speeds(half_width)
    }

    pub fn engine_mode(&self) -> [u8; 2] {
        if self.condition.immobilized() {
            [1, 0]
        } else {
            self.drive.engine_mode()
        }
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
        self.drive.is_active()
    }

    pub fn stop_motion(&mut self) {
        self.drive.stop();
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

    pub fn drive_demand(
        &mut self,
        seconds: f32,
        ground: GroundContact,
        forward_speed: f32,
        yaw_rate: f32,
    ) -> DriveDemand {
        let Some(performance) = &self.performance else {
            return DriveDemand::default();
        };
        if self.health <= 0
            || self.is_tipped()
            || self.condition.immobilized()
            || seconds <= 0.0
            || !seconds.is_finite()
            || !forward_speed.is_finite()
            || !yaw_rate.is_finite()
            || !ground.pitch.is_finite()
            || ground.surfaces == [0; 3]
        {
            return DriveDemand::default();
        }
        let power_per_kg = self.effective_power_watts() / f64::from(self.effective_mass_kg());
        self.drive.demand(
            performance,
            ground,
            seconds,
            &self.parameters,
            [forward_speed, yaw_rate],
            power_per_kg,
        )
    }

    pub fn sync_physics(
        &mut self,
        position: Vec3,
        direction: Vec3,
        forward_speed: f32,
        yaw_rate: f32,
        grounded: bool,
    ) {
        self.position = position;
        self.yaw = direction.z;
        self.set_hull_tilt(direction);
        self.drive.speed = forward_speed;
        self.drive.yaw_rate = yaw_rate;
        self.physics_grounded = grounded;
        self.physics_tipped =
            direction.x.cos() * direction.y.cos() <= self.parameters.tip_terrain_roll_radians.cos();
    }

    pub fn physical_recovery_due(
        &mut self,
        stationary_on_stable_ground: bool,
        seconds: f32,
    ) -> bool {
        if !self.physics_tipped || self.health <= 0 || !stationary_on_stable_ground {
            self.physics_settled_seconds = 0.0;
            return false;
        }
        self.physics_settled_seconds += f64::from(seconds);
        self.physics_settled_seconds
            >= f64::from(self.parameters.settle_time_seconds)
                + f64::from(self.parameters.recovery_time_seconds)
    }

    pub fn set_stun_traverse(&mut self, factor: Option<f32>) {
        self.stun_traverse = factor;
    }

    pub fn advance_gun(&mut self, seconds: f32) -> bool {
        let seconds = seconds * self.stun_traverse.unwrap_or(1.0);
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
                let horizontal = from_gun.z.max(0.0);
                let pitch = gun
                    .shell_ballistics
                    .get(&self.ammunition.settings()[0])
                    .map_or_else(
                        || from_gun.y.atan2(horizontal),
                        |shell| elevation(horizontal, from_gun.y, shell.speed, shell.gravity),
                    );
                (yaw, -pitch)
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
        if self.condition.disabled(wot_shared_kernel::Device::Gun) {
            return Err("gun_disabled");
        }
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
        let ballistics = performance.gun.shell_ballistics.get(&shell).copied();
        if ballistics.is_none() && self.ammunition.loaded(shell) > 0 {
            return Err("shell_ballistics_unavailable");
        }
        self.ammunition.fire(&performance.gun.loading, now)?;
        let ballistics = ballistics.ok_or("shell_ballistics_unavailable")?;
        let ray = self.gun_ray();
        self.last_shot = Some(now);
        Ok(Shot {
            origin: ray.origin,
            travelled: 0.0,
            penetration_loss: 0.0,
            ricocheted: false,
            heat_jet: false,
            direction: ray.direction,
            damage,
            max_distance: max_distance.min(ballistics.damage_limited_range),
            effects,
            ballistics,
            penetration_roll: roll(
                ballistics.penetration_randomization,
                ballistics.uniform_penetration,
                &self.parameters,
            ),
            damage_roll: roll(
                ballistics.damage_randomization,
                ballistics.uniform_damage,
                &self.parameters,
            ),
        })
    }

    pub fn spotting_state(&self, entity: u32, team: u8, moving: bool, now: Instant) -> Combatant {
        let profile = self
            .performance
            .as_ref()
            .and_then(|vehicle| vehicle.spotting);
        let firing = self.last_shot.is_some_and(|shot| {
            now.saturating_duration_since(shot)
                < std::time::Duration::from_millis(
                    u64::from(self.parameters.spotting_shot_ticks) * 100,
                )
        });
        let geometry = profile.map(|profile| {
            let hull = self.hull_rotation();
            let checkpoints = local_checkpoints(&profile).map(|point| self.position + hull * point);
            let turret = Quat::from_rotation_y(self.gun_yaw);
            let mount = profile.hull_offset + profile.turret_offset + turret * profile.gun_mount;
            SightGeometry {
                checkpoints,
                ports: [checkpoints[0], self.position + hull * mount],
            }
        });
        let concealment = profile.map_or(0.0, |profile| {
            let base = if moving {
                profile.moving_concealment
            } else {
                profile.still_concealment
            };
            base * 0.5
                * if firing {
                    profile.firing_multiplier
                } else {
                    1.0
                }
        });
        Combatant {
            entity,
            team,
            position: self.position,
            geometry,
            view_range: profile.map_or(0.0, |profile| profile.view_range),
            radio_range: profile.map_or(0.0, |profile| profile.radio_range),
            concealment,
            firing,
        }
    }

    pub fn gun_ray(&self) -> GunRay {
        let hull = self.hull_rotation();
        let turret = Quat::from_rotation_y(self.gun_yaw);
        let (pivot, shot_position) = self.gun_mount();
        let speed = self
            .performance
            .as_ref()
            .and_then(|profile| {
                profile
                    .gun
                    .shell_ballistics
                    .get(&self.ammunition.settings()[0])
            })
            .map_or(self.parameters.fallback_gun_ray_speed_mps, |shell| {
                shell.speed
            });
        GunRay {
            speed,
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

    pub fn apply_criticals(&mut self, strikes: &[Option<DeviceStrike>], now: Instant) -> Criticals {
        let Some(performance) = &self.performance else {
            return Criticals::default();
        };
        let armed = self.ammunition.rounds.iter().any(|&(_, count)| count > 0);
        self.condition.apply(
            strikes,
            &performance.device_health,
            armed,
            now,
            &self.parameters,
        )
    }

    pub fn fire_tick_damage(&self, remaining_ticks: u8) -> u16 {
        let damage = f32::from(self.max_health)
            * self.parameters.fire_damage_percent
            * f32::from(remaining_ticks)
            / (100.0 * f32::from(self.parameters.fire_ticks.max(1)));
        (damage.round() as u16).max(1)
    }

    pub fn hit(&mut self, damage: u16) -> (i16, i16) {
        let previous = self.health;
        let minimum = i16::from(self.parameters.unlimited_life && previous > 0);
        self.health = previous.saturating_sub_unsigned(damage).max(minimum);
        if self.health == 0 {
            self.condition.extinguish();
            self.drive.set_flags(0);
        }
        (previous, self.health)
    }

    pub fn armor_pose(&self, part: usize, offset: Vec3) -> (Vec3, Quat) {
        let hull = self.hull_rotation();
        let turret = hull * Quat::from_rotation_y(self.gun_yaw);
        let (pivot, _) = self.gun_mount();
        match part {
            2 => (self.position + hull * pivot + turret * offset, turret),
            3 => (
                self.position + hull * pivot + turret * offset,
                turret * Quat::from_rotation_x(self.gun_pitch),
            ),
            _ => (self.position + hull * offset, hull),
        }
    }

    fn hull_rotation(&self) -> Quat {
        Quat::from_euler(EulerRot::YXZ, self.yaw, self.hull_tilt.y, self.hull_tilt.x)
    }

    fn gun_mount(&self) -> (Vec3, Vec3) {
        let default_pivot = Vec3::Y * self.parameters.fallback_turret_height_metres;
        self.performance
            .as_ref()
            .map_or((default_pivot, Vec3::ZERO), |profile| {
                (
                    profile.turret_pivot.unwrap_or(default_pivot),
                    profile.gun.shot_position.unwrap_or_default(),
                )
            })
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
            BattleParameters::default(),
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
                armor: Default::default(),
                spotting: None,
                device_health: Default::default(),
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
                    shell_ballistics: [
                        (
                            10,
                            test_ballistics(36.0, wot_shared_kernel::ShellKind::ArmorPiercing),
                        ),
                        (
                            20,
                            test_ballistics(
                                90.0,
                                wot_shared_kernel::ShellKind::HighExplosive {
                                    shield_penetration: true,
                                    obstacle_penetration: true,
                                },
                            ),
                        ),
                    ]
                    .into(),
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
                                resisted_sticker: None,
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
                                resisted_sticker: None,
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

    fn test_ballistics(damage: f32, kind: wot_shared_kernel::ShellKind) -> ShellBallistics {
        ShellBallistics {
            kind,
            caliber_mm: 37.0,
            penetration_mm: [100.0, 100.0],
            penetration_randomization: 0.0,
            damage_randomization: 0.0,
            uniform_penetration: false,
            uniform_damage: false,
            far_damage: damage,
            damage_limited_range: f32::INFINITY,
            device_damage: [0.0; 2],
            explosive: None,
            speed: BattleParameters::default().fallback_gun_ray_speed_mps,
            gravity: 0.0,
            normalization: 5.0_f32.to_radians(),
            ricochet_cos: 70.0_f32.to_radians().cos(),
            trace_ricochet: true,
            obstacle_damage: None,
            obstacle_penetration_loss: 0.0,
        }
    }

    #[test]
    fn gameplay_mobility_scales_power_and_mass_and_can_bypass_the_speed_governor() {
        let mut tank = tank(vec![]);
        tank.drive(FORWARD);
        let mass = tank.effective_mass_kg();
        let acceleration = tank
            .drive_demand(0.1, HARD, 8.0, 0.0)
            .longitudinal_acceleration;
        tank.parameters.horsepower_multiplier = 2.0;
        assert!(
            tank.drive_demand(0.1, HARD, 8.0, 0.0)
                .longitudinal_acceleration
                > acceleration
        );
        tank.parameters.weight_multiplier = 2.0;
        assert_eq!(tank.effective_mass_kg(), 2.0 * mass);
        assert_eq!(tank.collision_inverse_mass(), (2.0 * mass).recip());
        assert_eq!(
            tank.drive_demand(0.1, HARD, 8.0, 0.0)
                .longitudinal_acceleration,
            acceleration
        );

        for (flags, speed) in [(FORWARD, 13.0), (REVERSE, -5.0)] {
            tank.drive(flags);
            assert!(
                tank.drive_demand(0.1, HARD, speed, 0.0)
                    .longitudinal_acceleration
                    * speed
                    < 0.0
            );
            tank.parameters.speed_limit_enabled = false;
            assert!(
                tank.drive_demand(0.1, HARD, speed, 0.0)
                    .longitudinal_acceleration
                    * speed
                    > 0.0
            );
            tank.parameters.speed_limit_enabled = true;
        }
        tank.parameters.horsepower_multiplier = 0.0;
        assert!(
            tank.drive_demand(0.1, HARD, 0.0, 0.0)
                .longitudinal_acceleration
                .abs()
                < f32::EPSILON
        );
    }

    #[test]
    fn unlimited_life_preserves_normal_damage_until_one_hit_point() {
        let mut tank = tank(vec![]);
        tank.parameters.unlimited_life = true;
        assert_eq!(tank.hit(20), (100, 80));
        assert_eq!(tank.hit(u16::MAX), (80, 1));
        assert_eq!(tank.hit(80), (1, 1));
        tank.parameters.unlimited_life = false;
        assert_eq!(tank.hit(1), (1, 0));
    }

    #[test]
    fn physical_drive_uses_equipped_power_and_surface_without_integrating_gravity_or_pose() {
        let mut tank = tank(vec![]);
        tank.drive(FORWARD);
        let demand = tank.drive_demand(0.1, HARD, 2.0, 0.0);
        let downhill = GroundContact { pitch: 0.2, ..HARD };
        let soft = GroundContact {
            surfaces: [0, 0, 4],
            ..HARD
        };
        assert_eq!(
            demand.longitudinal_acceleration,
            tank.drive_demand(0.1, downhill, 2.0, 0.0)
                .longitudinal_acceleration
        );
        assert!(
            demand.longitudinal_acceleration
                > tank
                    .drive_demand(0.1, soft, 2.0, 0.0)
                    .longitudinal_acceleration
        );
        let standard = tank.drive_demand(0.1, HARD, 8.0, 0.0);
        let mobility = &mut tank.performance.as_mut().unwrap().mobility;
        mobility.power_per_kg *= 2.0;
        mobility.max_climb_angle = Some(0.1);
        assert!(
            tank.drive_demand(0.1, HARD, 8.0, 0.0)
                .longitudinal_acceleration
                > standard.longitudinal_acceleration
        );
        let uphill = GroundContact {
            pitch: -0.2,
            ..HARD
        };
        assert_eq!(
            tank.drive_demand(0.1, uphill, 0.0, 0.0)
                .longitudinal_acceleration,
            0.0
        );
        assert_eq!(tank.position, Vec3::ZERO);
        assert_eq!(tank.track_speeds(), [0.0, 0.0]);
    }

    #[test]
    fn physical_governor_preserves_coasting_braking_and_powered_downhill_limit() {
        let mut tank = tank(vec![]);
        let downhill = GroundContact { pitch: 0.2, ..HARD };
        let seconds = 1.0 / 60.0;
        for (flags, ground, expected) in [
            (FORWARD, HARD, 12.0),
            (REVERSE, HARD, -4.0),
            (FORWARD | HALF_SPEED, downhill, 6.0),
            (FORWARD, downhill, 18.0),
        ] {
            tank.drive(flags);
            let mut speed = 0.0;
            for _ in 0..2400 {
                let command = tank.drive_demand(seconds, ground, speed, 0.0);
                let gravity = tank.parameters.gravity_mps2 * ground.pitch.sin();
                speed += (command.longitudinal_acceleration + gravity) * seconds;
            }
            assert!(
                (speed - expected).abs() < 0.001,
                "flags {flags}, speed {speed}"
            );
        }
        tank.sync_physics(Vec3::ZERO, Vec3::ZERO, 8.0, 0.0, true);
        tank.drive(0);
        let coast = tank
            .drive_demand(seconds, HARD, 8.0, 0.0)
            .longitudinal_acceleration;
        tank.drive(BRAKE);
        let brake = tank
            .drive_demand(seconds, HARD, 8.0, 0.0)
            .longitudinal_acceleration;
        assert!((coast + tank.parameters.coast_deceleration_mps2).abs() < 0.001);
        assert!(brake < coast && coast < 0.0);
    }

    #[test]
    fn physical_pivots_and_chassis_animation_use_track_geometry_and_solved_velocity() {
        let mut tank = tank(vec![]);
        tank.drive(LEFT);
        let demand = tank.drive_demand(0.1, HARD, 0.0, 0.0);
        assert!(demand.yaw_acceleration < 0.0);
        assert_eq!(demand.longitudinal_acceleration, 0.0);
        assert!(!demand.requests_travel);
        assert!(tank.drive_demand(0.1, HARD, -3.0, 0.0).yaw_acceleration > 0.0);
        tank.performance
            .as_mut()
            .unwrap()
            .mobility
            .rotation_around_center = false;
        let demand = tank.drive_demand(0.1, HARD, 0.0, 0.0);
        assert!(demand.requests_travel && demand.longitudinal_acceleration > 0.0);
        let forward_speed = demand.longitudinal_acceleration * 0.1;
        let yaw_rate = demand.yaw_acceleration * 0.1;
        tank.sync_physics(Vec3::Z, Vec3::ZERO, forward_speed, yaw_rate, true);
        let [left, right] = tank.track_speeds();
        assert!(left.abs() < 0.001 && right > 0.0);
        tank.performance.as_mut().unwrap().mobility.track_lock = true;
        tank.drive(BRAKE | LEFT);
        assert!(tank.drive_demand(0.1, HARD, 8.0, 0.0).track_locked);
    }

    #[test]
    fn overturned_physical_body_gates_drive_fire_and_restarts_recovery_after_motion() {
        let mut tank = tank(vec![]);
        tank.drive(FORWARD);
        let roof_down = Vec3::X * std::f32::consts::PI;
        tank.sync_physics(Vec3::Y, roof_down, 0.0, 0.0, false);
        assert_eq!(tank.scenery_push_load(0.1, false), 0.0);
        assert_eq!(
            tank.drive_demand(0.1, HARD, 0.0, 0.0)
                .longitudinal_acceleration,
            0.0
        );
        assert_eq!(tank.fire(Instant::now()).err(), Some("vehicle_overturned"));
        assert!(!tank.physical_recovery_due(true, 10.0));
        assert!(!tank.physical_recovery_due(false, 1.0));
        assert!(!tank.physical_recovery_due(true, 17.0));
        assert!(tank.physical_recovery_due(true, 1.0));
        tank.sync_physics(Vec3::ZERO, Vec3::ZERO, 0.0, 0.0, true);
        assert!(!tank.is_tipped());
        assert!(!tank.physical_recovery_due(true, 1.0));
        assert!(
            tank.drive_demand(0.1, HARD, 0.0, 0.0)
                .longitudinal_acceleration
                > 0.0
        );
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
        assert!(
            (target.position + Vec3::Y - shot.origin)
                .cross(shot.direction)
                .length()
                < 0.001
        );
        assert_eq!(shot.damage, 36);
        assert_eq!(shot.max_distance, 720.0);
        assert_eq!((shot.effects.kind, shot.effects.legacy_index), (2, 17));
        assert_eq!(target.hit(shot.damage), (100, 64));
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
        assert!((distant.position - shot.origin).length() > shot.max_distance);
        assert_eq!((shot.effects.kind, shot.effects.legacy_index), (1, 19));
        assert_eq!(target.hit(shot.damage), (64, 0));
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
