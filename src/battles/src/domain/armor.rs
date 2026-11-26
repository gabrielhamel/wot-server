use super::{
    devices::{DEVICE_STRIKE_CAPACITY, DeviceStrike, TRACKS},
    parameters::BattleParameters,
    vehicle::{Shot, Tank},
};
use glam::Vec3;
use wot_shared_kernel::{ArmorPlate, ShellKind, shell_value_at_distance};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ArmorOutcome {
    Penetrated,
    Blocked,
    Ricochet,
    ScreenPenetrated,
    ScreenBlocked,
    Spall,
}

impl ArmorOutcome {
    pub fn name(self) -> &'static str {
        match self {
            Self::Penetrated => "penetrated",
            Self::Blocked => "armor_blocked",
            Self::Ricochet => "ricochet",
            Self::ScreenPenetrated => "screen_penetrated",
            Self::ScreenBlocked => "screen_blocked",
            Self::Spall => "spall",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ImpactSegment {
    pub part: u8,
    pub start: [u8; 3],
    pub end: [u8; 3],
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ArmorHit {
    pub distance: f32,
    pub segment: ImpactSegment,
    pub material: u32,
    pub outcome: ArmorOutcome,
    pub damage: u16,
    reflected: Option<Vec3>,
    pub devices: [Option<DeviceStrike>; DEVICE_STRIKE_CAPACITY],
    pub thickness_mm: f32,
    effective_mm: f32,
    remaining_mm: f32,
}

pub(crate) struct ArmorPath(Vec<Surface>);

impl ArmorPath {
    pub fn first_distance(&self) -> f32 {
        self.0
            .first()
            .map_or(f32::INFINITY, |surface| surface.distance)
    }
}

#[derive(Clone, Copy)]
struct Surface {
    distance: f32,
    segment: ImpactSegment,
    material: u32,
    plate: ArmorPlate,
    cosine: f32,
    normal: Vec3,
    track_damage_factor: f32,
}

impl Shot {
    #[tracing::instrument(name = "battles.resolve_armor", skip_all, fields(
        ddd.bounded_context.name = "battles", input.target.entity_id = target_id,
        input.shell.caliber_mm = self.ballistics.caliber_mm,
        input.shell.range_metres = self.max_distance,
        output.part.index = tracing::field::Empty, output.material.kind = tracing::field::Empty,
        output.plate.thickness_mm = tracing::field::Empty, output.plate.effective_mm = tracing::field::Empty,
        output.penetration.remaining_mm = tracing::field::Empty, output.damage = tracing::field::Empty,
        output.tracks.left_damage_hp = tracing::field::Empty,
        output.tracks.right_damage_hp = tracing::field::Empty,
        output.outcome = tracing::field::Empty,
    ))]
    pub fn resolve_armor(
        &self,
        target_id: u32,
        path: ArmorPath,
        parameters: &BattleParameters,
    ) -> Option<ArmorHit> {
        let hit = self.resolve_surfaces(&path.0, parameters);
        if let Some(hit) = hit {
            let span = tracing::Span::current();
            span.record("output.part.index", hit.segment.part);
            span.record("output.material.kind", hit.material);
            span.record("output.plate.thickness_mm", hit.thickness_mm);
            span.record("output.plate.effective_mm", hit.effective_mm);
            span.record("output.penetration.remaining_mm", hit.remaining_mm);
            span.record("output.damage", hit.damage);
            for (track, attribute) in TRACKS.into_iter().zip([
                "output.tracks.left_damage_hp",
                "output.tracks.right_damage_hp",
            ]) {
                if let Some(strike) = hit
                    .devices
                    .iter()
                    .flatten()
                    .find(|strike| strike.device == track)
                {
                    span.record(attribute, strike.damage);
                }
            }
            span.record("output.outcome", hit.outcome.name());
        } else {
            tracing::Span::current().record("output.outcome", "miss");
        }
        hit
    }

    pub fn penetration_at(&self, distance: f32) -> f32 {
        let penetration =
            (shell_value_at_distance(self.ballistics.penetration_mm, self.travelled + distance)
                * self.penetration_roll
                - self.penetration_loss)
                .max(0.0);
        if self.heat_jet
            && let ShellKind::HollowCharge { jet_loss_per_metre } = self.ballistics.kind
        {
            return penetration * (1.0 - distance * jet_loss_per_metre).max(0.0);
        }
        penetration
    }

