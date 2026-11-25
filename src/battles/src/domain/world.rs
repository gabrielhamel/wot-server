use glam::{EulerRot, Quat, Vec2, Vec3, Vec3Swizzles};

use super::parameters::BattleParameters;

#[derive(Clone, Copy)]
pub(crate) struct HullParameters {
    pub contact_skin_metres: f32,
    height_metres: f32,
    wall_normal_y_limit: f32,
}

impl From<&BattleParameters> for HullParameters {
    fn from(parameters: &BattleParameters) -> Self {
        Self {
            contact_skin_metres: parameters.contact_skin_metres,
            height_metres: parameters.hull_height_metres,
            wall_normal_y_limit: parameters.scenery_wall_normal_y_limit,
        }
    }
}

#[cfg(test)]
impl Default for HullParameters {
    fn default() -> Self {
        Self::from(&BattleParameters::default())
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Hull {
    pub position: Vec3,
    pub yaw: f32,
    pub half_size: Vec2,
    pub offset: Vec2,
    pub body: Option<VehicleBody>,
    pub parameters: HullParameters,
}

#[derive(Clone, Copy)]
pub(crate) struct VehicleBody {
    pub origin: Vec3,
    pub tilt: Vec2,
    pub center: Vec3,
    pub half_size: Vec3,
}

struct ContactBox {
    center: Vec3,
    half_size: Vec3,
    axes: [Vec3; 3],
}

impl ContactBox {
    fn radius(&self, axis: Vec3) -> f32 {
        let projections = Vec3::from_array(self.axes.map(|basis| basis.dot(axis).abs()));
        self.half_size.dot(projections)
    }
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
            self.center() + Vec3::new(extent.x, self.parameters.height_metres, extent.y),
        )
    }

    fn contact_box(self) -> ContactBox {
        let body = self.body.unwrap_or(VehicleBody {
            origin: self.position,
            tilt: Vec2::ZERO,
            center: Vec3::new(
                self.offset.x,
                self.parameters.height_metres * 0.5,
                self.offset.y,
            ),
            half_size: Vec3::new(
                self.half_size.x,
                self.parameters.height_metres * 0.5,
                self.half_size.y,
            ),
        });
        let rotation = Quat::from_euler(EulerRot::YXZ, self.yaw, body.tilt.y, body.tilt.x);
        ContactBox {
            center: body.origin + rotation * body.center,
            half_size: body.half_size,
            axes: [Vec3::X, Vec3::Y, Vec3::Z].map(|axis| rotation * axis),
        }
    }

