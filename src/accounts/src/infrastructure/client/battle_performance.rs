use glam::{Vec3, Vec3Swizzles};
use std::{collections::BTreeMap, io::Read};
use wot_platform::client_formats::pxml::{Element, Value};
use wot_resources::ResourceBundle;
use wot_shared_kernel::{GunLoading, GunPerformance, Mobility, ShellEffects, Wheel};

pub(super) fn collision_model<'a>(
    local: Option<&'a Element>,
    base: &'a Element,
) -> Option<&'a str> {
    field(local, base, &["hitTester", "collisionModelClient"])
        .and_then(Value::as_string)
        .and_then(|path| path.trim().strip_suffix(".model"))
}

pub(super) fn collision_bounds(
    fs: &ResourceBundle,
    local: Option<&Element>,
    base: &Element,
) -> anyhow::Result<Option<[Vec3; 2]>> {
    let Some(model) = collision_model(local, base) else {
        return Ok(None);
    };
    let path = format!("{model}.bounds");
    if !fs.has_file(&path) {
        return Ok(None);
    }
    let mut bytes = [0; 28];
    fs.read(&path)?.read_exact(&mut bytes)?;
    let (header, values) = bytes.split_at(4);
    anyhow::ensure!(
        header == b"WVB1",
        "Invalid vehicle collision bounds in {path}"
    );
    let mut floats = values
        .as_chunks::<4>()
        .0
        .iter()
        .copied()
        .map(f32::from_le_bytes);
    let mut read = || Vec3::from_array(std::array::from_fn(|_| floats.next().unwrap_or(f32::NAN)));
    let [min, max] = [read(), read()];
    anyhow::ensure!(
        min.is_finite()
            && max.is_finite()
            && min.cmplt(max).all()
            && (max - min).max_element() <= 32.0
            && min.abs().max(max.abs()).max_element() <= 32.0,
        "Invalid vehicle collision extent in {path}"
    );
    Ok(Some([min, max]))
}

pub(super) fn speed_limits(vehicle: &Element) -> Option<[f32; 2]> {
    let speed = vehicle.get_child_element("speedLimits")?;
    Some([
        non_negative(speed.get_child_float("forward")?)? / 3.6,
        non_negative(speed.get_child_float("backward")?)? / 3.6,
    ])
}

pub(super) fn traverse(local: Option<&Element>, base: &Element) -> Option<f32> {
    let rotation = number(local, base, "rotationSpeed")?.to_radians();
    non_negative(rotation)
}

pub(super) fn hull_traverse_limit(local: Option<&Element>, base: &Element) -> Option<f32> {
    number(local, base, "rotationSpeedLimit")
        .and_then(non_negative)
        .map(f32::to_radians)
}

pub(super) fn mobility(
    local: Option<&Element>,
    base: &Element,
    physics: Option<&Element>,
    wheeled: bool,
) -> Mobility {
    let mut mobility = Mobility {
        traverse_limit: hull_traverse_limit(local, base),
        max_climb_angle: number(local, base, "maxClimbAngle")
            .filter(|angle| (0.0..90.0).contains(angle))
            .map(f32::to_radians),
        rotation_around_center: field(local, base, &["rotationIsAroundCenter"])
            .and_then(Value::as_boolean)
            .unwrap_or(true),
        ..Mobility::default()
    };
    if let Some(resistance) = vector(local, base, "terrainResistance")
        .filter(|value| value.min_element() >= 0.05 && value.max_element() <= 20.0)
    {
        mobility.terrain_resistance = resistance.to_array();
    }
    for (index, ground) in ["firm", "medium", "soft"].into_iter().enumerate() {
        if let Some(friction) = physics
            .and_then(|element| element.get_child_value_at(&["grounds", ground, "rollingFriction"]))
            .and_then(Value::as_float)
            .filter(|value| (0.0..=1.0).contains(value))
        {
            mobility.rolling_friction[index] = friction;
        }
    }
    if let Some(point) = field(local, base, &["topRightCarryingPoint"])
        .and_then(Value::as_vec2)
        .filter(|point| (0.1..=5.0).contains(&point.x))
    {
        mobility.track_half_width = point.x;
        if (0.5..=10.0).contains(&point.y) {
            mobility.track_half_length = point.y;
        }
    }
    if let Some(wheels) = local
        .and_then(|chassis| chassis.get_child_element("wheels"))
        .or_else(|| base.get_child_element("wheels"))
        && wheels.get_child_boolean("generalWheels") == Some(true)
    {
        mobility.wheels = general_wheels(wheels, physics);
        mobility.turns_in_place = !wheeled
            || mobility.wheels.is_empty()
            || physics
                .and_then(|element| element.get_child_boolean("isWheeledOnSpotRotation"))
                .unwrap_or(false);
    }
    mobility
}

