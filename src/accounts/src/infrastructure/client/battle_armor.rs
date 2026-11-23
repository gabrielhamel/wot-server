use super::{battle_performance::collision_bounds, item_defs::read_xml};
use anyhow::{Context, Result, ensure};
use byteorder::{LE, ReadBytesExt};
use glam::Vec3;
use std::{
    collections::BTreeMap,
    io::{BufReader, Read},
    sync::Arc,
};
use wot_platform::client_formats::pxml::{Element, Value};
use wot_resources::ResourceBundle;
use wot_shared_kernel::{ArmorMesh, ArmorPlate, ArmorTriangle, ShellBallistics, ShellKind};

pub(super) struct ArmorMaterials {
    materials: BTreeMap<String, (u32, Element)>,
    pub(super) projectile_speed_factor: f32,
}

impl ArmorMaterials {
    pub(super) fn load(fs: &ResourceBundle) -> Result<Self> {
        let path = "scripts/item_defs/vehicles/common/vehicle_common.xml";
        if !fs.has_file(path) {
            return Ok(Self {
                materials: BTreeMap::new(),
                projectile_speed_factor: 1.0,
            });
        }
        let common = read_xml(fs, path)?;
        let defaults = common
            .get_child_element("materials")
            .context("Missing vehicle materials")?;
        let kinds = read_xml(fs, "system/data/material_kinds.xml")?;
        let materials: BTreeMap<_, _> = kinds
            .iter_children_all()
            .filter_map(|(_, value)| {
                let kind = value.as_element()?;
                let name = kind.get_child_string("desc")?.trim();
                let id = u32::try_from(kind.get_child_integer("id")?).ok()?;
                Some((
                    name.to_owned(),
                    (id, defaults.get_child_element(name)?.clone()),
                ))
            })
            .collect();
        ensure!(!materials.is_empty(), "Missing vehicle armor material IDs");
        let projectile_speed_factor = common
            .get_child_value_at(&["miscParams", "projectileSpeedFactor"])
            .and_then(Value::as_float)
            .filter(|value| (0.01..=10.0).contains(value))
            .context("Invalid projectile speed factor")?;
        Ok(Self {
            materials,
            projectile_speed_factor,
        })
    }

    pub(super) fn mesh(
        &self,
        fs: &ResourceBundle,
        local: Option<&Element>,
        base: &Element,
    ) -> Result<Option<Arc<ArmorMesh>>> {
        let Some(path) = super::battle_performance::collision_model(local, base) else {
            return Ok(None);
        };
        let path = format!("{path}.armor");
        if self.materials.is_empty() || !fs.has_file(&path) {
            return Ok(None);
        }
        let bounds = collision_bounds(fs, local, base)?.context("Missing armor bounds")?;
        let mut plates = BTreeMap::new();
        for part in [Some(base), local].into_iter().flatten() {
            let Some(armor) = part.get_child_element("armor") else {
                continue;
            };
            for (name, value) in armor.iter_children_all() {
                let Some((id, defaults)) = self.materials.get(name) else {
                    continue;
                };
                let overrides = value.as_element();
                let Some(thickness_mm) = overrides
                    .map_or(value, |entry| &entry.value)
                    .as_float()
                    .filter(|&value| (0.0..=20_000.0).contains(&value))
                else {
                    continue;
                };
                let boolean = |key| {
                    overrides
                        .and_then(|e| e.get_child_boolean(key))
                        .or_else(|| defaults.get_child_boolean(key))
                        .unwrap_or(false)
                };
                let damage_factor = overrides
                    .and_then(|entry| entry.get_child_float("vehicleDamageFactor"))
                    .or_else(|| defaults.get_child_float("vehicleDamageFactor"))
                    .filter(|value| (0.0..=1.0).contains(value))
                    .unwrap_or(0.0);
                plates.insert(
                    *id,
                    ArmorPlate {
                        thickness_mm,
                        damage_factor,
                        angled: boolean("useHitAngle"),
                        ricochets: boolean("mayRicochet"),
                        caliber_ricochet: boolean("checkCaliberForRicochet"),
                        caliber_normalization: boolean("checkCaliberForHitAngleNorm"),
                        once: boolean("collideOnceOnly"),
                    },
                );
            }
        }
        let mut file = BufReader::new(fs.read(&path)?);
        let mut header = [0; 4];
        file.read_exact(&mut header)?;
        let count = file.read_u32::<LE>()?;
        ensure!(
            &header == b"WVA1" && count <= 50_000,
            "Invalid armor mesh {path}"
        );
        let mut triangles = Vec::new();
        for _ in 0..count {
            let material = file.read_u32::<LE>()?;
            let mut vertices = [Vec3::ZERO; 3];
            for vertex in &mut vertices {
                *vertex = Vec3::new(
                    file.read_f32::<LE>()?,
                    file.read_f32::<LE>()?,
                    file.read_f32::<LE>()?,
                );
                ensure!(
                    vertex.is_finite() && vertex.abs().max_element() <= 32.0,
                    "Invalid armor vertex in {path}"
                );
            }
            if let Some(&plate) = plates.get(&material) {
                triangles.push(ArmorTriangle {
                    vertices,
                    material,
                    plate,
                });
            }
        }
        Ok((!triangles.is_empty()).then(|| Arc::new(ArmorMesh { bounds, triangles })))
    }
}

