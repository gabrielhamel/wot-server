use crate::{
    platform::client_formats::pxml::{Element, Value},
    shared_kernel::{GunLoading, GunPerformance},
};

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

pub(super) fn hull_traverse(local: Option<&Element>, base: &Element) -> Option<f32> {
    let traverse = traverse(local, base)?;
    let limit = number(local, base, "rotationSpeedLimit")
        .and_then(non_negative)
        .map_or(traverse, f32::to_radians);
    Some(traverse.min(limit))
}

pub(super) fn gun(
    local: Option<&Element>,
    base: &Element,
    turret_traverse: f32,
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
        loading: GunLoading {
            capacity,
            reload_time: positive(number(local, base, "reloadTime")?)?,
            intra_clip_time: non_negative(intra_clip_time)?,
            forced_reload_time: non_negative(
                number(local, base, "forcedReloadTime").unwrap_or(0.0),
            )?,
            autoreload_times,
        },
    })
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
    use std::path::Path;
    use wot_resources::ResourceBundle;

    #[test]
    #[ignore = "Requires WG_RESOURCE_DIR with the extracted EU client bundle"]
    fn reads_equipped_battle_performance_from_the_client_bundle() {
        let resource_dir = std::env::var_os("WG_RESOURCE_DIR").unwrap();
        let resources = ResourceBundle::open(Path::new(&resource_dir)).unwrap();
        let catalog = GarageCatalog::load(resources, &[]).unwrap();
        let performance = |key| {
            let vehicle = catalog.vehicle(key).unwrap();
            vehicle
                .modules
                .battle_performance(&vehicle.modules.rules.stock)
                .unwrap()
        };

        let su85 = performance("ussr:R02_SU-85");
        let [left, right] = su85.gun.yaw_limits.unwrap();
        assert!(left < 0.0 && right > 0.0 && right - left < std::f32::consts::PI);
        assert!(su85.forward_speed > su85.reverse_speed && su85.reverse_speed > 0.0);
        assert!(su85.hull_traverse > 0.0);

        let amx = performance("france:F17_AMX_13_90");
        assert!(amx.gun.loading.capacity > 1);
        assert!(amx.gun.loading.reload_time > amx.gun.loading.intra_clip_time);
        assert!(amx.gun.loading.autoreload_times.is_empty());

        let progetto = performance("italy:It13_Progetto_M35_mod_46");
        assert_eq!(
            progetto.gun.loading.autoreload_times.len(),
            usize::from(progetto.gun.loading.capacity)
        );
        assert!(progetto.gun.turret_traverse > 0.0);
    }
}