fn general_wheels(wheels: &Element, physics: Option<&Element>) -> Vec<Wheel> {
    let mut indexed = Vec::new();
    for (name, value) in wheels.iter_children_all() {
        let Some(wheel) = value.as_element().filter(|_| name == "wheel") else {
            continue;
        };
        if wheel.get_child_boolean("nonTrack") != Some(true) {
            continue;
        }
        if let (Some(index), Some(center)) = (
            wheel.get_child_integer("index"),
            wheel
                .get_child_vec3("wheelPos")
                .filter(|center| center.is_finite() && center.abs().max_element() <= 20.0),
        ) {
            indexed.push((
                index,
                center.xz(),
                wheel.get_child("steeringProcessor").is_some(),
            ));
        }
    }
    indexed.sort_by_key(|&(index, _, _)| index);
    indexed.truncate(32);
    let mut axles: Vec<_> = indexed.iter().map(|(_, center, _)| center.y).collect();
    axles.sort_by(f32::total_cmp);
    axles.dedup_by(|a, b| (*a - *b).abs() < 0.1);
    let angles = physics
        .and_then(|element| element.get_child_value("axleSteeringAngles"))
        .and_then(floats)
        .unwrap_or_default();
    indexed
        .into_iter()
        .map(|(_, center, steers)| Wheel {
            center,
            steering_limit: steers.then(|| {
                axles
                    .iter()
                    .position(|&z| (z - center.y).abs() < 0.1)
                    .and_then(|axle| angles.get(axle).copied())
                    .filter(|angle| angle.is_finite() && angle.abs() < 90.0)
                    .map_or(0.0, f32::to_radians)
            }),
        })
        .collect()
}

pub(super) fn gun(
    local: Option<&Element>,
    base: &Element,
    turret_traverse: f32,
    gun_position: Option<Vec3>,
    shell_damage: BTreeMap<i32, u16>,
    shell_ranges: BTreeMap<i32, f32>,
    shell_effects: BTreeMap<i32, ShellEffects>,
) -> Option<GunPerformance> {
    let yaw_limits = field(local, base, &["turretYawLimits"])
        .and_then(Value::as_vec2)
        .filter(|yaw| yaw.is_finite() && yaw.x <= yaw.y)
        .filter(|yaw| yaw.x > -179.0 || yaw.y < 179.0)
        .map(|yaw| [yaw.x.to_radians(), yaw.y.to_radians()]);
    let min_pitch = pitch_curve(field(local, base, &["pitchLimits", "minPitch"])?)?;
    let max_pitch = pitch_curve(field(local, base, &["pitchLimits", "maxPitch"])?)?;
    let capacity = field(local, base, &["clip", "count"])
        .and_then(Value::as_integer)
        .unwrap_or(1);
    let capacity = u16::try_from(capacity).ok().filter(|&n| n > 0)?;
    let intra_clip_time = if capacity > 1 {
        60.0 / positive(field(local, base, &["clip", "rate"])?.as_float()?)?
    } else {
        0.0
    };
    let autoreload_times = match field(local, base, &["autoreload", "reloadTime"]) {
        Some(value) => floats(value)?
            .into_iter()
            .map(positive)
            .collect::<Option<Vec<_>>>()?,
        None => Vec::new(),
    };
    Some(GunPerformance {
        turret_traverse,
        gun_traverse: traverse(local, base).unwrap_or(0.0),
        yaw_limits,
        min_pitch,
        max_pitch,
        aiming_time: positive(number(local, base, "aimingTime")?)?,
        base_dispersion_angle: number(local, base, "shotDispersionRadius")
            .and_then(non_negative)
            .map(|radius| (radius / 100.0).atan()),
        shot_position: gun_position
            .map(|position| position + vector(local, base, "shotOffset").unwrap_or(Vec3::ZERO)),
        loading: GunLoading {
            capacity,
            reload_time: positive(number(local, base, "reloadTime")?)?,
            intra_clip_time: non_negative(intra_clip_time)?,
            forced_reload_time: non_negative(
                number(local, base, "forcedReloadTime").unwrap_or(0.0),
            )?,
            autoreload_times,
        },
        shell_damage,
        shell_ranges,
        shell_effects,
        shell_ballistics: BTreeMap::new(),
    })
}