    pub fn sweep_triangle(self, vertices: [Vec3; 3], travel: Vec2) -> Option<Contact> {
        let (contact, overlap) = self.triangle_contact(vertices, travel)?;
        let blocks = if travel == Vec2::ZERO {
            overlap > self.parameters.contact_skin_metres
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
        let center = self.center() + Vec3::Y * (self.parameters.height_metres * 0.5);
        let [a, b, c] = vertices.map(|v| to_local(v - center));
        let edges = [b - a, c - b, a - c];
        let face = edges[0].cross(edges[1]).normalize_or_zero();
        if face == Vec3::ZERO || face.y.abs() > self.parameters.wall_normal_y_limit {
            return None;
        }
        let half = Vec3::new(
            self.half_size.x,
            self.parameters.height_metres * 0.5,
            self.half_size.y,
        );
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
    let first = first.contact_box();
    let other = other.contact_box();
    let displacement = other.center - first.center;
    let relative_travel = first_travel - other_travel;
    let travel = Vec3::new(relative_travel.x, 0.0, relative_travel.y);
    let reach = first.half_size.length() + other.half_size.length();
    if displacement
        .abs()
        .cmpgt(travel.abs() + Vec3::splat(reach))
        .any()
    {
        return None;
    }
    let cross_axes = first
        .axes
        .into_iter()
        .flat_map(|axis| other.axes.map(|basis| axis.cross(basis)));
    let mut sweep = Sweep::new();
    for axis in first.axes.into_iter().chain(other.axes).chain(cross_axes) {
        if axis.length_squared() < 1e-8 {
            continue;
        }
        let axis = axis.normalize();
        let center = displacement.dot(axis);
        let radius = first.radius(axis) + other.radius(axis);
        let horizontal = axis.xz().length();
        if horizontal < 1e-4 {
            if center.abs() >= radius {
                return None;
            }
            continue;
        }
        // Measure penetration along the horizontal correction the solver can apply.
        let planar_axis = Vec3::new(axis.x, 0.0, axis.z) / horizontal;
        if !sweep.clip(
            (center - radius) / horizontal,
            (center + radius) / horizontal,
            travel.dot(planar_axis),
            planar_axis,
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

    pub fn advance(
        &mut self,
        yaw: f32,
        tilt: Vec2,
        grip: f32,
        seconds: f32,
        parameters: &BattleParameters,
    ) -> (Vec2, bool) {
        let (sin, cos) = yaw.sin_cos();
        let right = Vec2::new(cos, -sin);
        let forward = Vec2::new(sin, cos);
        let descent = -right * tilt.x.sin() + forward * tilt.y.sin();
        let gravity = descent * parameters.gravity_mps2;
        let excess = gravity.length() - grip * parameters.gravity_mps2;
        if excess > 0.0 {
            self.velocity += gravity.normalize_or_zero() * excess * seconds;
        } else {
            let speed = self.velocity.length();
            self.velocity = self.velocity.normalize_or_zero()
                * (speed - parameters.contact_drag_mps2 * seconds).max(0.0);
        }
        self.velocity = self
            .velocity
            .clamp_length_max(parameters.max_passive_speed_mps);
        (self.velocity * seconds, excess > 0.0)
    }

    pub fn block(&mut self, normal: Vec2) {
        self.velocity -= normal * self.velocity.dot(normal).min(0.0);
    }

    pub fn push(&mut self, impulse: Vec2, parameters: &BattleParameters) {
        self.velocity =
            (self.velocity + impulse).clamp_length_max(parameters.max_passive_speed_mps);
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
        parameters: &BattleParameters,
    ) -> f32 {
        if let Some(tipped) = &mut self.tipped {
            if stationary {
                tipped.idle_seconds += seconds;
            } else {
                tipped.idle_seconds = 0.0;
            }
            if support_roll.abs() < parameters.tip_terrain_roll_radians * 0.5
                && (side_speed * tipped.side >= parameters.righting_side_speed_mps
                    || tipped.idle_seconds
                        >= parameters.settle_time_seconds + parameters.recovery_time_seconds)
            {
                self.tipped = None;
                return support_roll;
            }
            return std::f32::consts::FRAC_PI_2 * tipped.side;
        }

        let tilt = support_roll - side_speed / parameters.tip_side_speed_mps;
        if tilt.abs() >= parameters.tip_terrain_roll_radians
            && (support_roll.abs() > parameters.tip_min_support_roll_radians
                || side_speed.abs() >= parameters.tip_side_speed_mps)
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
            parameters: Default::default(),
            position: Vec3::new(x, y, 0.0),
            yaw,
            half_size: Vec2::new(1.0, 2.0),
            offset: Vec2::ZERO,
            body: None,
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
        assert!((contact.fraction - 3.0 / 10.0).abs() < 1e-5);
        assert_eq!(contact.normal, -Vec2::X);
        assert!((correction - 7.0).abs() < 1e-5);
        assert!(vehicle_contact(a, hull(1.5, 0.0, 0.0), -travel, Vec2::ZERO).is_none());
        assert!(
            vehicle_contact(
                a,
                hull(6.0, BattleParameters::default().hull_height_metres, 0.0),
                travel,
                Vec2::ZERO
            )
            .is_none()
        );
    }

    #[test]
    fn pitched_amx_30_b_bodies_close_to_their_end_faces_on_a_slope() {
        let min = Vec3::new(-1.55, 0.01, -3.328);
        let max = Vec3::new(1.55, 1.658, 3.200);
        let pitch = -0.3;
        let body = VehicleBody {
            origin: Vec3::ZERO,
            tilt: Vec2::new(0.0, pitch),
            center: (min + max) * 0.5,
            half_size: (max - min) * 0.5,
        };
        let first = Hull {
            body: Some(body),
            ..hull(0.0, 0.0, 0.0)
        };
        let touch = Quat::from_rotation_x(pitch) * Vec3::Z * (max.z - min.z);
        let origin = touch + Vec3::Z * 0.05;
        let second = Hull {
            position: origin,
            body: Some(VehicleBody { origin, ..body }),
            ..first
        };
        assert!(vehicle_contact(first, second, Vec2::Y * 0.04, Vec2::ZERO).is_none());

        let (hit, correction) = vehicle_contact(first, second, Vec2::Y * 0.1, Vec2::ZERO).unwrap();
        assert!((hit.fraction - 0.5).abs() < 1e-4);
        assert!((correction - 0.05).abs() < 1e-4);
        assert!(hit.normal.abs_diff_eq(-Vec2::Y, 1e-5));
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
        let (stable, sliding) = motion.advance(
            0.0,
            Vec2::new(0.1, 0.0),
            0.5,
            0.1,
            &BattleParameters::default(),
        );
        assert!(!sliding);
        assert_eq!(stable, Vec2::ZERO);
        let (slide, sliding) = motion.advance(
            0.0,
            Vec2::new(0.8, 0.0),
            0.5,
            0.1,
            &BattleParameters::default(),
        );
        assert!(sliding);
        assert!(slide.x < 0.0 && slide.y == 0.0);
        let (flat, _) = motion.advance(0.0, Vec2::ZERO, 0.5, 1.0, &BattleParameters::default());
        assert_eq!(flat, Vec2::ZERO);
        motion.attitude(1.05, 0.0, false, 0.1, &BattleParameters::default());
        assert!(motion.is_tipped());
        motion.attitude(0.0, 0.0, true, 17.0, &BattleParameters::default());
        motion.attitude(0.0, 0.0, false, 0.1, &BattleParameters::default());
        motion.attitude(0.0, 0.0, true, 17.0, &BattleParameters::default());
        assert!(motion.is_tipped());
        motion.attitude(0.0, 0.0, true, 1.0, &BattleParameters::default());
        assert!(!motion.is_tipped());

        motion.attitude(0.0, 7.0, false, 0.1, &BattleParameters::default());
        assert!(motion.is_tipped());
        motion.attitude(0.0, -2.0, false, 0.1, &BattleParameters::default());
        assert!(!motion.is_tipped());
    }
}
