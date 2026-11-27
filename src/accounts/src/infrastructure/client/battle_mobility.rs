use super::{
    CatalogVehicle, account::Vehicle, equipment::words, item_defs::read_xml, items::IntCompactDescr,
};
use crate::{CrewMember, CrewRoster, MAX_EFFICIENCY_XP, ModuleLoadout, perk_role};
use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};
use wot_platform::client_formats::pxml::Element;
use wot_resources::ResourceBundle;
use wot_shared_kernel::{Device, VehiclePerformance};

const DRIVER: u8 = 3;
const REPAIRS: u8 = 6;
const FULL_CREW_REPAIRS_BONUS: f32 = 0.8;
const BROTHERS_IN_ARMS: u8 = 9;
const CLUTCH_BRAKING: u8 = 28;
const OFF_ROAD_DRIVING: u8 = 30;
const ENGINEER: u8 = 33;

pub(super) struct BattleMobility {
    devices: BTreeMap<u16, Item>,
    supplies: BTreeMap<u16, Item>,
    mobility_categories: BTreeSet<String>,
}

struct Item {
    name: String,
    categories: BTreeSet<String>,
    weight: f32,
    base: Modifiers,
    matching_slot: Modifiers,
}

#[derive(Clone, Copy)]
struct Modifiers {
    power: f32,
    forward_kmh: f32,
    reverse_kmh: f32,
    traverse: f32,
    turret_traverse: f32,
    turning_resistance: f32,
    rolling_friction: f32,
    crew_bonus: f32,
    health: f32,
    repair_speed: f32,
    chassis_repair_speed: f32,
}

impl Default for Modifiers {
    fn default() -> Self {
        Self {
            power: 1.0,
            forward_kmh: 0.0,
            reverse_kmh: 0.0,
            traverse: 1.0,
            turret_traverse: 1.0,
            turning_resistance: 1.0,
            rolling_friction: 1.0,
            crew_bonus: 0.0,
            health: 1.0,
            repair_speed: 1.0,
            chassis_repair_speed: 1.0,
        }
    }
}

impl Modifiers {
    fn combine(&mut self, other: Self) {
        self.power *= other.power;
        self.forward_kmh += other.forward_kmh;
        self.reverse_kmh += other.reverse_kmh;
        self.traverse *= other.traverse;
        self.turret_traverse *= other.turret_traverse;
        self.turning_resistance *= other.turning_resistance;
        self.rolling_friction *= other.rolling_friction;
        self.crew_bonus += other.crew_bonus;
        self.health *= other.health;
        self.repair_speed *= other.repair_speed;
        self.chassis_repair_speed *= other.chassis_repair_speed;
    }
}

impl BattleMobility {
    pub(super) fn load(fs: &ResourceBundle) -> Result<Self> {
        let devices = read_xml(fs, "scripts/item_defs/vehicles/common/optional_devices.xml")?;
        let supplies = read_xml(fs, "scripts/item_defs/vehicles/common/equipments.xml")?;
        let devices = read_items(&devices);
        let mobility_categories = devices
            .values()
            .filter(|item| item.name.starts_with("grousers_"))
            .flat_map(|item| item.categories.iter().cloned())
            .collect();
        Ok(Self {
            devices,
            supplies: read_items(&supplies),
            mobility_categories,
        })
    }

    fn equipment_modifiers(&self, item: &Item, categories: &BTreeSet<String>) -> Modifiers {
        if categories.iter().any(|category| {
            item.categories.contains(category)
                || (item.categories.contains("mobility")
                    && self.mobility_categories.contains(category))
        }) {
            item.matching_slot
        } else {
            item.base
        }
    }

    pub(super) fn equipment_health_factors(&self, id: i32, catalog: &CatalogVehicle) -> Vec<f32> {
        let item = self.devices.get(&raw_id(id));
        catalog
            .supply_slots
            .optional_device_categories
            .iter()
            .map(|categories| {
                item.map_or(1.0, |item| {
                    self.equipment_modifiers(item, categories).health
                })
            })
            .collect()
    }

    pub(super) fn consumable_health_factor(&self, id: i32) -> f32 {
        self.supplies
            .get(&raw_id(id))
            .map_or(1.0, |item| item.base.health)
    }