pub(super) fn vector(local: Option<&Element>, base: &Element, name: &str) -> Option<Vec3> {
    field(local, base, &[name])
        .and_then(Value::as_vec3)
        .filter(|value| value.is_finite())
}

pub(super) fn shell_range(local: Option<&Element>, shared: Option<&Element>) -> Option<f32> {
    let distance = local
        .and_then(|shot| shot.get_child_value("maxDistance"))
        .or_else(|| shared.and_then(|shot| shot.get_child_value("maxDistance")))?
        .as_float()?;
    (distance > 0.0 && distance <= 10_000.0).then_some(distance)
}

pub(super) fn shell_damage(
    local_shot: Option<&Element>,
    shared_shot: Option<&Element>,
    shell: &Element,
) -> Option<u16> {
    let path = ["damage", "armor"];
    let value = local_shot
        .and_then(|shot| shot.get_child_value_at(&path))
        .or_else(|| shared_shot.and_then(|shot| shot.get_child_value_at(&path)))
        .or_else(|| shell.get_child_value_at(&path))?;
    // The optional second value is nominal damage at range. The first is the
    // midpoint of the ordinary random damage bounds at short range.
    let damage = value
        .as_vec2()
        .map(|pair| pair.x)
        .or_else(|| value.as_float())?;
    let midpoint = damage.round();
    (damage >= 0.0 && midpoint <= f32::from(u16::MAX)).then_some(midpoint as u16)
}

fn field<'a>(local: Option<&'a Element>, base: &'a Element, path: &[&str]) -> Option<&'a Value> {
    local
        .and_then(|element| element.get_child_value_at(path))
        .or_else(|| base.get_child_value_at(path))
}

fn number(local: Option<&Element>, base: &Element, name: &str) -> Option<f32> {
    field(local, base, &[name])?.as_float()
}

fn floats(value: &Value) -> Option<Vec<f32>> {
    if let Some(vector) = value.as_vector() {
        Some(vector.iter().copied().collect())
    } else if let Some(text) = value.as_string() {
        text.split_whitespace()
            .map(|word| word.parse().ok())
            .collect()
    } else {
        Some(vec![value.as_float()?])
    }
}

fn pitch_curve(value: &Value) -> Option<Vec<(f32, f32)>> {
    let values = floats(value)?;
    let (pairs, remainder) = values.as_chunks::<2>();
    if !remainder.is_empty() || pairs.len() < 2 {
        return None;
    }
    let points = pairs
        .iter()
        .map(|&[yaw, pitch]| (yaw, pitch.to_radians()))
        .collect::<Vec<_>>();
    (points
        .iter()
        .all(|(yaw, pitch)| yaw.is_finite() && pitch.is_finite())
        && points.windows(2).all(|pair| pair[0].0 < pair[1].0))
    .then_some(points)
}

fn non_negative(value: f32) -> Option<f32> {
    (value.is_finite() && (0.0..=3_600.0).contains(&value)).then_some(value)
}

fn positive(value: f32) -> Option<f32> {
    non_negative(value).filter(|&value| value > 0.0)
}

#[cfg(test)]
mod tests {
    use super::super::GarageCatalog;
    use super::*;
    use std::path::Path;
    use wot_resources::ResourceBundle;