    pub fn continue_from(
        self,
        distance: f32,
        direction: Vec3,
        parameters: &BattleParameters,
    ) -> Option<Self> {
        let advance = distance + parameters.projectile_contact_clearance_metres;
        let max_distance = self.max_distance - advance;
        (max_distance > 0.0).then_some(Self {
            origin: self.origin
                + self.direction * distance
                + direction * parameters.projectile_contact_clearance_metres,
            travelled: self.travelled + advance,
            max_distance,
            direction,
            ..self
        })
    }

    pub fn through_screen(self, hit: ArmorHit, parameters: &BattleParameters) -> Option<Self> {
        if hit.outcome != ArmorOutcome::ScreenPenetrated || hit.remaining_mm <= 0.0 {
            return None;
        }
        let mut next = self.continue_from(hit.distance, self.direction, parameters)?;
        let unspent = shell_value_at_distance(self.ballistics.penetration_mm, next.travelled)
            * self.penetration_roll;
        next.penetration_loss = (unspent - hit.remaining_mm).max(0.0);
        next.heat_jet = matches!(self.ballistics.kind, ShellKind::HollowCharge { .. });
        Some(next)
    }

    pub fn ricochet_from(self, hit: ArmorHit, parameters: &BattleParameters) -> Option<Self> {
        let mut shot = self.continue_from(hit.distance, hit.reflected?, parameters)?;
        let reflected_range = self.ballistics.caliber_mm
            * (parameters.ricochet_range_calibers / 1000.0)
            - parameters.projectile_contact_clearance_metres;
        shot.max_distance = shot.max_distance.min(reflected_range);
        let retention = if matches!(self.ballistics.kind, ShellKind::ArmorPiercing) {
            parameters.ap_ricochet_penetration_retention
        } else {
            1.0
        };
        shot.penetration_loss +=
            (self.penetration_at(hit.distance) - hit.remaining_mm * retention).max(0.0);
        shot.ricocheted = true;
        Some(shot)
    }

    pub fn armor_path(&self, target: &Tank, range: f32) -> Option<ArmorPath> {
        let mut surfaces = Vec::new();
        let profile = target.performance.as_ref()?;
        for (index, mounted) in profile.armor.iter().enumerate() {
            let Some(mounted) = mounted else {
                continue;
            };
            let (origin, rotation) = target.armor_pose(index, mounted.offset);
            let origin = rotation.inverse() * (self.origin - origin);
            let direction = rotation.inverse() * self.direction;
            let bounds = mounted.mesh.bounds;
            let Some([near, far]) = clip_ray(bounds, origin, direction) else {
                continue;
            };
            if near > range || far < 0.0 {
                continue;
            }
            for triangle in &mounted.mesh.triangles {
                let [a, b, c] = triangle.vertices;
                let Some(distance) = triangle_distance(a, b, c, origin, direction)
                    .filter(|&distance| distance <= range)
                else {
                    continue;
                };
                let normal = (b - a).cross(c - a).normalize_or_zero();
                let segment =
                    impact_segment(index as u8, bounds, origin, direction, near, far, distance);
                let track_damage_factor = profile.mobility.track_damage.map_or(1.0, |tracks| {
                    let [rear, front] = tracks.protected_z;
                    let z = origin.z + direction.z * distance;
                    if index == 0 && (rear..=front).contains(&z) {
                        tracks.bulk_health_factor.recip()
                    } else {
                        1.0
                    }
                });
                surfaces.push(Surface {
                    distance,
                    material: triangle.material,
                    plate: triangle.plate,
                    cosine: normal.dot(direction).abs().clamp(0.0, 1.0),
                    normal: rotation * normal,
                    track_damage_factor,
                    segment,
                });
            }
        }
        surfaces.sort_unstable_by(|a, b| {
            a.distance
                .total_cmp(&b.distance)
                .then(a.segment.part.cmp(&b.segment.part))
                .then(a.material.cmp(&b.material))
        });
        surfaces.dedup_by(|a, b| {
            a.segment.part == b.segment.part
                && a.material == b.material
                && (a.distance - b.distance).abs() < 0.001
        });
        (!surfaces.is_empty()).then_some(ArmorPath(surfaces))
    }