pub(super) fn shell_ballistics(
    local: Option<&Element>,
    shared: Option<&Element>,
    shell: &Element,
    projectile_speed_factor: f32,
) -> Option<ShellBallistics> {
    let shot = |path: &[&str]| {
        local
            .and_then(|e| e.get_child_value_at(path))
            .or_else(|| shared.and_then(|e| e.get_child_value_at(path)))
    };
    let property = |path: &[&str]| shot(path).or_else(|| shell.get_child_value_at(path));
    let float = |path: &[&str]| {
        property(path)
            .and_then(Value::as_float)
            .filter(|v| v.is_finite())
    };
    let text = |key| property(&[key]).and_then(Value::as_string).map(str::trim);
    let first = |value: &Value| value.as_vec2().map(|v| v.x).or_else(|| value.as_float());
    let kind = match text("kind")? {
        "ARMOR_PIERCING" | "ARMOR_PIERCING_CR" | "ARMOR_PIERCING_HE" => ShellKind::ArmorPiercing,
        "HOLLOW_CHARGE" => ShellKind::HollowCharge {
            jet_loss_per_metre: 10.0
                * float(&["piercingPowerLossFactorByDistance"]).unwrap_or(0.05),
        },
        "HIGH_EXPLOSIVE" => ShellKind::HighExplosive {
            spall_damage: property(&["armorSpalls", "damage", "armor"])
                .and_then(first)
                .unwrap_or(0.0),
            shield_penetration: property(&["shieldPenetration"])
                .and_then(Value::as_boolean)
                .unwrap_or_else(|| text("mechanics") == Some("MODERN")),
            obstacle_penetration: property(&["obstaclePenetration"])
                .and_then(Value::as_boolean)
                .unwrap_or_else(|| text("mechanics") == Some("MODERN")),
        },
        _ => ShellKind::Other,
    };
    let penetration = shot(&["piercingPower"])?;
    let penetration_mm = penetration
        .as_vec2()
        .map(|pair| pair.to_array())
        .or_else(|| penetration.as_float().map(|value| [value; 2]))?;
    let speed = shot(&["speed"])?.as_float()? * projectile_speed_factor;
    let caliber_mm = float(&["caliber"])?;
    if !penetration_mm
        .iter()
        .all(|value| (0.0..=10_000.0).contains(value))
        || !(1.0..=10_000.0).contains(&speed)
        || !(1.0..=1_000.0).contains(&caliber_mm)
    {
        return None;
    }
    Some(ShellBallistics {
        kind,
        caliber_mm,
        penetration_mm,
        speed,
        penetration_randomization: float(&["piercingPowerRandomization"])
            .unwrap_or(0.25)
            .clamp(0.0, 1.0),
        damage_randomization: float(&["damageRandomization"])
            .unwrap_or(0.25)
            .clamp(0.0, 1.0),
        uniform_penetration: text("piercingPowerRandomizationType") == Some("UNIFORM"),
        uniform_damage: text("damageRandomizationType") == Some("UNIFORM"),
        far_damage: property(&["damage", "armor"])?
            .as_vec2()
            .map(|pair| pair.y)
            .or_else(|| float(&["damage", "armor"]))?
            .clamp(0.0, f32::from(u16::MAX)),
        normalization: float(&["normalizationAngle"])
            .unwrap_or(0.0)
            .clamp(0.0, 90.0)
            .to_radians(),
        trace_ricochet: property(&["enableTraceRicochet"])
            .and_then(Value::as_boolean)
            .unwrap_or(true),
        obstacle_damage: float(&["obstacles", "damage"]).filter(|&v| v >= 0.0),
        obstacle_penetration_loss: float(&["obstacles", "powerReduction"])
            .unwrap_or_default()
            .max(0.0),
        ricochet_cos: float(&["ricochetAngle"])
            .unwrap_or(90.0)
            .clamp(0.0, 90.0)
            .to_radians()
            .cos(),
    })
}
