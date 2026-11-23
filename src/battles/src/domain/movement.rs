use glam::{Vec2, Vec3};
use wot_shared_kernel::VehiclePerformance;

const FORWARD: u8 = 1;
const REVERSE: u8 = 2;
const LEFT: u8 = 4;
const RIGHT: u8 = 8;
const HALF_SPEED: u8 = 16;
const QUARTER_SPEED: u8 = 32;
const BRAKE: u8 = 64;

const GRAVITY: f32 = 9.81;
const COAST_DECELERATION: f32 = 14.0;
const BRAKE_DECELERATION: f32 = 24.0;
const TURN_RESPONSE_SECONDS: f32 = 0.2;

#[derive(Clone, Copy)]
pub(crate) struct GroundContact {
    pub surfaces: [u8; 3],
    pub pitch: f32,
}

impl GroundContact {
    fn average(self, coefficients: [f32; 3]) -> f32 {
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
}

impl Drive {
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
            let deceleration = if braking {
                BRAKE_DECELERATION
            } else {
                COAST_DECELERATION
            };
            self.speed = approach(self.speed, 0.0, deceleration * seconds);
        } else if self.speed.abs() > self.target_speed.abs() {
            self.speed = approach(self.speed, self.target_speed, COAST_DECELERATION * seconds);
        } else {
            let traction = 1.13 * mobility.power_per_kg / self.speed.abs().max(1.0);
            let rolling_drag = GRAVITY * ground.average(mobility.rolling_friction);
            let slope = GRAVITY * ground.pitch.sin() * self.target_speed.signum();
            let acceleration = traction.min(6.0 / resistance) - rolling_drag + slope;
            let speed =
                (self.speed.abs() + acceleration * seconds).clamp(0.0, self.target_speed.abs());
            self.speed = speed * self.target_speed.signum();
        }

        let traverse = performance.hull_traverse * mobility.terrain_resistance[0] / resistance;
        let turn_target = if mobility.turns_in_place {
            let direction = if self.speed < 0.0 || (self.speed == 0.0 && throttle < 0.0) {
                -1.0
            } else {
                1.0
            };
            let moving = if self.speed.abs() > 0.5 { 0.85 } else { 1.0 };
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
        self.yaw_rate = approach(
            self.yaw_rate,
            turn_target,
            traverse * seconds / TURN_RESPONSE_SECONDS,
        );

        let turn = (old_yaw_rate + self.yaw_rate) * 0.5 * seconds;
        let heading = *yaw + turn * 0.5;
        let forward = Vec2::new(heading.sin(), heading.cos());
        let start = Vec2::new(position.x, position.z);
        let next = start + forward * (old_speed + self.speed) * 0.5 * seconds;
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

    pub fn track_speeds(&self, half_width: f32) -> [f32; 2] {
        [
            self.speed + self.yaw_rate * half_width,
            self.speed - self.yaw_rate * half_width,
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