    pub fn exposed_armor(&self, target: &Tank, range: f32) -> Option<(f32, f32)> {
        self.armor_path(target, range)?
            .0
            .into_iter()
            .find(|contact| contact.plate.damage_factor > 0.0)
            .map(|contact| (contact.distance, contact.plate.thickness_mm))
    }

    pub fn damage_at(&self, distance: f32) -> f32 {
        shell_value_at_distance(
            [f32::from(self.damage), self.ballistics.far_damage],
            self.travelled + distance,
        ) * self.damage_roll
    }

    fn resolve_surfaces(
        &self,
        surfaces: &[Surface],
        parameters: &BattleParameters,
    ) -> Option<ArmorHit> {
        let shell = self.ballistics;
        let first = surfaces.first()?;
        let mut penetration = self.penetration_at(first.distance);
        let mut last_hit = None;
        let mut jet_exit: Option<f32> = None;
        let mut seen = Vec::new();
        let mut devices = [None; DEVICE_STRIKE_CAPACITY];
        for (index, &surface) in surfaces.iter().enumerate() {
            let Surface {
                distance,
                segment,
                material,
                plate,
                cosine,
                normal,
                ..
            } = surface;
            let material_key = (segment.part, material);
            if seen.contains(&material_key) {
                continue;
            }
            let thickness = plate.thickness_mm;
            let ratio = shell.caliber_mm / thickness;
            let ap = matches!(shell.kind, ShellKind::ArmorPiercing);
            let heat = matches!(shell.kind, ShellKind::HollowCharge { .. });
            let cosine = if plate.angled { cosine } else { 1.0 };
            if let (ShellKind::HollowCharge { jet_loss_per_metre }, Some(exit)) =
                (shell.kind, jet_exit)
            {
                penetration *= (1.0 - (distance - exit).max(0.0) * jet_loss_per_metre).max(0.0);
            }
            let mut hit = ArmorHit {
                distance,
                segment,
                material,
                thickness_mm: thickness,
                effective_mm: thickness,
                remaining_mm: penetration,
                outcome: ArmorOutcome::Blocked,
                damage: 0,
                reflected: None,
                devices,
            };
            if (ap || heat)
                && jet_exit.is_none()
                && !self.heat_jet
                && plate.ricochets
                && thickness > 0.0
                && cosine <= shell.ricochet_cos
                && (!ap
                    || !plate.caliber_ricochet
                    || ratio <= parameters.ap_ricochet_overmatch_ratio)
            {
                hit.outcome = ArmorOutcome::Ricochet;
                if !self.ricocheted && shell.trace_ricochet {
                    hit.reflected = Some(
                        (self.direction - 2.0 * self.direction.dot(normal) * normal)
                            .normalize_or_zero(),
                    );
                }
                return Some(hit);
            }
            let effective = self.effective_thickness(surface, parameters);
            hit.effective_mm = effective;
            if plate.damage_factor > 0.0 {
                if penetration >= effective && !matches!(shell.kind, ShellKind::Other) {
                    hit.outcome = ArmorOutcome::Penetrated;
                    let internal_range =
                        shell.caliber_mm * (parameters.internal_projectile_range_calibers / 1000.0);
                    let mut internal_penetration = penetration - effective;
                    for internal in &surfaces[index + 1..] {
                        if internal.distance - distance > internal_range
                            || internal.plate.damage_factor > 0.0
                            || internal_penetration < internal.plate.thickness_mm
                        {
                            break;
                        }
                        if ap && is_track(*internal) {
                            break;
                        }
                        self.record_device(
                            *internal,
                            false,
                            &mut hit.devices,
                            parameters.max_device_hits,
                        );
                        internal_penetration -= internal.plate.thickness_mm;
                    }
                    hit.remaining_mm = penetration - effective;
                    let onward = &surfaces[index + 1..];
                    if ap && onward.iter().copied().any(is_track) {
                        self.strike_tracks_beyond_hull(
                            surface, onward, &mut seen, &mut hit, parameters,
                        );
                    }
                    hit.damage = rolled_damage(
                        shell_value_at_distance(
                            [f32::from(self.damage), shell.far_damage],
                            self.travelled + distance,
                        ),
                        self.damage_roll,
                        plate.damage_factor,
                    );
                } else if let Some(he) = shell.explosive {
                    let travel = self.travelled + distance;
                    let spall = shell_value_at_distance(he.spall_damage, travel);
                    let fragments = (shell_value_at_distance(he.fragment_damage, travel)
                        - thickness * he.fragment_absorption)
                        .max(0.0);
                    if spall > 0.0 || fragments > 0.0 {
                        hit.outcome = ArmorOutcome::Spall;
                        // The current client supplies damage curves and absorption factors,
                        // but the server's spall transfer function is not in its scripts.
                        let spall = spall * shell.penetration_mm[0]
                            / (shell.penetration_mm[0] + thickness).max(1.0);
                        hit.damage = rolled_damage(
                            spall.max(fragments),
                            self.damage_roll,
                            plate.damage_factor,
                        )
                        .max(1);
                    }
                }
                if hit.outcome == ArmorOutcome::Spall
                    && let Some(he) = shell.explosive
                    && surface.cosine >= he.spall_cone_cos
                {
                    for internal in &surfaces[index + 1..] {
                        if internal.distance - distance > he.spall_radius
                            || internal.plate.damage_factor > 0.0
                        {
                            break;
                        }
                        self.record_device(
                            *internal,
                            true,
                            &mut hit.devices,
                            parameters.max_device_hits,
                        );
                        if !he.piercing_spalls && internal.plate.thickness_mm > 0.0 {
                            break;
                        }
                    }
                }
                return Some(hit);
            }
            let loss_factor = match shell.kind {
                ShellKind::HighExplosive {
                    shield_penetration: true,
                    ..
                } => parameters.he_screen_penetration_loss_factor,
                ShellKind::HighExplosive {
                    shield_penetration: false,
                    ..
                } => {
                    hit.outcome = ArmorOutcome::ScreenBlocked;
                    return Some(hit);
                }
                _ => 1.0,
            };
            if penetration < effective || matches!(shell.kind, ShellKind::Other) {
                hit.outcome = ArmorOutcome::ScreenBlocked;
                return Some(hit);
            }
            self.record_device(surface, false, &mut devices, parameters.max_device_hits);
            hit.devices = devices;
            penetration = (penetration - effective * loss_factor).max(0.0);
            hit.remaining_mm = penetration;
            hit.outcome = ArmorOutcome::ScreenPenetrated;
            last_hit = Some(hit);
            if plate.once {
                seen.push(material_key);
            }
            if heat {
                jet_exit = Some(distance + effective * 0.001);
            }
        }
        last_hit
    }
    fn effective_thickness(&self, surface: Surface, parameters: &BattleParameters) -> f32 {
        let plate = surface.plate;
        let ap = matches!(self.ballistics.kind, ShellKind::ArmorPiercing);
        let cosine = if plate.angled { surface.cosine } else { 1.0 };
        let ratio = self.ballistics.caliber_mm / plate.thickness_mm;
        let mut normalization = if ap {
            self.ballistics.normalization
        } else {
            0.0
        };
        if ap
            && plate.caliber_normalization
            && plate.thickness_mm > 0.0
            && ratio > parameters.ap_normalization_overmatch_ratio
        {
            normalization *= parameters.ap_normalization_caliber_factor * ratio;
        }
        let angle = (cosine.acos() - normalization).max(0.0);
        plate.thickness_mm / angle.cos().max(0.00001)
    }

