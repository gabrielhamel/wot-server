use super::{parameters::BattleParameters, vehicle::Shot};
use glam::Vec3;

pub(crate) struct FlightSegment {
    pub shot: Shot,
    pub distance: f32,
    pub started: f32,
    pub seconds: f32,
    launch_velocity: Vec3,
}

impl FlightSegment {
    pub fn contact_speed(&self, elapsed: f32) -> f32 {
        (self.launch_velocity - Vec3::Y * self.shot.ballistics.gravity * elapsed).length()
    }
}

impl Shot {
    pub fn flight_segments(
        self,
        parameters: &BattleParameters,
    ) -> impl Iterator<Item = FlightSegment> + use<> {
        let mut origin = self.origin;
        let mut velocity = self.direction * self.ballistics.speed;
        let acceleration = -Vec3::Y * self.ballistics.gravity;
        let mut travelled = 0.0;
        let mut elapsed = 0.0;
        let step_seconds = parameters.flight_step_seconds;
        (0..parameters.max_flight_segments).map_while(move |_| {
            let remaining = self.max_distance - travelled;
            if remaining <= 0.0 {
                return None;
            }
            let speed = velocity.length().max(1.0);
            let step = if self.ballistics.gravity > 0.0 {
                step_seconds
            } else {
                remaining / speed
            };
            let displacement = (velocity + acceleration * (0.5 * step)) * step;
            let length = displacement.length();
            if length < 0.00001 {
                return None;
            }
            let distance = length.min(remaining);
            let seconds = step * (distance / length);
            let direction = displacement / length;
            let mut shot = self;
            shot.origin = origin;
            shot.direction = direction;
            shot.travelled += travelled;
            shot.max_distance = remaining;
            shot.ballistics.speed = speed;
            let segment = FlightSegment {
                shot,
                distance,
                started: elapsed,
                seconds,
                launch_velocity: velocity,
            };
            origin += direction * distance;
            velocity += acceleration * seconds;
            travelled += distance;
            elapsed += seconds;
            Some(segment)
        })
    }
}

pub(crate) fn elevation(horizontal: f32, height: f32, speed: f32, gravity: f32) -> f32 {
    if gravity <= 0.0 || horizontal < 0.001 {
        return height.atan2(horizontal);
    }
    let squared_speed = speed * speed;
    let discriminant = squared_speed * squared_speed
        - gravity * (gravity * horizontal * horizontal + 2.0 * height * squared_speed);
    if discriminant < 0.0 {
        return squared_speed.atan2(gravity * horizontal);
    }
    (gravity * horizontal * horizontal + 2.0 * height * squared_speed)
        .atan2(horizontal * (squared_speed + discriminant.sqrt()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angle_intersects_target_after_gravity_drop() {
        let (distance, height, speed, gravity) = (300.0, 15.0, 200.0, 9.81);
        let pitch = elevation(distance, height, speed, gravity);
        let seconds = distance / (speed * pitch.cos());
        let impact = speed * pitch.sin() * seconds - 0.5 * gravity * seconds * seconds;
        assert!((impact - height).abs() < 0.001);
        assert!(pitch > elevation(distance, height, speed, 0.0));
    }
}
