use glam::{Vec2, Vec3};
use wot_shared_kernel::VehiclePerformance;

const FORWARD: u8 = 1;
const REVERSE: u8 = 2;
const LEFT: u8 = 4;
const RIGHT: u8 = 8;
const HALF_SPEED: u8 = 16;
const QUARTER_SPEED: u8 = 32;
const BRAKE: u8 = 64;

pub(super) const GRAVITY: f32 = 9.81;
const COAST_DECELERATION: f32 = 14.0;
const BRAKE_DECELERATION: f32 = 24.0;
const TURN_RESPONSE_SECONDS: f32 = 0.2;
const TRACK_LOCK_MIN_SPEED_MPS: f32 = 2.0;
const TRACK_LOCK_DECELERATION: f32 = 6.0;
const TRACK_LOCK_RESPONSE_SECONDS: f32 = 0.1;
const DOWNHILL_SPEED_LIMIT_RATIO: f32 = 1.5;

#[derive(Clone, Copy)]
pub(crate) struct GroundContact {
    pub surfaces: [u8; 3],
    pub pitch: f32,
}

impl GroundContact {
    pub(crate) fn average(self, coefficients: [f32; 3]) -> f32 {
        self.surfaces
            .into_iter()
            .zip(coefficients)
            .map(|(count, coefficient)| f32::from(count) * coefficient)
            .sum::<f32>()
            * 0.25
    }
}

#[derive(Default)]
pub(super) struct Drive {
    pub flags: u8,
    pub speed: f32,
    pub yaw_rate: f32,
    curvature: f32,
    target_speed: f32,
    pivot_speed: f32,
    movement_heading: Option<f32>,
}

impl Drive {
    pub fn throttle(&self) -> f32 {
        if self.flags & BRAKE == 0 {
            axis(self.flags, FORWARD, REVERSE)
        } else {
            0.0
        }
    }

    pub fn has_steering(&self) -> bool {
        self.flags & BRAKE == 0 && axis(self.flags, RIGHT, LEFT) != 0.0
    }

    pub fn step(
        &mut self,
        performance: &VehiclePerformance,
        ground: GroundContact,
        position: &mut Vec3,
        yaw: &mut f32,
        seconds: f32,
        [min, max]: [Vec2; 2],
    ) {
        let mobility = &performance.mobility;
        let resistance = ground.average(mobility.terrain_resistance);
        let throttle = axis(self.flags, FORWARD, REVERSE);
        let steering = axis(self.flags, RIGHT, LEFT);
        let braking = self.flags & BRAKE != 0;
        let track_lock = mobility.track_lock
            && braking
            && steering != 0.0
            && self.speed.abs() >= TRACK_LOCK_MIN_SPEED_MPS;
        let speed_limit = if throttle < 0.0 {
            performance.reverse_speed
        } else {
            performance.forward_speed
        };
        let cruise = if self.flags & QUARTER_SPEED != 0 {
            0.25
        } else if self.flags & HALF_SPEED != 0 {
            0.5
        } else {
            1.0
        };
        self.target_speed = if braking {
            0.0
        } else if !mobility.turns_in_place && throttle == 0.0 && steering != 0.0 {
            2.0_f32.min(speed_limit)
        } else {
            throttle * speed_limit * cruise
        };

        let old_speed = self.speed;
        let old_yaw_rate = self.yaw_rate;
        let reversing = self.speed * self.target_speed < 0.0;
        if braking || self.target_speed == 0.0 || reversing {
            let deceleration = if track_lock {
                TRACK_LOCK_DECELERATION
            } else if braking {
                BRAKE_DECELERATION
            } else {
                COAST_DECELERATION
            };
            self.speed = approach(self.speed, 0.0, deceleration * seconds);
        } else {
            let traction = 1.13 * mobility.power_per_kg / self.speed.abs().max(1.0);
            let rolling_drag = GRAVITY * ground.average(mobility.rolling_friction);
            let grade = ground.pitch * self.target_speed.signum();
            let slope = GRAVITY * grade.sin();
            let target = self.target_speed.abs();
            let drive_force = if mobility.max_climb_angle.is_some_and(|limit| grade < -limit) {
                0.0
            } else {
                traction.min(6.0 / resistance)
            };
            let speed = if self.speed.abs() < target {
                (self.speed.abs() + (drive_force - rolling_drag + slope) * seconds)
                    .clamp(0.0, target)
            } else if cruise == 1.0 && slope > rolling_drag {
                (self.speed.abs() + (slope - rolling_drag) * seconds)
                    .min(target * DOWNHILL_SPEED_LIMIT_RATIO)
            } else if self.speed.abs() == target {
                (target + (drive_force - rolling_drag + slope).min(0.0) * seconds).max(0.0)
            } else {
                approach(self.speed.abs(), target, COAST_DECELERATION * seconds)
            };
            self.speed = speed * self.target_speed.signum();
        }

        let ground_traverse = performance.hull_traverse * mobility.terrain_resistance[0]
            / (resistance * mobility.turning_resistance);
        let traverse = ground_traverse.min(mobility.traverse_limit.unwrap_or(f32::INFINITY));
        let turn_target = if mobility.turns_in_place {
            let direction = if self.speed < 0.0 || (self.speed == 0.0 && throttle < 0.0) {
                -1.0
            } else {
                1.0
            };
            let moving = if track_lock {
                1.0
            } else if self.speed.abs() > 0.5 {
                0.85
            } else {
                1.0
            };
            steering * direction * traverse * moving
        } else {
            let max_curvature = mobility
                .wheels
                .iter()
                .filter_map(|wheel| {
                    let limit = wheel.steering_limit?.abs();
                    (limit > 0.0 && wheel.center.y.abs() > 0.1).then(|| {
                        limit.tan() / (wheel.center.y.abs() + wheel.center.x.abs() * limit.tan())
                    })
                })
                .reduce(f32::min)
                .unwrap_or(0.0);
            self.curvature = approach(
                self.curvature,
                steering * max_curvature,
                max_curvature * seconds / TURN_RESPONSE_SECONDS,
            );
            (self.speed * self.curvature).clamp(-traverse, traverse)
        };
        let response_seconds = if track_lock {
            TRACK_LOCK_RESPONSE_SECONDS
        } else {
            TURN_RESPONSE_SECONDS
        };
        self.yaw_rate = approach(
            self.yaw_rate,
            turn_target,
            traverse * seconds / response_seconds,
        );

        let turn = (old_yaw_rate + self.yaw_rate) * 0.5 * seconds;
        let heading = *yaw + turn * 0.5;
        let movement_heading = if track_lock {
            let previous = self.movement_heading.unwrap_or(*yaw);
            let delta = super::vehicle::angle(heading - previous);
            let heading = previous + delta * (2.0 * seconds).clamp(0.0, 1.0);
            self.movement_heading = Some(heading);
            heading
        } else {
            self.movement_heading = None;
            heading
        };
        let forward = Vec2::new(movement_heading.sin(), movement_heading.cos());
        let start = Vec2::new(position.x, position.z);
        let pivoting = mobility.turns_in_place
            && !mobility.rotation_around_center
            && mobility.wheels.is_empty()
            && !braking
            && throttle == 0.0
            && old_speed == 0.0
            && self.speed == 0.0;
        self.pivot_speed = if pivoting {
            self.yaw_rate.abs() * mobility.track_half_width
        } else {
            0.0
        };
        let pivot_distance = if pivoting {
            turn.abs() * mobility.track_half_width
        } else {
            0.0
        };
        let next = start + forward * ((old_speed + self.speed) * 0.5 * seconds + pivot_distance);
        let bounded = next.clamp(min, max);
        if bounded != next {
            self.speed *=
                forward.dot(bounded - start).abs() / next.distance(start).max(f32::EPSILON);
        }
        if turn != 0.0 {
            *yaw = super::vehicle::angle(*yaw + turn);
        }
        position.x = bounded.x;
        position.z = bounded.y;
    }