    fn strike_tracks_beyond_hull(
        &self,
        entry: Surface,
        surfaces: &[Surface],
        seen: &mut Vec<(u8, u32)>,
        hit: &mut ArmorHit,
        parameters: &BattleParameters,
    ) {
        let entering = entry.normal.dot(self.direction);
        let mut exited = false;
        let mut remaining = hit.remaining_mm;
        for &surface in surfaces {
            if surface.distance <= entry.distance {
                continue;
            }
            if surface.plate.damage_factor > 0.0 {
                if exited || surface.segment.part != entry.segment.part {
                    break;
                }
                if surface.normal.dot(self.direction) * entering >= 0.0 {
                    continue;
                }
                remaining -= self.effective_thickness(surface, parameters);
                if remaining < 0.0 {
                    break;
                }
                exited = true;
            } else {
                let key = (surface.segment.part, surface.material);
                if seen.contains(&key) {
                    continue;
                }
                remaining -= self.effective_thickness(surface, parameters);
                if remaining < 0.0 {
                    break;
                }
                if exited && is_track(surface) {
                    self.record_device(
                        surface,
                        false,
                        &mut hit.devices,
                        parameters.max_device_hits,
                    );
                }
                if surface.plate.once {
                    seen.push(key);
                }
            }
        }
    }

