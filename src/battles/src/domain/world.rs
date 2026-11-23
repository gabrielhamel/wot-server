use glam::{Vec2, Vec3, Vec3Swizzles};

pub(crate) const CONTACT_SKIN: f32 = 0.025;
pub(crate) const HULL_HEIGHT: f32 = 2.0;

#[derive(Clone, Copy)]
pub(crate) struct Hull {
    pub position: Vec3,
    pub yaw: f32,
    pub half_size: Vec2,
    pub offset: Vec2,
}

impl Hull {
    fn axes(self) -> [Vec2; 2] {
        let (sin, cos) = self.yaw.sin_cos();
        [Vec2::new(cos, -sin), Vec2::new(sin, cos)]
    }

    fn center(self) -> Vec3 {
        let [right, forward] = self.axes();
        let offset = right * self.offset.x + forward * self.offset.y;
        self.position + Vec3::new(offset.x, 0.0, offset.y)
    }

    pub fn bounds(self) -> (Vec3, Vec3) {
        let [right, forward] = self.axes();
        let extent = right.abs() * self.half_size.x + forward.abs() * self.half_size.y;
        (
            self.center() - Vec3::new(extent.x, 0.0, extent.y),
            self.center() + Vec3::new(extent.x, HULL_HEIGHT, extent.y),
        )
    }

    fn radius(self, axis: Vec2) -> f32 {
        let [right, forward] = self.axes();
        right.dot(axis).abs() * self.half_size.x + forward.dot(axis).abs() * self.half_size.y
    }

    pub fn sweep_triangle(self, vertices: [Vec3; 3], travel: Vec2) -> Option<Contact> {
        let (contact, overlap) = self.triangle_contact(vertices, travel)?;
        let blocks = if travel == Vec2::ZERO {
            overlap > CONTACT_SKIN
        } else {
            travel.dot(contact.normal) < -0.0001
        };
        blocks.then_some(contact)
    }

    pub fn triangle_overlap(self, vertices: [Vec3; 3]) -> Option<(Vec2, f32)> {
        let (contact, depth) = self.triangle_contact(vertices, Vec2::ZERO)?;
        Some((contact.normal, depth))
    }