    pub fn coast_in_air(
        &mut self,
        position: &mut Vec3,
        yaw: f32,
        seconds: f32,
        [min, max]: [Vec2; 2],
    ) {
        let heading = self.movement_heading.unwrap_or(yaw);
        self.yaw_rate = 0.0;
        self.pivot_speed = 0.0;
        let offset = Vec2::new(heading.sin(), heading.cos()) * self.speed * seconds;
        let next = Vec2::new(position.x, position.z) + offset;
        let bounded = next.clamp(min, max);
        if bounded != next {
            self.speed = 0.0;
        }
        position.x = bounded.x;
        position.z = bounded.y;
    }

    pub fn track_speeds(&self, half_width: f32) -> [f32; 2] {
        let speed = self.speed + self.pivot_speed;
        [
            speed + self.yaw_rate * half_width,
            speed - self.yaw_rate * half_width,
        ]
    }

    pub fn engine_mode(&self) -> [u8; 2] {
        let mode = if self.flags == 0 {
            1
        } else if (self.target_speed - self.speed).abs() > 0.1 || self.yaw_rate.abs() > 0.1 {
            3
        } else {
            2
        };
        [mode, self.flags]
    }

    pub fn rpm(&self, performance: &VehiclePerformance) -> f32 {
        let speed_limit = if self.speed < 0.0 {
            performance.reverse_speed
        } else {
            performance.forward_speed
        };
        let speed_share = self.speed.abs() / speed_limit.max(0.1);
        let turn_share = self.yaw_rate.abs() / performance.hull_traverse.max(0.1);
        0.2 + 0.8 * speed_share.max(turn_share).min(1.0)
    }

    pub fn wheel_motion(
        &self,
        performance: &VehiclePerformance,
    ) -> impl Iterator<Item = (f32, Option<f32>)> {
        performance.mobility.wheels.iter().map(|wheel| {
            let curvature = if self.speed.abs() > 0.1 {
                self.yaw_rate / self.speed
            } else {
                self.curvature
            };
            let angle = wheel.steering_limit.map(|limit| {
                (wheel.center.y * curvature)
                    .atan2((1.0 - wheel.center.x * curvature).max(0.01))
                    .clamp(-limit.abs(), limit.abs())
            });
            let heading = angle.unwrap_or(0.0);
            let speed = (self.speed - wheel.center.x * self.yaw_rate) * heading.cos()
                + wheel.center.y * self.yaw_rate * heading.sin();
            (speed, angle)
        })
    }

    pub fn is_active(&self) -> bool {
        self.flags != 0 || self.speed != 0.0 || self.yaw_rate != 0.0 || self.curvature != 0.0
    }

    pub fn is_track_locked(&self) -> bool {
        self.movement_heading.is_some()
    }

    pub fn stop(&mut self) {
        self.speed = 0.0;
        self.yaw_rate = 0.0;
        self.curvature = 0.0;
        self.pivot_speed = 0.0;
        self.movement_heading = None;
    }
}

fn axis(flags: u8, positive: u8, negative: u8) -> f32 {
    f32::from(u8::from(flags & positive != 0)) - f32::from(u8::from(flags & negative != 0))
}

pub(super) fn approach(current: f32, target: f32, step: f32) -> f32 {
    let difference = target - current;
    if difference.abs() <= step {
        target
    } else {
        current + difference.signum() * step
    }
}