    #[tracing::instrument(name = "accounts.battle_mobility", skip_all, fields(
        ddd.bounded_context.name = "accounts", input.inventory_id = inventory_id,
        input.mobility.equipment_count = vehicle.equipment.iter().filter(|&&id| id != 0).count(),
        input.mobility.consumable_count = vehicle.consumables.iter().filter(|&&id| id != 0).count(),
        input.mobility.directive_id = vehicle.directive,
        output.vehicle.health_factor = tracing::field::Empty,
        output.tracks.repair_speed_factor = tracing::field::Empty,
        output.mobility.collision_bounds_available = performance.mobility.collision_bounds.is_some(),
        output.mobility.equipment_mass_kg = tracing::field::Empty,
        output.mobility.power_watts_per_kg = tracing::field::Empty,
        output.mobility.forward_speed_mps = tracing::field::Empty,
        output.mobility.reverse_speed_mps = tracing::field::Empty,
        output.mobility.hull_traverse_rad_per_second = tracing::field::Empty,
    ))]
    pub(super) fn apply(
        &self,
        performance: &mut VehiclePerformance,
        vehicle: &Vehicle,
        catalog: &CatalogVehicle,
        crew: Option<&CrewRoster>,
        inventory_id: i64,
        loadout: &ModuleLoadout,
    ) -> f32 {
        let mut modifiers = Modifiers::default();
        let mut equipment_mass = 0.0;
        let mut turbocharger = false;
        let mut ventilation = false;
        for (&id, categories) in vehicle
            .equipment
            .iter()
            .zip(&catalog.supply_slots.optional_device_categories)
        {
            if id == 0 {
                continue;
            }
            let Some(item) = self.devices.get(&raw_id(id)) else {
                continue;
            };
            equipment_mass += item.weight;
            turbocharger |= item.name.to_ascii_lowercase().contains("turbocharger");
            ventilation |= item.name.to_ascii_lowercase().contains("ventilation");
            modifiers.combine(self.equipment_modifiers(item, categories));
        }
        for &id in vehicle.consumables.iter().filter(|&&id| id != 0) {
            if let Some(item) = self.supplies.get(&raw_id(id)) {
                modifiers.combine(item.base);
            }
        }
        if let Some(item) = vehicle
            .directive
            .and_then(|id| self.supplies.get(&raw_id(id)))
        {
            match item.name.as_str() {
                "turbochargerBattleBooster" if turbocharger => modifiers.power *= item.base.power,
                "improvedVentilationBattleBooster" if ventilation => {
                    modifiers.crew_bonus += item.base.crew_bonus
                }
                _ => {}
            }
        }
        let mass_factor = catalog
            .modules
            .rules
            .loadout_weight(loadout)
            .map_or(1.0, |mass| {
                if mass.is_finite() && mass > 0.0 {
                    mass / (mass + equipment_mass)
                } else {
                    1.0
                }
            });
        let mobility = &mut performance.mobility;
        mobility.mass_kg += equipment_mass;
        let power_factor = modifiers.power * mass_factor;
        mobility.power_per_kg *= power_factor;
        performance.hull_traverse *= power_factor * modifiers.traverse;
        mobility.turning_resistance *= modifiers.turning_resistance;
        mobility.rolling_friction = mobility
            .rolling_friction
            .map(|v| v * modifiers.rolling_friction);
        performance.forward_speed += modifiers.forward_kmh / 3.6;
        performance.reverse_speed += modifiers.reverse_kmh / 3.6;
        performance.gun.turret_traverse *= modifiers.turret_traverse;

        let mut repair_speed = modifiers.repair_speed * modifiers.chassis_repair_speed;
        if let Some(roster) = crew {
            let seats = roster.seats(inventory_id, catalog.crew_roles.len());
            let members = seats.iter().map(|id| {
                id.and_then(|id| roster.members.get(&id))
                    .filter(|member| member.dismissed_at.is_none())
            });
            let brothers = members
                .clone()
                .flatten()
                .map(|member| perk(member, BROTHERS_IN_ARMS, 1))
                .sum::<f32>()
                / catalog.crew_roles.len().max(1) as f32;
            let crew_bonus = (modifiers.crew_bonus + 5.0 * brothers) / 100.0;
            let repairs = members
                .clone()
                .flatten()
                .map(|member| perk(member, REPAIRS, member.role))
                .sum::<f32>()
                / seats.len().max(1) as f32;
            repair_speed *= 1.0 + FULL_CREW_REPAIRS_BONUS * repairs * (1.0 + crew_bonus);
            if let Some((role, driver)) =
                catalog
                    .crew_roles
                    .iter()
                    .zip(members)
                    .find_map(|(roles, member)| {
                        let primary = *roles.first()?;
                        roles.contains(&DRIVER).then_some((primary, member?))
                    })
            {
                mobility.terrain_resistance = mobility
                    .terrain_resistance
                    .map(|value| value / (1.0 + 0.43 * crew_bonus));
                performance.hull_traverse *= 1.0 + 0.05 * perk(driver, CLUTCH_BRAKING, role);
                let engineer_kmh = perk(driver, ENGINEER, role);
                performance.forward_speed += engineer_kmh / 3.6;
                performance.reverse_speed += engineer_kmh / 3.6;
                let off_road = perk(driver, OFF_ROAD_DRIVING, role);
                let [hard, medium, soft] = &mut mobility.terrain_resistance;
                *medium -= (*medium - *hard).max(0.0) * (0.05 * off_road);
                *soft -= (*soft - *medium).max(0.0) * off_road;
            }
        }
        for track in [Device::TrackLeft, Device::TrackRight] {
            if let Some(health) = performance.device_health.get_mut(&track) {
                health.repair_seconds = (health.repair_seconds / repair_speed).clamp(0.1, 120.0);
            }
        }
        let span = tracing::Span::current();
        span.record("output.tracks.repair_speed_factor", repair_speed);
        span.record("output.mobility.equipment_mass_kg", equipment_mass);
        span.record("output.mobility.power_watts_per_kg", mobility.power_per_kg);
        span.record(
            "output.mobility.forward_speed_mps",
            performance.forward_speed,
        );
        span.record(
            "output.mobility.reverse_speed_mps",
            performance.reverse_speed,
        );
        span.record(
            "output.mobility.hull_traverse_rad_per_second",
            performance.hull_traverse,
        );
        span.record("output.vehicle.health_factor", modifiers.health);
        modifiers.health
    }
}

