use super::{battle_performance::collision_bounds, item_defs::read_xml};
use anyhow::{Context, Result, ensure};
use byteorder::{LE, ReadBytesExt};
use glam::{Affine3A, Vec3};
use std::{
    collections::BTreeMap,
    io::{BufReader, Read},
    sync::Arc,
};
use wot_platform::client_formats::pxml::{Element, Value};
use wot_resources::ResourceBundle;
use wot_shared_kernel::{
    ArmorDevice, ArmorMesh, ArmorPlate, ArmorTriangle, Device, DeviceHealth, EngineCompartment,
    ExplosiveShell, ShellBallistics, ShellKind, ShellStun, shell_zero_distance,
};

pub(super) struct ArmorMaterials {
    materials: BTreeMap<String, (u32, Element)>,
    projectile_speed_factor: f32,
    explosion_factors: [f32; 4],
    stun_defaults: [f32; 5],
}

impl ArmorMaterials {
    pub(super) fn load(fs: &ResourceBundle) -> Result<Self> {
        let path = "scripts/item_defs/vehicles/common/vehicle_common.xml";
        if !fs.has_file(path) {
            return Ok(Self {
                materials: BTreeMap::new(),
                projectile_speed_factor: 1.0,
                explosion_factors: [0.5, 1.1, 0.05, 1.1],
                stun_defaults: [30.0, 0.45, 0.55, 5.0, 0.6],
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
        let explosion_factors = [
            "explosionDamageFactor",
            "explosionDamageAbsorptionFactor",
            "explosionEdgeDamageFactor",
            "shellFragmentsDamageAbsorptionFactor",
        ]
        .map(|key| {
            common
                .get_child_value_at(&["miscParams", key])
                .and_then(Value::as_float)
                .filter(|value| (0.0..=100.0).contains(value))
                .with_context(|| format!("Invalid {key}"))
        })
        .into_iter()
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Missing explosion factors"))?;
        let stun_path = "scripts/item_defs/vehicles/common/stun.xml";
        let stun = fs
            .has_file(stun_path)
            .then(|| read_xml(fs, stun_path))
            .transpose()?;
        let stun_defaults = [
            ("baseStunDuration", 30.0),
            ("guaranteedStunDuration", 0.45),
            ("damageDurationCoeff", 0.55),
            ("minStunDuration", 5.0),
            ("stunFactorTurretTraverse", 0.6),
        ]
        .map(|(key, fallback)| {
            stun.as_ref()
                .and_then(|xml| xml.get_child_float(key))
                .filter(|value| (0.0..=120.0).contains(value))
                .unwrap_or(fallback)
        });
        Ok(Self {
            materials,
            projectile_speed_factor,
            explosion_factors,
            stun_defaults,
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
                let number = |key| {
                    overrides
                        .and_then(|entry| entry.get_child_float(key))
                        .or_else(|| defaults.get_child_float(key))
                        .unwrap_or(0.0)
                        .clamp(0.0, 1.0)
                };
                let device = device_kind(name, *id).map(|device| {
                    let info = match device {
                        Device::TrackLeft | Device::TrackRight | Device::Wheel | Device::Gun => {
                            Some(part)
                        }
                        Device::AmmoRack => part.get_child_element("ammoBayHealth"),
                        Device::Turret => part.get_child_element("turretRotatorHealth"),
                        Device::Optics => part.get_child_element("surveyingDeviceHealth"),
                        _ => None,
                    };
                    ArmorDevice {
                        device,
                        can_ignite_engine: name == "engine",
                        projectile_chance: number("chanceToHitByProjectile"),
                        explosion_chance: number("chanceToHitByExplosion"),
                        health: info.and_then(|info| read_device_health(None, info)),
                    }
                });
                plates.insert(
                    *id,
                    ArmorPlate {
                        thickness_mm,
                        damage_factor: if device.is_some_and(|device| {
                            matches!(device.device, Device::Engine | Device::Fuel)
                        }) {
                            0.0
                        } else {
                            damage_factor
                        },
                        angled: boolean("useHitAngle"),
                        ricochets: boolean("mayRicochet"),
                        caliber_ricochet: boolean("checkCaliberForRicochet"),
                        caliber_normalization: boolean("checkCaliberForHitAngleNorm"),
                        once: boolean("collideOnceOnly"),
                        device,
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
        let engine_compartment = if triangles.iter().any(|triangle| {
            triangle
                .plate
                .device
                .is_some_and(|device| device.can_ignite_engine)
        }) {
            None
        } else {
            self.engine_compartment(fs, local, base)
        };
        Ok((!triangles.is_empty()).then(|| {
            Arc::new(ArmorMesh {
                bounds,
                triangles,
                engine_compartment,
            })
        }))
    }

    fn engine_compartment(
        &self,
        fs: &ResourceBundle,
        local: Option<&Element>,
        base: &Element,
    ) -> Option<EngineCompartment> {
        let value = |key: &[&str]| {
            local
                .and_then(|xml| xml.get_child_value_at(key))
                .or_else(|| base.get_child_value_at(key))
                .and_then(Value::as_string)
        };
        let exhaust = value(&["exhaust", "nodes"])?;
        let model = value(&["models", "undamaged"])?
            .trim()
            .strip_suffix(".model")?;
        let visual = read_xml(fs, &format!("{model}.visual_processed")).ok()?;
        let mut positions = Vec::new();
        exhaust_positions(&visual, Affine3A::IDENTITY, exhaust, &mut positions, 0);
        if positions.is_empty() {
            return None;
        }
        let exhaust_z = positions.iter().sum::<f32>() / positions.len() as f32;
        let (material, defaults) = self.materials.get("engine")?;
        let chance = |key| {
            defaults
                .get_child_float(key)
                .unwrap_or_default()
                .clamp(0.0, 1.0)
        };
        Some(EngineCompartment {
            exhaust_z,
            device: ArmorDevice {
                device: Device::Engine,
                can_ignite_engine: true,
                projectile_chance: chance("chanceToHitByProjectile"),
                explosion_chance: chance("chanceToHitByExplosion"),
                health: None,
            },
            material: *material,
        })
    }

    pub(super) fn shell_ballistics(
        &self,
        local: Option<&Element>,
        shared: Option<&Element>,
        shell: &Element,
        artillery: bool,
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
        let pair = |path: &[&str]| {
            let value = property(path)?;
            let values = value
                .as_vec2()
                .map(|v| v.to_array())
                .or_else(|| value.as_float().map(|value| [value; 2]))?;
            values
                .iter()
                .all(|value| (0.0..=65_535.0).contains(value))
                .then_some(values)
        };
        let modern = text("mechanics") == Some("MODERN");
        let armor_damage = pair(&["damage", "armor"])?;
        let spalls = pair(&["armorSpalls", "damage", "armor"]).unwrap_or_default();
        let kind = match text("kind")? {
            "ARMOR_PIERCING" | "ARMOR_PIERCING_CR" | "ARMOR_PIERCING_HE" => {
                ShellKind::ArmorPiercing
            }
            "HOLLOW_CHARGE" => ShellKind::HollowCharge {
                jet_loss_per_metre: 10.0
                    * float(&["piercingPowerLossFactorByDistance"]).unwrap_or(0.05),
            },
            "HIGH_EXPLOSIVE" => ShellKind::HighExplosive {
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
        let speed = shot(&["speed"])?.as_float()? * self.projectile_speed_factor;
        let caliber_mm = float(&["caliber"])?;
        if !penetration_mm
            .iter()
            .all(|value| (0.0..=10_000.0).contains(value))
            || !(1.0..=10_000.0).contains(&speed)
            || !(1.0..=1_000.0).contains(&caliber_mm)
        {
            return None;
        }
        let mut range = shell_zero_distance(penetration_mm).min(shell_zero_distance(armor_damage));
        if let Some(device_damage) = pair(&["damage", "devices"]) {
            range = range.min(shell_zero_distance(device_damage));
        }
        if modern {
            for impact in ["blastWave", "shellFragments", "armorSpalls"] {
                for damage in ["armor", "devices"] {
                    if let Some(values) = pair(&[impact, "damage", damage]) {
                        range = range.min(shell_zero_distance(values));
                    }
                }
            }
        }
        let explosive = matches!(kind, ShellKind::HighExplosive { .. }).then(|| {
            let radius = float(&["explosionRadius"])
                .filter(|r| *r > 0.0)
                .unwrap_or_else(|| {
                    if modern {
                        0.0
                    } else {
                        caliber_mm * caliber_mm / 5555.0
                    }
                })
                .clamp(0.0, 100.0);
            let [splash, absorption, edge, fragments] = self.explosion_factors;
            let factor = |key, fallback| float(&[key]).filter(|v| *v > 0.0).unwrap_or(fallback);
            let [duration, guaranteed, coefficient, minimum, turret_traverse] = self.stun_defaults;
            let stun = (artillery
                && property(&["hasStun"]).and_then(Value::as_boolean) == Some(true))
            .then(|| ShellStun {
                radius: factor("stunRadius", radius).clamp(0.0, 100.0),
                duration: factor("stunDuration", duration).clamp(0.0, 120.0),
                guaranteed: float(&["guaranteedStunDuration"])
                    .unwrap_or(guaranteed)
                    .clamp(0.0, 1.0),
                damage_coefficient: float(&["damageDurationCoeff"])
                    .unwrap_or(coefficient)
                    .clamp(0.0, 1.0),
                minimum,
                turret_traverse: turret_traverse.clamp(0.0, 1.0),
            });
            ExplosiveShell {
                scenery_radius: radius.max(
                    float(&["blastWave", "impactRadius"])
                        .unwrap_or_default()
                        .clamp(0.0, 100.0),
                ),
                artillery_radius: if artillery { radius } else { 0.0 },
                splash_factor: factor("explosionDamageFactor", splash).clamp(0.0, 1.0),
                edge_factor: factor("explosionEdgeDamageFactor", edge).clamp(0.0, 1.0),
                armor_absorption: factor("explosionDamageAbsorptionFactor", absorption)
                    .clamp(0.0, 100.0),
                fragment_damage: pair(&["shellFragments", "damage", "armor"]).unwrap_or_default(),
                fragment_absorption: factor("shellFragmentsDamageAbsorptionFactor", fragments)
                    .clamp(0.0, 100.0),
                spall_damage: spalls,
                spall_radius: float(&["armorSpalls", "impactRadius"])
                    .unwrap_or(0.0)
                    .clamp(0.0, 30.0),
                spall_cone_cos: float(&["armorSpalls", "coneAngle"])
                    .unwrap_or(90.0)
                    .clamp(0.0, 180.0)
                    .to_radians()
                    .cos(),
                piercing_spalls: property(&["armorSpalls", "piercingSpalls"])
                    .and_then(Value::as_boolean)
                    .unwrap_or(true),
                spall_device_damage: pair(&["armorSpalls", "damage", "devices"])
                    .unwrap_or_default(),
                stun,
            }
        });
        Some(ShellBallistics {
            kind,
            caliber_mm,
            penetration_mm,
            speed,
            gravity: if artillery {
                shot(&["gravity"])
                    .and_then(Value::as_float)
                    .filter(|value| (0.0..=10_000.0).contains(value))
                    .unwrap_or_default()
                    * self.projectile_speed_factor.powi(2)
            } else {
                0.0
            },
            penetration_randomization: float(&["piercingPowerRandomization"])
                .unwrap_or(0.25)
                .clamp(0.0, 1.0),
            damage_randomization: float(&["damageRandomization"])
                .unwrap_or(0.25)
                .clamp(0.0, 1.0),
            uniform_penetration: text("piercingPowerRandomizationType") == Some("UNIFORM"),
            uniform_damage: text("damageRandomizationType") == Some("UNIFORM"),
            far_damage: armor_damage[1],
            damage_limited_range: range,
            device_damage: pair(&["damage", "devices"]).unwrap_or_default(),
            explosive,
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
}

fn exhaust_positions(
    root: &Element,
    parent: Affine3A,
    exhaust: &str,
    positions: &mut Vec<f32>,
    depth: u8,
) {
    if depth >= 16 {
        return;
    }
    for (name, value) in root.iter_children_all() {
        if name != "node" {
            continue;
        }
        let Some(node) = value.as_element() else {
            continue;
        };
        let Some(transform) = node
            .get_child_value("transform")
            .and_then(Value::as_vector)
            .and_then(|value| value.as_affine3())
        else {
            continue;
        };
        let absolute = parent * transform;
        if let Some(identifier) = node.get_child_string("identifier")
            && exhaust
                .split_whitespace()
                .any(|name| name == identifier.trim())
            && absolute.translation.z.is_finite()
        {
            positions.push(absolute.translation.z);
        }
        exhaust_positions(node, absolute, exhaust, positions, depth + 1);
    }
}

fn device_kind(name: &str, id: u32) -> Option<Device> {
    Some(match name {
        "leftTrack" => Device::TrackLeft,
        "rightTrack" => Device::TrackRight,
        "wheel" => Device::Wheel,
        "engine" | "transmission" => Device::Engine,
        "fuelTank" => Device::Fuel,
        "ammoBay" => Device::AmmoRack,
        "gun" | "gun2" => Device::Gun,
        "turretRotator" => Device::Turret,
        "surveyingDevice" => Device::Optics,
        "radio" => Device::Radio,
        "commander" | "driver" | "gunner" | "gunner1" | "gunner2" | "loader" | "loader1"
        | "loader2" | "radioman" | "radioman1" | "radioman2" => Device::Crew(id),
        _ => return None,
    })
}

pub(super) fn read_device_health(local: Option<&Element>, base: &Element) -> Option<DeviceHealth> {
    let number = |key| {
        local
            .and_then(|l| l.get_child_float(key))
            .or_else(|| base.get_child_float(key))
    };
    let maximum = number("maxHealth").filter(|v| (1.0..=65_535.0).contains(v))? as u16;
    let repaired =
        number("maxRegenHealth").filter(|v| (1.0..=f32::from(maximum)).contains(v))? as u16;
    Some(DeviceHealth {
        maximum,
        repaired,
        repair_seconds: number("repairTime")
            .filter(|v| (0.1..=120.0).contains(v))
            .unwrap_or(10.0),
        fire_chance: number("fireStartingChance")
            .filter(|v| (0.0..=1.0).contains(v))
            .unwrap_or(0.0),
    })
}
