use super::parameters::{BattleParameters, MAX_ANGULAR_SPEED, MAX_LINEAR_SPEED};
use wot_shared_kernel::VehiclePerformance;

const FORWARD: u8 = 1;
const REVERSE: u8 = 2;
const LEFT: u8 = 4;
const RIGHT: u8 = 8;
const HALF_SPEED: u8 = 16;
const QUARTER_SPEED: u8 = 32;
const BRAKE: u8 = 64;

#[derive(Clone, Copy)]
pub(crate) struct GroundContact {
    pub surfaces: [u8; 3],
    pub pitch: f32,
}

impl GroundContact {
    pub(crate) fn average(self, coefficients: [f32; 3]) -> f32 {
        (self
            .surfaces
            .into_iter()
            .zip(coefficients)
            .map(|(count, coefficient)| f64::from(count) * f64::from(coefficient))
            .sum::<f64>()
            * 0.25) as f32
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct DriveDemand {
    pub longitudinal_acceleration: f32,
    pub yaw_acceleration: f32,
    pub track_locked: bool,
    pub requests_travel: bool,
}

struct DriveResponse {
    speed: f32,
    yaw_rate: f32,
    track_locked: bool,
    single_track_pivot: bool,
}

#[derive(Default)]
pub(super) struct Drive {
    pub flags: u8,
    pub speed: f32,
    pub yaw_rate: f32,
    curvature: f32,
    target_speed: f32,
    released_speed: f32,
}

impl Drive {
    pub fn set_flags(&mut self, flags: u8) {
        if flags == 0 && self.flags != 0 {
            self.released_speed = self.speed;
        } else if flags != 0 {
            self.released_speed = 0.0;
        }
        self.flags = flags;
    }

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

    fn response(
        &mut self,
        performance: &VehiclePerformance,
        ground: GroundContact,
        seconds: f32,
        parameters: &BattleParameters,
        [old_speed, old_yaw_rate]: [f32; 2],
    ) -> DriveResponse {
        let mobility = &performance.mobility;
        let resistance = ground.average(mobility.terrain_resistance);
        let throttle = axis(self.flags, FORWARD, REVERSE);
        let steering = axis(self.flags, RIGHT, LEFT);
        let braking = self.flags & BRAKE != 0;
        let track_lock = mobility.track_lock
            && braking
            && steering != 0.0
            && old_speed.abs() >= parameters.track_lock_min_speed_mps;
        let speed_limit = if throttle < 0.0 {
            performance.reverse_speed
        } else {
            performance.forward_speed
        };
        let cruise = if self.flags & QUARTER_SPEED != 0 {
            parameters.quarter_speed_ratio
        } else if self.flags & HALF_SPEED != 0 {
            parameters.half_speed_ratio
        } else {
            1.0
        };
        self.target_speed = if braking {
            0.0
        } else if !mobility.turns_in_place && throttle == 0.0 && steering != 0.0 {
            parameters.wheel_turn_creep_mps.min(speed_limit)
        } else {
            throttle * (speed_limit * cruise).min(MAX_LINEAR_SPEED)
        };

        let reversing = old_speed * self.target_speed < 0.0;
        let speed = if braking || self.target_speed == 0.0 || reversing {
            let deceleration = if track_lock {
                parameters.track_lock_deceleration_mps2
            } else if braking {
                parameters.brake_deceleration_mps2
            } else {
                parameters.coast_deceleration_mps2
            };
            approach(old_speed, 0.0, deceleration * seconds)
        } else {
            let traction =
                parameters.traction_multiplier * mobility.power_per_kg / old_speed.abs().max(1.0);
            let rolling_drag =
                parameters.gravity_mps2.max(0.0) * ground.average(mobility.rolling_friction);
            let grade = ground.pitch * self.target_speed.signum();
            let slope = parameters.gravity_mps2 * grade.sin();
            let target = self.target_speed.abs();
            let drive_force = if mobility.max_climb_angle.is_some_and(|limit| grade < -limit) {
                0.0
            } else {
                traction.min(parameters.traction_acceleration_mps2 / resistance)
            };
            // Gravity acts on the body independently. Predict its contribution only
            // to decide when the engine or the speed governor needs to act.
            let gravity_dv = slope * seconds;
            let speed = old_speed.abs();
            let limit = if cruise == 1.0 && slope > rolling_drag {
                target * parameters.downhill_speed_limit_ratio
            } else {
                target
            };
            let drag = rolling_drag.min(speed / seconds + drive_force);
            let requested_acceleration = (target - speed - gravity_dv) / seconds + drag;
            let propulsion = requested_acceleration.max(0.0).min(drive_force);
            let governor = ((speed + (propulsion - drag) * seconds + gravity_dv - limit) / seconds)
                .clamp(0.0, parameters.coast_deceleration_mps2);
            let speed = speed + (propulsion - drag - governor) * seconds;
            speed * self.target_speed.signum()
        };

        let ground_traverse = performance.hull_traverse * mobility.terrain_resistance[0]
            / (resistance * mobility.turning_resistance);
        let traverse = ground_traverse.min(mobility.traverse_limit.unwrap_or(f32::INFINITY));
        let turn_target = if mobility.turns_in_place {
            let pivot_at_rest =
                throttle == 0.0 && speed.abs() <= parameters.moving_turn_min_speed_mps;
            let direction = if (speed < 0.0 && !pivot_at_rest) || (speed == 0.0 && throttle < 0.0) {
                -1.0
            } else {
                1.0
            };
            let moving = if track_lock {
                1.0
            } else if speed.abs() > parameters.moving_turn_min_speed_mps {
                parameters.moving_turn_multiplier
            } else {
                1.0
            };
            steering * direction * (traverse * moving).min(MAX_ANGULAR_SPEED)
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
                max_curvature * seconds / parameters.turn_response_seconds,
            );
            (speed * self.curvature).clamp(-traverse, traverse)
        };
        let response_seconds = if track_lock {
            parameters.track_lock_response_seconds
        } else {
            parameters.turn_response_seconds
        };
        let yaw_rate = approach(
            old_yaw_rate,
            turn_target,
            traverse * seconds / response_seconds,
        );

        DriveResponse {
            speed,
            yaw_rate,
            track_locked: track_lock,
            single_track_pivot: mobility.turns_in_place
                && !mobility.rotation_around_center
                && mobility.wheels.is_empty()
                && !braking
                && throttle == 0.0,
        }
    }

    pub fn demand(
        &mut self,
        performance: &VehiclePerformance,
        ground: GroundContact,
        seconds: f32,
        parameters: &BattleParameters,
        measured: [f32; 2],
    ) -> DriveDemand {
        let [speed, yaw_rate] = measured;
        let response = self.response(performance, ground, seconds, parameters, measured);
        let moving_pivot = response.single_track_pivot && self.has_steering();
        let next_speed = if self.flags == 0 {
            // Releasing powered movement coasts to a stop. A parked hull pushed
            // by another vehicle only opposes its new motion with rolling resistance.
            let rolling = parameters.gravity_mps2.max(0.0)
                * ground.average(performance.mobility.rolling_friction);
            let coast = if speed * self.released_speed > 0.0 {
                self.released_speed
                    .abs()
                    .min(parameters.coast_deceleration_mps2 * seconds)
            } else {
                0.0
            };
            self.released_speed = approach(
                self.released_speed,
                0.0,
                parameters.coast_deceleration_mps2 * seconds,
            );
            approach(speed, 0.0, coast.max(rolling * seconds))
        } else if moving_pivot {
            let target = response.yaw_rate.abs() * performance.mobility.track_half_width;
            let acceleration = if speed > target {
                parameters.coast_deceleration_mps2
            } else {
                parameters.traction_acceleration_mps2
                    / ground.average(performance.mobility.terrain_resistance)
            };
            approach(speed, target, acceleration * seconds)
        } else {
            response.speed
        };
        DriveDemand {
            longitudinal_acceleration: (next_speed - speed) / seconds,
            yaw_acceleration: (response.yaw_rate - yaw_rate) / seconds,
            track_locked: response.track_locked,
            requests_travel: self.target_speed != 0.0 || moving_pivot,
        }
    }

    pub fn track_speeds(&self, half_width: f32) -> [f32; 2] {
        let speed = self.speed;
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

    pub fn stop(&mut self) {
        self.speed = 0.0;
        self.yaw_rate = 0.0;
        self.curvature = 0.0;
        self.released_speed = 0.0;
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