fn perk(member: &CrewMember, perk: u8, seat_role: u8) -> f32 {
    let training = &member.training;
    let level = if seat_role == DRIVER || perk_role(perk) == Some(None) {
        training
            .perks
            .iter()
            .position(|&id| id == perk)
            .map_or(0.0, |index| {
                if index + 1 == training.perks.len() {
                    f32::from(training.last_perk_level.min(100)) / 100.0
                } else {
                    1.0
                }
            })
    } else if let Some(index) = training.bonus_perks.iter().position(|&id| id == perk) {
        // Bonus perks acquire one percent of training per two percent of major perk training.
        let major_progress = training.perks.len().saturating_sub(1) as f32
            + f32::from(training.last_perk_level.min(100)) / 100.0;
        (major_progress * 0.5 - index as f32).clamp(0.0, 1.0)
    } else {
        0.0
    };
    level * training.efficiency_xp.min(MAX_EFFICIENCY_XP) as f32 / MAX_EFFICIENCY_XP as f32
}

fn raw_id(id: i32) -> u16 {
    IntCompactDescr::from_int(i64::from(id)).id
}

fn read_items(definitions: &Element) -> BTreeMap<u16, Item> {
    definitions
        .iter_child_elements()
        .filter_map(|(name, item)| {
            if words(item, "tags").contains("trigger") {
                return None;
            }
            let id = u16::try_from(item.get_child_integer("id")?).ok()?;
            let script = item.get_child_element("script");
            let weight = script
                .and_then(|s| s.get_child_float("weight"))
                .filter(|v| (0.0..=10_000.0).contains(v))
                .unwrap_or(0.0);
            Some((
                id,
                Item {
                    name: name.to_owned(),
                    categories: words(item, "categories"),
                    weight,
                    base: read_modifiers(item, false),
                    matching_slot: read_modifiers(item, true),
                },
            ))
        })
        .collect()
}