    #[test]
    fn shell_damage_prefers_gun_overrides_and_uses_nominal_short_range_damage() {
        fn definition(armor: Value) -> Element {
            let mut damage = Element::new();
            damage.add_children("armor", armor);
            let mut definition = Element::new();
            definition.add_children("damage", Value::Element(Box::new(damage)));
            definition
        }

        let shell = definition(Value::Integer(90));
        let shared = definition(Value::Integer(100));
        let local = definition(Value::String("125 80".into()));
        assert_eq!(shell_damage(None, None, &shell), Some(90));
        assert_eq!(shell_damage(None, Some(&shared), &shell), Some(100));
        assert_eq!(shell_damage(Some(&local), Some(&shared), &shell), Some(125));
        let invalid = definition(Value::Integer(-1));
        assert_eq!(shell_damage(Some(&invalid), Some(&shared), &shell), None);
    }

    #[test]
    #[ignore = "Requires WOT_RESOURCE_DIR with the extracted EU client bundle"]
    fn reads_equipped_battle_performance_from_the_client_bundle() {
        let resource_dir = std::env::var_os("WOT_RESOURCE_DIR").unwrap();
        let resources = ResourceBundle::open(Path::new(&resource_dir)).unwrap();
        let catalog = GarageCatalog::load(resources, &[]).unwrap();
        let performance = |key| {
            let vehicle = catalog.vehicle(key).unwrap();
            vehicle
                .modules
                .battle_performance(&vehicle.modules.rules.stock)
                .unwrap()
        };

        let t1 = performance("usa:A01_T1_Cunningham");
        assert!(t1.armor.iter().all(Option::is_some));
        let hull = &t1.armor[1].as_ref().unwrap().mesh;
        assert!(
            hull.triangles
                .iter()
                .any(|t| t.plate.damage_factor == 1.0 && t.plate.thickness_mm > 0.0)
        );
        assert!(
            t1.armor[0]
                .as_ref()
                .unwrap()
                .mesh
                .triangles
                .iter()
                .all(|t| t.plate.damage_factor == 0.0 && t.plate.thickness_mm > 0.0)
        );
        assert!(t1.gun.shell_ballistics.values().any(|shell| matches!(
            shell.kind,
            wot_shared_kernel::ShellKind::HighExplosive {
                shield_penetration: true,
                ..
            }
        )));
        assert!(t1.gun.shell_ballistics.values().any(|shell| {
            shell
                .explosive
                .is_some_and(|he| he.spall_damage[0] > 0.0 && he.artillery_radius == 0.0)
        }));
        let artillery = performance("usa:A18_M41");
        assert!(artillery.gun.shell_ballistics.values().any(|shell| {
            shell.gravity > 0.0
                && shell
                    .explosive
                    .is_some_and(|he| he.artillery_radius > 0.0 && he.stun.is_some())
        }));

        assert_eq!(t1.gun.shell_ballistics.len(), t1.gun.shell_damage.len());

        let su85 = performance("ussr:R02_SU-85");
        let [left, right] = su85.gun.yaw_limits.unwrap();
        assert!(left < 0.0 && right > 0.0 && right - left < std::f32::consts::PI);
        assert!(su85.forward_speed > su85.reverse_speed && su85.reverse_speed > 0.0);
        assert!(su85.hull_traverse > 0.0);

        assert!(su85.mobility.terrain_resistance[2] > su85.mobility.terrain_resistance[0]);
        assert!(su85.mobility.power_per_kg > 5.0 && su85.mobility.power_per_kg < 25.0);

        let ebr = performance("france:F108_Panhard_EBR_105");
        assert_eq!(ebr.mobility.wheels.len(), 8);
        assert!(
            ebr.mobility
                .wheels
                .iter()
                .any(|wheel| wheel.steering_limit.is_some_and(|limit| limit.abs() > 0.01))
        );
        assert!(!ebr.mobility.turns_in_place);

        let amx = performance("france:F17_AMX_13_90");
        assert!(amx.gun.loading.capacity > 1);
        assert!(amx.gun.loading.reload_time > amx.gun.loading.intra_clip_time);
        assert!(amx.gun.loading.autoreload_times.is_empty());

        let amx30 = performance("france:F72_AMX_30");
        assert!(amx30.turret_pivot.unwrap().y > 0.0);
        assert!(amx30.gun.shot_position.unwrap().length() > 0.0);

        let progetto = performance("italy:It13_Progetto_M35_mod_46");
        assert_eq!(
            progetto.gun.loading.autoreload_times.len(),
            usize::from(progetto.gun.loading.capacity)
        );
        assert!(progetto.gun.turret_traverse > 0.0);
    }
}