    fn record_device(
        &self,
        hit: Surface,
        explosion: bool,
        devices: &mut [Option<DeviceStrike>],
        limit: usize,
    ) {
        let Some(device) = hit.plate.device else {
            return;
        };
        if devices
            .iter()
            .flatten()
            .any(|existing| existing.device == device.device)
        {
            return;
        }
        let chance = if explosion {
            device.explosion_chance
        } else {
            device.projectile_chance
        };
        if chance <= 0.0 || fastrand::f32() >= chance {
            return;
        }
        let damage = if !explosion && is_track(hit) {
            (self.damage_at(hit.distance) * hit.track_damage_factor).round() as u16
        } else {
            let curve = if explosion {
                self.ballistics
                    .explosive
                    .map_or([0.0; 2], |he| he.spall_device_damage)
            } else {
                self.ballistics.device_damage
            };
            rolled_damage(
                shell_value_at_distance(curve, self.travelled + hit.distance),
                self.damage_roll,
                1.0,
            )
        };
        if damage > 0
            && let Some(slot) = devices.iter_mut().take(limit).find(|slot| slot.is_none())
        {
            *slot = Some(DeviceStrike {
                device: device.device,
                health: device.health,
                damage,
            });
        }
    }
}

fn is_track(surface: Surface) -> bool {
    surface
        .plate
        .device
        .is_some_and(|device| TRACKS.contains(&device.device))
}

fn rolled_damage(nominal: f32, roll: f32, factor: f32) -> u16 {
    (nominal * roll * factor).round() as u16
}

pub(crate) fn roll(magnitude: f32, uniform: bool, parameters: &BattleParameters) -> f32 {
    if magnitude == 0.0 {
        return 1.0;
    }
    let unit = fastrand::f32;
    let sample = if uniform {
        2.0 * unit() - 1.0
    } else {
        // Approximate the client armor inspector's truncated normal distribution.
        (0..parameters.normal_roll_max_samples)
            .map(|_| {
                (-2.0 * (1.0 - unit()).ln()).sqrt()
                    * (std::f32::consts::TAU * unit()).cos()
                    * parameters.normal_roll_standard_deviation
            })
            .find(|sample| sample.abs() <= 1.0)
            .unwrap_or(0.0)
    };
    1.0 + magnitude * sample
}

fn clip_ray([min, max]: [Vec3; 2], origin: Vec3, direction: Vec3) -> Option<[f32; 2]> {
    let mut near = f32::NEG_INFINITY;
    let mut far = f32::INFINITY;
    for axis in 0..3 {
        if direction[axis].abs() < 1e-7 {
            if origin[axis] < min[axis] || origin[axis] > max[axis] {
                return None;
            }
        } else {
            let a = (min[axis] - origin[axis]) / direction[axis];
            let b = (max[axis] - origin[axis]) / direction[axis];
            near = near.max(a.min(b));
            far = far.min(a.max(b));
        }
    }
    (near <= far).then_some([near, far])
}

fn triangle_distance(a: Vec3, b: Vec3, c: Vec3, origin: Vec3, direction: Vec3) -> Option<f32> {
    let ab = b - a;
    let ac = c - a;
    let cross = direction.cross(ac);
    let determinant = ab.dot(cross);
    if determinant.abs() < 1e-8 {
        return None;
    }
    let offset = origin - a;
    let u = offset.dot(cross) / determinant;
    let cross = offset.cross(ab);
    let v = direction.dot(cross) / determinant;
    let distance = ac.dot(cross) / determinant;
    (u >= 0.0 && v >= 0.0 && u + v <= 1.0 && distance > 1e-4).then_some(distance)
}

fn impact_segment(
    part: u8,
    [min, max]: [Vec3; 2],
    origin: Vec3,
    direction: Vec3,
    near: f32,
    far: f32,
    distance: f32,
) -> ImpactSegment {
    let quantize = |at: f32| {
        let point = (origin + direction * at - min) / (max - min);
        point
            .to_array()
            .map(|axis| (axis.clamp(0.0, 1.0) * 255.0).round() as u8)
    };
    // A short lead-in selects this plate even when the same component has
    // another surface farther back along the ray.
    ImpactSegment {
        part,
        start: quantize((distance - 0.05).max(near)),
        end: quantize((distance + 0.5).min(far)),
    }
}

#[cfg(test)]
mod tests;