fn read_modifiers(item: &Element, specialized: bool) -> Modifiers {
    let mut modifiers = Modifiers::default();
    if let Some(kpi) = item.get_child_element("kpi") {
        for (operation, stat) in kpi.iter_child_elements() {
            let Some(name) = stat.get_child_string("name") else {
                continue;
            };
            let value = specialized
                .then(|| stat.get_child_float("specValue"))
                .flatten()
                .or_else(|| stat.get_child_float("value"));
            let Some(value) = value.filter(|v| (0.01..=30.0).contains(v)) else {
                continue;
            };
            match (operation, name) {
                ("mul", "vehicleEnginePower") => modifiers.power *= value,
                ("add", "vehicleForwardMaxSpeed") => modifiers.forward_kmh += value,
                ("add", "vehicleBackwardMaxSpeed") => modifiers.reverse_kmh += value,
                ("mul", "vehicleAllGroundRotationSpeed") => modifiers.traverse *= value,
                ("mul", "vehicleTurretRotationSpeed" | "vehicleTurretOrCuttingRotationSpeed") => {
                    modifiers.turret_traverse *= value
                }
                ("mul", "crewLevel") => modifiers.crew_bonus += (value - 1.0) * 100.0,
                ("mul", "vehicleRepairSpeed") => modifiers.repair_speed *= value,
                ("mul", "vehicleChassisRepairSpeed") => modifiers.chassis_repair_speed *= value,
                _ => {}
            }
        }
    }
    if let Some(script) = item.get_child_element("script") {
        let factor = |field| {
            let values = script.get_child_value(field)?;
            let level = usize::from(specialized);
            let value = values
                .as_vec2()
                .map(|pair| pair.to_array()[level])
                .or_else(|| values.as_float())?;
            (0.01..=3.0).contains(&value).then_some(value)
        };
        // Rotation equipment uses the script's chassis modifier even where the KPI is UI-only.
        modifiers.traverse = factor("trackRotateSpeedFactor").unwrap_or(modifiers.traverse);
        modifiers.turning_resistance = factor("rotationFactor").unwrap_or(1.0);
        modifiers.rolling_friction = factor("rollingFrictionFactor").unwrap_or(1.0);
        modifiers.health = script
            .get_child_element("factors")
            .and_then(|factors| {
                factors.iter_child_elements().find_map(|(_, factor)| {
                    if factor.get_child_string("attribute")?.trim() != "miscAttrs/healthFactor"
                        || factor.get_child_string("type")?.trim() != "mul"
                    {
                        return None;
                    }
                    let value = factor.get_child("valueByLevel")?;
                    let value = value
                        .as_vec2()
                        .map(|pair| pair.to_array()[usize::from(specialized)])
                        .or_else(|| value.as_float())?;
                    (0.01..=3.0).contains(&value).then_some(value)
                })
            })
            .unwrap_or(1.0);
    }
    modifiers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AccountSnapshot, Balances, CrewPassport, CrewSeat, CrewTraining, OwnedVehicle};
    use std::path::Path;

    #[test]
    #[ignore = "Requires WOT_RESOURCE_DIR with the extracted EU client bundle"]
    fn spotting_uses_the_mounted_su_152_gun_and_client_geometry() {
        let root = std::env::var_os("WOT_RESOURCE_DIR").unwrap();
        let resources = ResourceBundle::open(Path::new(&root)).unwrap();
        let catalog = super::super::GarageCatalog::load(resources, &[]).unwrap();
        let vehicle = catalog.vehicle("ussr:R18_SU-152").unwrap();
        let spotting = vehicle
            .modules
            .battle_performance(&vehicle.modules.rules.stock)
            .unwrap()
            .spotting
            .unwrap();
        assert!(spotting.view_range > 0.0 && spotting.radio_range > 0.0);
        assert!(spotting.hull_bounds[0].cmplt(spotting.hull_bounds[1]).all());
        let mut multipliers = Vec::new();
        for (&id, module) in &vehicle.modules.rules.modules {
            if module.kind == crate::ModuleKind::Gun {
                let mut loadout = vehicle.modules.rules.stock.clone();
                loadout.insert(crate::ModuleKind::Gun, id);
                let equipped = vehicle
                    .modules
                    .battle_performance(&loadout)
                    .unwrap()
                    .spotting
                    .unwrap();
                multipliers.push(equipped.firing_multiplier);
            }
        }
        multipliers.sort_by(f32::total_cmp);
        assert_eq!(multipliers, [0.143, 0.19, 0.2]);
    }

    #[test]
    #[ignore = "Requires WOT_RESOURCE_DIR with extracted AMX 30 B contact bounds"]
    fn amx_30_b_contact_footprint_includes_the_rear_overhang() {
        let root = std::env::var_os("WOT_RESOURCE_DIR").unwrap();
        let resources = ResourceBundle::open(Path::new(&root)).unwrap();
        let catalog = super::super::GarageCatalog::load(resources, &[]).unwrap();
        let vehicle = catalog.vehicle("france:F72_AMX_30").unwrap();
        let mobility = vehicle
            .modules
            .battle_performance(&vehicle.modules.rules.stock)
            .unwrap()
            .mobility;
        let [rear, front] = mobility.collision_bounds.unwrap();
        assert!(rear.z < -3.3 && front.z > 3.1);
        assert!(rear.z < -mobility.track_half_length - 1.0);
        assert!(rear.y < 0.05 && (1.5..2.0).contains(&front.y));
    }
    #[test]
    #[ignore = "Requires WOT_RESOURCE_DIR with the extracted EU client bundle"]
    fn training_loadout_applies_mobility_equipment_fuel_and_driver_perks() {
        let root = std::env::var_os("WOT_RESOURCE_DIR").unwrap();
        let resources = ResourceBundle::open(Path::new(&root)).unwrap();
        let catalog = super::super::GarageCatalog::load(resources, &[]).unwrap();
        let (key, inventory_id) = ("germany:G54_E-50", 1);
        let vehicle = catalog.vehicle(key).unwrap();
        let mut snapshot = AccountSnapshot {
            id: 1,
            display_name: "mobility test".to_owned(),
            revision: 0,
            balances: Balances::default(),
            crew: None,
            vehicles: vec![OwnedVehicle {
                inventory_id,
                catalog_key: key.to_owned(),
                ammunition: None,
                has_crew: false,
            }],
            depot_shells: Default::default(),
            progression: Default::default(),
            settings: Default::default(),
        };
        let base = catalog
            .battle_vehicle(&snapshot, inventory_id)
            .unwrap()
            .unwrap()
            .performance;
        let turbo = catalog
            .battle_mobility
            .devices
            .iter()
            .find(|(_, item)| item.name == "turbocharger_tier1")
            .unwrap();
        let fuel = catalog
            .battle_mobility
            .supplies
            .iter()
            .find(|(_, item)| item.name == "qualityFuel")
            .unwrap();
        let compact = |id, item_type| {
            IntCompactDescr {
                id,
                item_type,
                nation: 15,
            }
            .to_int() as i32
        };
        let mut layout = [0; crate::MAX_EQUIPMENT_SLOTS];
        let slot = vehicle
            .supply_slots
            .optional_device_categories
            .iter()
            .position(|categories| !categories.is_disjoint(&turbo.1.categories))
            .expect("E 50 mobility slot");
        layout[slot] = compact(*turbo.0, super::super::items::item_type::OPTIONAL_DEVICE);
        snapshot.progression.equipment.insert(inventory_id, layout);
        let mut consumables = crate::VehicleConsumables::default();
        consumables.layout[0] = compact(*fuel.0, super::super::items::item_type::EQUIPMENT);
        snapshot
            .progression
            .set_vehicle_consumables(inventory_id, consumables);
        let equipped = catalog
            .battle_vehicle(&snapshot, inventory_id)
            .unwrap()
            .unwrap()
            .performance;
        assert!((equipped.forward_speed - base.forward_speed - 5.0 / 3.6).abs() < 1e-4);
        assert!((equipped.reverse_speed - base.reverse_speed - 3.0 / 3.6).abs() < 1e-4);
        assert!(equipped.mobility.power_per_kg > base.mobility.power_per_kg * 1.12);
        assert!(equipped.gun.turret_traverse > base.gun.turret_traverse);

        let (driver_slot, roles) = vehicle
            .crew_roles
            .iter()
            .enumerate()
            .find(|(_, roles)| roles.first() == Some(&DRIVER))
            .unwrap();
        let mut roster = CrewRoster::default();
        roster.members.insert(
            1,
            CrewMember {
                nation: 0,
                specialization: 0,
                role: roles[0],
                passport: CrewPassport {
                    first_name: 0,
                    last_name: 0,
                    portrait: 0,
                },
                seat: Some(CrewSeat {
                    vehicle: inventory_id,
                    slot: driver_slot,
                }),
                training: CrewTraining {
                    perks: vec![REPAIRS, CLUTCH_BRAKING, OFF_ROAD_DRIVING, ENGINEER],
                    last_perk_level: 50,
                    ..Default::default()
                },
                dismissed_at: None,
                role_change_target: None,
                skin: None,
                special_recruit: None,
                female: false,
            },
        );
        snapshot.crew = Some(roster);
        let trained = catalog
            .battle_vehicle(&snapshot, inventory_id)
            .unwrap()
            .unwrap()
            .performance;
        let expected_speedup = 1.0 + 0.8 / vehicle.crew_roles.len() as f32;
        for track in [Device::TrackLeft, Device::TrackRight] {
            let original = equipped.device_health[&track].repair_seconds;
            let trained = trained.device_health[&track].repair_seconds;
            assert!((original / trained - expected_speedup).abs() < 1e-4);
        }

        assert!((trained.forward_speed - equipped.forward_speed - 0.5 / 3.6).abs() < 1e-4);
        assert!(trained.hull_traverse > equipped.hull_traverse);
        assert!(trained.mobility.terrain_resistance[2] <= trained.mobility.terrain_resistance[1]);
    }
}