    // The box is expressed in hull coordinates so each triangle can be swept
    // against it with the box axes, face normal and edge cross products.
    fn triangle_contact(self, vertices: [Vec3; 3], travel: Vec2) -> Option<(Contact, f32)> {
        let [right, forward] = self.axes();
        let to_local = |v: Vec3| Vec3::new(v.xz().dot(right), v.y, v.xz().dot(forward));
        let center = self.center() + Vec3::Y * (HULL_HEIGHT * 0.5);
        let [a, b, c] = vertices.map(|v| to_local(v - center));
        let edges = [b - a, c - b, a - c];
        let face = edges[0].cross(edges[1]).normalize_or_zero();
        if face == Vec3::ZERO || face.y.abs() > 0.65 {
            return None;
        }
        let half = Vec3::new(self.half_size.x, HULL_HEIGHT * 0.5, self.half_size.y);
        let velocity = to_local(Vec3::new(travel.x, 0.0, travel.y));
        let mut sweep = Sweep::new();
        let mut horizontal_overlap = f32::INFINITY;
        let mut horizontal_normal = Vec3::ZERO;
        let box_axes = [Vec3::X, Vec3::Y, Vec3::Z];
        let cross_axes = edges
            .into_iter()
            .flat_map(|edge| box_axes.map(|axis| edge.cross(axis)));
        for axis in box_axes.into_iter().chain([face]).chain(cross_axes) {
            let axis = axis.normalize_or_zero();
            if axis == Vec3::ZERO {
                continue;
            }
            let [first, second, third] = [a.dot(axis), b.dot(axis), c.dot(axis)];
            let radius = half.dot(axis.abs());
            let low = first.min(second).min(third) - radius;
            let high = first.max(second).max(third) + radius;
            if !sweep.clip(low, high, velocity.dot(axis), axis) {
                return None;
            }
            if travel == Vec2::ZERO {
                // Vertical separation is already checked by SAT. A pivot can only
                // resolve a contact by moving the hull horizontally.
                let horizontal = axis.xz().length();
                if horizontal > 1e-4 {
                    let depth = (-low).min(high) / horizontal;
                    if depth < horizontal_overlap {
                        horizontal_overlap = depth;
                        horizontal_normal = if -low < high { -axis } else { axis };
                    }
                }
            }
        }
        let (fraction, local_normal, overlap) = if travel == Vec2::ZERO {
            (0.0, horizontal_normal, horizontal_overlap)
        } else {
            sweep.finish()?
        };
        let normal = (right * local_normal.x + forward * local_normal.z).normalize_or_zero();
        (normal != Vec2::ZERO).then_some((Contact { fraction, normal }, overlap))
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Contact {
    pub fraction: f32,
    /// Points out from the obstacle towards the moving hull.
    pub normal: Vec2,
}

pub(crate) fn vehicle_contact(
    first: Hull,
    other: Hull,
    first_travel: Vec2,
    other_travel: Vec2,
) -> Option<(Contact, f32)> {
    if (first.position.y - other.position.y).abs() >= HULL_HEIGHT {
        return None;
    }
    let displacement = other.center().xz() - first.center().xz();
    let relative_travel = first_travel - other_travel;
    let reach = first.half_size.length() + other.half_size.length() + CONTACT_SKIN;
    if displacement
        .abs()
        .cmpgt(relative_travel.abs() + Vec2::splat(reach))
        .any()
    {
        return None;
    }
    let mut sweep = Sweep::new();
    for axis in first.axes().into_iter().chain(other.axes()) {
        let center = displacement.dot(axis);
        let radius = first.radius(axis) + other.radius(axis) + CONTACT_SKIN;
        if !sweep.clip(
            center - radius,
            center + radius,
            relative_travel.dot(axis),
            Vec3::new(axis.x, 0.0, axis.y),
        ) {
            return None;
        }
    }
    let (fraction, normal, overlap) = sweep.finish()?;
    let normal = normal.xz();
    let correction = overlap - relative_travel.dot(normal) * (1.0 - fraction);
    (correction > 0.0001).then_some((Contact { fraction, normal }, correction))
}

struct Sweep {
    enter: f32,
    exit: f32,
    entry_normal: Vec3,
    overlap: f32,
    overlap_normal: Vec3,
}

impl Sweep {
    fn new() -> Self {
        Self {
            enter: f32::NEG_INFINITY,
            exit: 1.0,
            entry_normal: Vec3::ZERO,
            overlap: f32::INFINITY,
            overlap_normal: Vec3::ZERO,
        }
    }

    // The moving center starts at zero. The interval already includes its extent.
    fn clip(&mut self, low: f32, high: f32, speed: f32, axis: Vec3) -> bool {
        let (overlap, normal) = if -low < high {
            (-low, -axis)
        } else {
            (high, axis)
        };
        if overlap < self.overlap {
            self.overlap = overlap;
            self.overlap_normal = normal;
        }
        if speed.abs() < 1e-6 {
            return low <= 0.0 && high >= 0.0;
        }
        let a = low / speed;
        let b = high / speed;
        let enter = a.min(b);
        if enter > self.enter {
            self.enter = enter;
            self.entry_normal = -axis * speed.signum();
        }
        self.exit = self.exit.min(a.max(b));
        self.enter <= self.exit && self.exit >= 0.0
    }

    fn finish(self) -> Option<(f32, Vec3, f32)> {
        if self.enter >= 0.0 {
            Some((self.enter, self.entry_normal, 0.0))
        } else if self.overlap.is_finite() {
            Some((0.0, self.overlap_normal, self.overlap.max(0.0)))
        } else {
            None
        }
    }
}

const MAX_PASSIVE_SPEED: f32 = 12.0;
const CONTACT_DRAG: f32 = 4.0;
const TIP_TERRAIN_ROLL: f32 = 1.0;
const TIP_SIDE_SPEED: f32 = 5.5;
const RIGHTING_SIDE_SPEED: f32 = 1.5;
const SETTLE_TIME_SECONDS: f32 = 3.0;
const RECOVERY_TIME_SECONDS: f32 = 15.0;

#[derive(Default)]
pub(crate) struct WorldMotion {
    velocity: Vec2,
    tipped: Option<Tipped>,
}

struct Tipped {
    side: f32,
    idle_seconds: f32,
}

impl WorldMotion {
    pub fn is_tipped(&self) -> bool {
        self.tipped.is_some()
    }

    pub fn advance(&mut self, yaw: f32, tilt: Vec2, grip: f32, seconds: f32) -> (Vec2, bool) {
        let (sin, cos) = yaw.sin_cos();
        let right = Vec2::new(cos, -sin);
        let forward = Vec2::new(sin, cos);
        let descent = -right * tilt.x.sin() + forward * tilt.y.sin();
        let gravity = descent * super::movement::GRAVITY;
        let excess = gravity.length() - grip * super::movement::GRAVITY;
        if excess > 0.0 {
            self.velocity += gravity.normalize_or_zero() * excess * seconds;
        } else {
            let speed = self.velocity.length();
            self.velocity =
                self.velocity.normalize_or_zero() * (speed - CONTACT_DRAG * seconds).max(0.0);
        }
        self.velocity = self.velocity.clamp_length_max(MAX_PASSIVE_SPEED);
        (self.velocity * seconds, excess > 0.0)
    }

    pub fn block(&mut self, normal: Vec2) {
        self.velocity -= normal * self.velocity.dot(normal).min(0.0);
    }

    pub fn push(&mut self, impulse: Vec2) {
        self.velocity = (self.velocity + impulse).clamp_length_max(MAX_PASSIVE_SPEED);
    }

    pub fn stop(&mut self) {
        self.velocity = Vec2::ZERO;
    }

    pub fn attitude(
        &mut self,
        support_roll: f32,
        side_speed: f32,
        stationary: bool,
        seconds: f32,
    ) -> f32 {
        if let Some(tipped) = &mut self.tipped {
            if stationary {
                tipped.idle_seconds += seconds;
            } else {
                tipped.idle_seconds = 0.0;
            }
            if support_roll.abs() < TIP_TERRAIN_ROLL * 0.5
                && (side_speed * tipped.side >= RIGHTING_SIDE_SPEED
                    || tipped.idle_seconds >= SETTLE_TIME_SECONDS + RECOVERY_TIME_SECONDS)
            {
                self.tipped = None;
                return support_roll;
            }
            return std::f32::consts::FRAC_PI_2 * tipped.side;
        }

        let tilt = support_roll - side_speed / TIP_SIDE_SPEED;
        if tilt.abs() >= TIP_TERRAIN_ROLL
            && (support_roll.abs() > 0.3 || side_speed.abs() >= TIP_SIDE_SPEED)
        {
            let side = tilt.signum();
            self.tipped = Some(Tipped {
                side,
                idle_seconds: 0.0,
            });
            std::f32::consts::FRAC_PI_2 * side
        } else {
            support_roll
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hull(x: f32, y: f32, yaw: f32) -> Hull {
        Hull {
            position: Vec3::new(x, y, 0.0),
            yaw,
            half_size: Vec2::new(1.0, 2.0),
            offset: Vec2::ZERO,
        }
    }

    #[test]
    fn reverse_sweep_stops_the_rear_overhang_at_a_wall() {
        let mut body = hull(0.0, 0.0, 0.0);
        body.position.z = 4.0;
        body.half_size = Vec2::new(1.55, 3.264);
        body.offset = Vec2::new(0.0, -0.064);
        let wall = [
            Vec3::new(-6.0, -1.0, 0.0),
            Vec3::new(6.0, -1.0, 0.0),
            Vec3::new(0.0, 6.0, 0.0),
        ];
        let contact = body.sweep_triangle(wall, -Vec2::Y * 2.0).unwrap();
        assert!((contact.fraction - (4.0 - 3.328) / 2.0).abs() < 0.001);
        assert_eq!(contact.normal, Vec2::Y);
    }
    #[test]
    fn swept_hulls_resolve_fast_crossings_and_allow_separation() {
        let a = hull(0.0, 0.0, 0.0);
        let b = hull(6.0, 0.0, std::f32::consts::FRAC_PI_2);
        let travel = Vec2::new(10.0, 0.0);
        let (contact, correction) = vehicle_contact(a, b, travel, Vec2::ZERO).unwrap();
        assert!((contact.fraction - (3.0 - CONTACT_SKIN) / 10.0).abs() < 1e-5);
        assert_eq!(contact.normal, -Vec2::X);
        assert!((correction - (7.0 + CONTACT_SKIN)).abs() < 1e-5);
        assert!(vehicle_contact(a, hull(1.5, 0.0, 0.0), -travel, Vec2::ZERO).is_none());
        assert!(vehicle_contact(a, hull(6.0, HULL_HEIGHT, 0.0), travel, Vec2::ZERO).is_none());
    }

    #[test]
    fn hull_sweep_hits_wall_faces_and_ignores_floor_and_overhead() {
        let body = hull(0.0, 0.0, 0.0);
        let wall = [
            Vec3::new(5.0, -1.0, -6.0),
            Vec3::new(5.0, 4.0, -6.0),
            Vec3::new(5.0, 1.0, 6.0),
        ];
        let contact = body.sweep_triangle(wall, Vec2::X * 12.0).unwrap();
        assert_eq!(contact.normal, -Vec2::X);
        assert!((contact.fraction - 1.0 / 3.0).abs() < 1e-5);
        assert!(
            body.sweep_triangle(wall.map(|v| v + Vec3::Y * 8.0), Vec2::X * 12.0)
                .is_none()
        );
        let floor = [Vec3::ZERO, Vec3::X * 12.0, Vec3::Z * 12.0];
        assert!(body.sweep_triangle(floor, Vec2::X).is_none());
    }

    #[test]
    fn slopes_slide_downhill_and_recovery_restarts_after_motion() {
        let mut motion = WorldMotion::default();
        let (stable, sliding) = motion.advance(0.0, Vec2::new(0.1, 0.0), 0.5, 0.1);
        assert!(!sliding);
        assert_eq!(stable, Vec2::ZERO);
        let (slide, sliding) = motion.advance(0.0, Vec2::new(0.8, 0.0), 0.5, 0.1);
        assert!(sliding);
        assert!(slide.x < 0.0 && slide.y == 0.0);
        let (flat, _) = motion.advance(0.0, Vec2::ZERO, 0.5, 1.0);
        assert_eq!(flat, Vec2::ZERO);
        motion.attitude(1.05, 0.0, false, 0.1);
        assert!(motion.is_tipped());
        motion.attitude(0.0, 0.0, true, 17.0);
        motion.attitude(0.0, 0.0, false, 0.1);
        motion.attitude(0.0, 0.0, true, 17.0);
        assert!(motion.is_tipped());
        motion.attitude(0.0, 0.0, true, 1.0);
        assert!(!motion.is_tipped());

        motion.attitude(0.0, 7.0, false, 0.1);
        assert!(motion.is_tipped());
        motion.attitude(0.0, -2.0, false, 0.1);
        assert!(!motion.is_tipped());
    }
}
