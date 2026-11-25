use super::{CatalogVehicle, GarageCatalog, localization};
use crate::{
    AccountSnapshot, AmmunitionRules, CrewRoster, CrewSeat, MAX_CONSUMABLE_SLOTS,
    MAX_EFFICIENCY_XP, MAX_EQUIPMENT_SLOTS, MAX_MAJOR_PERKS, ModuleKind, ModuleLoadout,
    OwnedVehicle, VehicleAmmunition,
    contract::{BattleVehicle, BattleVehicleUnavailable},
    perk_role,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use wot_resources::ResourceBundle;

const VEHICLE: i64 = 1;
const ROLES: [&str; 6] = [
    "Crew member",
    "Commander",
    "Radio Operator",
    "Driver",
    "Gunner",
    "Loader",
];

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomBattleLoadout {
    pub vehicle: i32,
    pub equipment: Option<Vec<i32>>,
    pub consumables: Option<Vec<i32>>,
    #[serde(default)]
    pub crew_perks: Vec<Option<Vec<u8>>>,
    #[serde(default)]
    pub crew_perk_efficiency: Vec<Option<u8>>,
    pub hit_points: Option<i16>,
    pub ammunition: Option<Vec<(i32, u32)>>,
    #[serde(default)]
    pub modules: ModuleLoadout,
}

#[derive(Serialize)]
pub struct CustomBattleLoadoutOptions {
    equipment_slots: usize,
    consumable_slots: usize,
    equipment: Vec<LoadoutItem>,
    consumables: Vec<LoadoutItem>,
    equipment_health_factors: BTreeMap<i32, Vec<f32>>,
    consumable_health_factors: BTreeMap<i32, f32>,
    crew: Vec<CrewPosition>,
    stock_hit_points: i16,
    ammunition_capacity: u32,
    ammunition: Vec<LoadoutShell>,
    stock_modules: ModuleLoadout,
    modules: BTreeMap<ModuleKind, Vec<ModuleChoice>>,
    hull_weight: f32,
    weapons: Vec<WeaponOptions>,
    current: Option<CustomBattleLoadout>,
    equipped_hit_points: Option<i16>,
}

#[derive(Serialize)]
struct LoadoutItem {
    id: i32,
    name: String,
}

#[derive(Clone, Serialize)]
struct LoadoutShell {
    id: i32,
    name: String,
    default_count: u32,
}

#[derive(Serialize)]
struct ModuleChoice {
    id: i32,
    name: String,
    weight: f32,
    max_weight: Option<f32>,
}

#[derive(Serialize)]
struct WeaponOptions {
    turret: i32,
    gun: i32,
    hit_points: i16,
    ammunition_capacity: u32,
    ammunition: Vec<LoadoutShell>,
}

#[derive(Serialize)]
struct CrewPosition {
    name: &'static str,
    perks: Vec<LoadoutItem>,
}

fn eligible_perk(id: u8, role: u8) -> bool {
    perk_role(id).is_some_and(|required| required.is_none_or(|required| required == role))
}

fn default_custom_ammunition(rules: &AmmunitionRules) -> impl Iterator<Item = (i32, u32)> {
    rules.defaults.iter().enumerate().map(|(index, &(id, _))| {
        let count = if index == 0 { rules.capacity } else { 0 };
        (id, count)
    })
}

impl GarageCatalog {
    #[tracing::instrument(name = "accounts.custom_battle_loadout_options", skip_all, fields(
        ddd.bounded_context.name = "accounts", input.vehicle_type = vehicle,
        output.equipment.count = tracing::field::Empty, output.consumables.count = tracing::field::Empty,
        output.crew.count = tracing::field::Empty,
        output.ammunition.types = tracing::field::Empty,
        output.ammunition.capacity = tracing::field::Empty,
        output.modules.count = tracing::field::Empty, output.weapon_configurations.count = tracing::field::Empty,
        output.vehicle.hit_points = tracing::field::Empty,
    ), err)]
    pub(crate) fn custom_battle_loadout_options(
        &self,
        vehicle: i32,
    ) -> Result<Option<CustomBattleLoadoutOptions>> {
        let Some(offer) = self.offers.get(&vehicle).filter(|offer| offer.grantable()) else {
            return Ok(None);
        };
        self.battle_loadout_options(&offer.key, None, None)
            .map(Some)
    }

    #[tracing::instrument(name = "accounts.owned_battle_loadout_options", skip_all, fields(
        ddd.bounded_context.name = "accounts", user.id = snapshot.id, input.inventory_id = inventory_id,
        output.equipment.count = tracing::field::Empty, output.consumables.count = tracing::field::Empty,
        output.crew.count = tracing::field::Empty, output.ammunition.types = tracing::field::Empty,
        output.ammunition.capacity = tracing::field::Empty, output.vehicle.hit_points = tracing::field::Empty,
        output.modules.count = tracing::field::Empty, output.weapon_configurations.count = tracing::field::Empty,
        output.outcome = tracing::field::Empty,
    ), err)]
    pub(crate) fn owned_battle_loadout_options(
        &self,
        snapshot: &AccountSnapshot,
        inventory_id: i64,
    ) -> Result<Result<CustomBattleLoadoutOptions, BattleVehicleUnavailable>> {
        let span = tracing::Span::current();
        let battle_vehicle = match self.admin_battle_vehicle(snapshot, inventory_id, None)? {
            Ok(vehicle) => vehicle,
            Err(reason) => {
                span.record("output.outcome", reason.outcome());
                return Ok(Err(reason));
            }
        };
        let owned = snapshot
            .vehicles
            .iter()
            .find(|owned| owned.inventory_id == inventory_id)
            .context("Missing ready vehicle")?;
        let template = self.vehicle(&owned.catalog_key)?;
        let crew = self.crew_roster(snapshot)?;
        let seats = crew.seats(inventory_id, template.crew_roles.len());
        let members = seats
            .iter()
            .map(|id| id.and_then(|id| crew.members.get(&id)));
        let current = CustomBattleLoadout {
            vehicle: template.modules.rules.vehicle,
            equipment: Some(
                snapshot
                    .progression
                    .equipment_layout(inventory_id)
                    .into_iter()
                    .take(template.equipment.slots)
                    .collect(),
            ),
            consumables: Some(
                snapshot
                    .progression
                    .vehicle_consumables(inventory_id)
                    .layout
                    .into_iter()
                    .take(template.supply_slots.consumables)
                    .collect(),
            ),
            modules: template.modules.rules.loadout(snapshot, inventory_id),
            crew_perks: members
                .clone()
                .map(|member| member.map(|member| member.training.perks.clone()))
                .collect(),
            crew_perk_efficiency: members
                .map(|member| {
                    member.map(|member| {
                        (member.training.efficiency_xp.min(MAX_EFFICIENCY_XP)
                            / (MAX_EFFICIENCY_XP / 100)) as u8
                    })
                })
                .collect(),
            hit_points: None,
            ammunition: Some(
                battle_vehicle
                    .ammunition
                    .into_iter()
                    .map(|(id, count)| (id, u32::from(count)))
                    .collect(),
            ),
        };
        let options = self.battle_loadout_options(
            &owned.catalog_key,
            Some(current),
            Some(battle_vehicle.health),
        )?;
        span.record("output.outcome", "loaded");
        Ok(Ok(options))
    }

    fn battle_loadout_options(
        &self,
        key: &str,
        current: Option<CustomBattleLoadout>,
        equipped_hit_points: Option<i16>,
    ) -> Result<CustomBattleLoadoutOptions> {
        let template = self.vehicle(key)?;
        let stock = &template.modules.rules.stock;
        let (nation, _) = key.split_once(':').context("Missing vehicle nation")?;
        let names = localization::localized_names(
            &self.filesystem,
            &format!("text/lc_messages/{nation}_vehicles.mo"),
        )
        .unwrap_or_default();
        let localized_name = |key: Option<&String>| {
            let (_, key) = key?.split_once(':')?;
            names.get(key).filter(|name| !name.is_empty()).cloned()
        };
        let mut modules: BTreeMap<ModuleKind, Vec<ModuleChoice>> = BTreeMap::new();
        for (&id, module) in &template.modules.rules.modules {
            let choices = modules.entry(module.kind).or_default();
            let name =
                localized_name(template.modules.module_name_keys.get(&id)).unwrap_or_else(|| {
                    let kind = match module.kind {
                        ModuleKind::Chassis => "Suspension",
                        ModuleKind::Turret => "Turret",
                        ModuleKind::Gun => "Gun",
                        ModuleKind::Engine => "Engine",
                        ModuleKind::FuelTank => "Fuel tank",
                        ModuleKind::Radio => "Radio",
                    };
                    if stock.get(&module.kind) == Some(&id) {
                        format!("Stock {}", kind.to_lowercase())
                    } else {
                        format!("{kind} option {}", choices.len() + 1)
                    }
                });
            choices.push(ModuleChoice {
                id,
                name,
                weight: module.weight,
                max_weight: module.max_weight,
            });
        }
        let weapons: Vec<_> = template
            .modules
            .ammunition
            .iter()
            .filter_map(|(&(turret, gun), rules)| {
                let hit_points = *template.modules.training_health.get(&turret)?;
                let ammunition = default_custom_ammunition(rules)
                    .enumerate()
                    .map(|(index, (id, default_count))| LoadoutShell {
                        id,
                        default_count,
                        name: localized_name(template.modules.shell_name_keys.get(&id))
                            .unwrap_or_else(|| format!("Shell type {}", index + 1)),
                    })
                    .collect();
                Some(WeaponOptions {
                    turret,
                    gun,
                    hit_points,
                    ammunition_capacity: rules.capacity,
                    ammunition,
                })
            })
            .collect();
        let stock_weapon = weapons
            .iter()
            .find(|weapon| {
                stock.get(&ModuleKind::Turret) == Some(&weapon.turret)
                    && stock.get(&ModuleKind::Gun) == Some(&weapon.gun)
            })
            .context("Missing stock weapon loadout")?;
        let stock_hit_points = stock_weapon.hit_points;
        let ammunition_capacity = stock_weapon.ammunition_capacity;
        let ammunition = stock_weapon.ammunition.clone();
        let equipment = self
            .equipment_grants
            .names
            .iter()
            .filter(|&(&id, _)| {
                self.equipment
                    .validate(&template.equipment, &[id, 0, 0, 0])
                    .is_ok()
            })
            .map(|(&id, name)| LoadoutItem {
                id,
                name: name.clone(),
            })
            .collect::<Vec<_>>();
        let consumables = self
            .consumable_names
            .iter()
            .filter(|&(&id, _)| {
                self.consumables
                    .validate(&template.equipment, &[id, 0, 0])
                    .is_ok()
            })
            .map(|(&id, name)| LoadoutItem {
                id,
                name: name.clone(),
            })
            .collect::<Vec<_>>();
        let equipment_health_factors = equipment
            .iter()
            .map(|item| item.id)
            .chain(
                current
                    .iter()
                    .flat_map(|loadout| loadout.equipment.iter().flatten().copied()),
            )
            .map(|id| {
                (
                    id,
                    self.battle_mobility.equipment_health_factors(id, &template),
                )
            })
            .filter(|(_, factors)| factors.iter().any(|&factor| factor != 1.0))
            .collect();
        let consumable_health_factors = consumables
            .iter()
            .map(|item| item.id)
            .chain(
                current
                    .iter()
                    .flat_map(|loadout| loadout.consumables.iter().flatten().copied()),
            )
            .map(|id| (id, self.battle_mobility.consumable_health_factor(id)))
            .filter(|&(_, factor)| factor != 1.0)
            .collect();
        let crew: Vec<_> = template
            .crew_roles
            .iter()
            .filter_map(|roles| {
                let &role = roles.first()?;
                Some(CrewPosition {
                    name: ROLES.get(usize::from(role)).copied().unwrap_or(ROLES[0]),
                    perks: self
                        .perk_names
                        .iter()
                        .filter(|&(&id, _)| eligible_perk(id, role))
                        .map(|(&id, name)| LoadoutItem {
                            id: id.into(),
                            name: name.clone(),
                        })
                        .collect(),
                })
            })
            .collect();
        let span = tracing::Span::current();
        span.record("output.equipment.count", equipment.len());
        span.record("output.consumables.count", consumables.len());
        span.record("output.crew.count", crew.len());
        span.record("output.ammunition.types", ammunition.len());
        span.record("output.ammunition.capacity", ammunition_capacity);
        span.record(
            "output.vehicle.hit_points",
            equipped_hit_points.unwrap_or(stock_hit_points),
        );
        span.record(
            "output.modules.count",
            modules.values().map(Vec::len).sum::<usize>(),
        );
        span.record("output.weapon_configurations.count", weapons.len());
        Ok(CustomBattleLoadoutOptions {
            stock_hit_points,
            ammunition_capacity,
            ammunition,
            equipment_slots: template.equipment.slots,
            consumable_slots: template.supply_slots.consumables,
            equipment,
            consumables,
            equipment_health_factors,
            consumable_health_factors,
            crew,
            stock_modules: stock.clone(),
            modules,
            hull_weight: template.modules.rules.hull_weight,
            weapons,
            current,
            equipped_hit_points,
        })
    }

    #[tracing::instrument(name = "accounts.custom_battle_vehicle", skip_all, fields(
        ddd.bounded_context.name = "accounts", input.vehicle_type = loadout.vehicle, user.id = account_id,
        input.equipment.count = loadout.equipment.as_ref().map(Vec::len), input.consumables.count = loadout.consumables.as_ref().map(Vec::len),
        input.modules.count = loadout.modules.len(),
        input.crew.count = loadout.crew_perks.len(),
        input.crew.perk_efficiency_min = loadout.crew_perk_efficiency.iter().flatten().min().copied(),
        input.vehicle.hit_points = loadout.hit_points,
        input.ammunition.types = loadout.ammunition.as_ref().map(Vec::len),
        output.vehicle.hit_points = tracing::field::Empty,
        output.ammunition.rounds = tracing::field::Empty,
        output.outcome = tracing::field::Empty,
        otel.status_code = tracing::field::Empty,
    ), err)]
    pub(crate) fn custom_battle_vehicle(
        &self,
        loadout: &CustomBattleLoadout,
        account_id: Option<i64>,
    ) -> Result<Option<BattleVehicle>> {
        let span = tracing::Span::current();
        let result = self.build_custom_battle_vehicle(loadout);
        span.record(
            "output.outcome",
            match &result {
                Ok(Some(_)) => "prepared",
                Ok(None) => "invalid_loadout",
                Err(_) => "projection_failed",
            },
        );
        if let Ok(Some(vehicle)) = &result {
            span.record("output.vehicle.hit_points", vehicle.health);
            span.record(
                "output.ammunition.rounds",
                vehicle
                    .ammunition
                    .iter()
                    .map(|&(_, count)| u64::from(count))
                    .sum::<u64>(),
            );
        }
        if result.is_err() {
            span.record("otel.status_code", "ERROR");
        }
        result
    }

    fn build_custom_battle_vehicle(
        &self,
        loadout: &CustomBattleLoadout,
    ) -> Result<Option<BattleVehicle>> {
        let Some(offer) = self
            .offers
            .get(&loadout.vehicle)
            .filter(|offer| offer.grantable())
        else {
            return Ok(None);
        };
        let definition = self.crew_vehicle(loadout.vehicle)?;
        let mut crew = CrewRoster::default();
        for (slot, roles) in definition.roles.iter().enumerate() {
            let role = *roles.first().context("Missing primary crew role")?;
            let mut member = self.new_crew_member(&definition, role, slot)?;
            member.seat = Some(CrewSeat {
                vehicle: VEHICLE,
                slot,
            });
            crew.insert(member)?;
        }
        let snapshot = AccountSnapshot {
            id: 0,
            display_name: String::new(),
            revision: 0,
            balances: Default::default(),
            crew: Some(crew),
            vehicles: vec![OwnedVehicle {
                inventory_id: VEHICLE,
                catalog_key: offer.key.clone(),
                ammunition: None,
                has_crew: true,
            }],
            depot_shells: Default::default(),
            progression: Default::default(),
            settings: Default::default(),
        };
        self.battle_vehicle_with_edits(&snapshot, VEHICLE, loadout, true)
    }

    #[tracing::instrument(name = "accounts.edit_battle_loadout", skip_all, fields(
        ddd.bounded_context.name = "accounts", user.id = (snapshot.id > 0).then_some(snapshot.id),
        input.inventory_id = inventory_id, input.vehicle_type = loadout.vehicle,
        input.modules.count = loadout.modules.len(), input.vehicle.hit_points = loadout.hit_points,
        input.ammunition.types = loadout.ammunition.as_ref().map(Vec::len),
        output.outcome = tracing::field::Empty, output.vehicle.hit_points = tracing::field::Empty,
        output.ammunition.rounds = tracing::field::Empty, otel.status_code = tracing::field::Empty,
    ), err)]
    pub(crate) fn battle_vehicle_with_edits(
        &self,
        snapshot: &AccountSnapshot,
        inventory_id: i64,
        loadout: &CustomBattleLoadout,
        custom: bool,
    ) -> Result<Option<BattleVehicle>> {
        let span = tracing::Span::current();
        let result = self.project_battle_edits(snapshot, inventory_id, loadout, custom);
        match &result {
            Ok(Some(vehicle)) => {
                span.record("output.outcome", "prepared");
                span.record("output.vehicle.hit_points", vehicle.health);
                span.record(
                    "output.ammunition.rounds",
                    vehicle
                        .ammunition
                        .iter()
                        .map(|&(_, count)| u64::from(count))
                        .sum::<u64>(),
                );
            }
            Ok(None) => {
                span.record("output.outcome", "invalid_loadout");
            }
            Err(_) => {
                span.record("output.outcome", "projection_failed");
                span.record("otel.status_code", "ERROR");
            }
        }
        result
    }

    fn project_battle_edits(
        &self,
        snapshot: &AccountSnapshot,
        inventory_id: i64,
        loadout: &CustomBattleLoadout,
        custom: bool,
    ) -> Result<Option<BattleVehicle>> {
        let Some(owned) = snapshot
            .vehicles
            .iter()
            .find(|vehicle| vehicle.inventory_id == inventory_id)
        else {
            return Ok(None);
        };
        let template = self.vehicle(&owned.catalog_key)?;
        if template.modules.rules.vehicle != loadout.vehicle
            || loadout.hit_points.is_some_and(|hp| hp < 1)
        {
            return Ok(None);
        }
        let base_modules = template.modules.rules.loadout(snapshot, inventory_id);
        let mut installed = base_modules.clone();
        installed.extend(&loadout.modules);
        if (custom || installed != base_modules)
            && template.modules.rules.validate_loadout(&installed).is_err()
        {
            return Ok(None);
        }
        let Some(rules) = template.ammunition_rules(&installed) else {
            return Ok(None);
        };
        let loaded = if let Some(shells) = &loadout.ammunition {
            if shells
                .iter()
                .any(|&(_, rounds)| u16::try_from(rounds).is_err())
                || rules.validate_layout(shells.iter().copied()).is_err()
            {
                return Ok(None);
            }
            Some(shells.clone())
        } else if custom {
            Some(default_custom_ammunition(rules).collect())
        } else if installed != base_modules {
            let current = owned.ammunition.as_ref().map(|a| &a.loaded).or_else(|| {
                template
                    .ammunition_rules(&base_modules)
                    .map(|r| &r.defaults)
            });
            Some(
                current
                    .filter(|shells| rules.validate_layout(shells.iter().copied()).is_ok())
                    .cloned()
                    .unwrap_or_else(|| default_custom_ammunition(rules).collect()),
            )
        } else {
            None
        };
        let mut draft = snapshot.clone();
        draft.progression.installed.insert(inventory_id, installed);
        if let Some(loaded) = loaded
            && let Some(vehicle) = draft
                .vehicles
                .iter_mut()
                .find(|vehicle| vehicle.inventory_id == inventory_id)
        {
            vehicle.ammunition = Some(VehicleAmmunition { loaded });
        }
        if let Some(items) = &loadout.equipment {
            let mut layout = [0; MAX_EQUIPMENT_SLOTS];
            let Some(slots) = layout.get_mut(..items.len()) else {
                return Ok(None);
            };
            slots.copy_from_slice(items);
            if items.len() > template.equipment.slots
                || self
                    .equipment
                    .validate(&template.equipment, &layout)
                    .is_err()
            {
                return Ok(None);
            }
            draft.progression.equipment.insert(inventory_id, layout);
        }
        if let Some(items) = &loadout.consumables {
            let mut supplies = draft.progression.vehicle_consumables(inventory_id);
            supplies.layout = [0; MAX_CONSUMABLE_SLOTS];
            let Some(slots) = supplies.layout.get_mut(..items.len()) else {
                return Ok(None);
            };
            slots.copy_from_slice(items);
            if items.len() > template.supply_slots.consumables
                || self
                    .consumables
                    .validate(&template.equipment, &supplies.layout)
                    .is_err()
            {
                return Ok(None);
            }
            draft
                .progression
                .set_vehicle_consumables(inventory_id, supplies);
        }
        if !self.edit_battle_crew(&mut draft, inventory_id, &template, loadout)? {
            return Ok(None);
        }
        let mut vehicle = self.battle_vehicle(&draft, inventory_id)?;
        if let (Some(vehicle), Some(hit_points)) = (&mut vehicle, loadout.hit_points) {
            vehicle.health = hit_points;
        }
        Ok(vehicle)
    }

    fn edit_battle_crew(
        &self,
        snapshot: &mut AccountSnapshot,
        inventory_id: i64,
        template: &CatalogVehicle,
        loadout: &CustomBattleLoadout,
    ) -> Result<bool> {
        let slots = template.crew_roles.len();
        if loadout.crew_perks.len() > slots || loadout.crew_perk_efficiency.len() > slots {
            return Ok(false);
        }
        if loadout.crew_perks.is_empty() && loadout.crew_perk_efficiency.is_empty() {
            return Ok(true);
        }
        let mut crew = self.crew_roster(snapshot)?;
        for (slot, id) in crew.seats(inventory_id, slots).into_iter().enumerate() {
            let perks = loadout.crew_perks.get(slot).and_then(Option::as_ref);
            let efficiency = loadout.crew_perk_efficiency.get(slot).copied().flatten();
            if perks.is_none() && efficiency.is_none() {
                continue;
            }
            let Some(member) = id.and_then(|id| crew.members.get_mut(&id)) else {
                return Ok(false);
            };
            if let Some(perks) = perks {
                if perks.len() > MAX_MAJOR_PERKS
                    || perks.iter().copied().collect::<BTreeSet<_>>().len() != perks.len()
                    || !perks.iter().all(|&perk| eligible_perk(perk, member.role))
                {
                    return Ok(false);
                }
                let previous_last = member.training.perks.last().copied();
                member.training.last_perk_level = match perks.last() {
                    None => 0,
                    Some(&last) if Some(last) == previous_last => member.training.last_perk_level,
                    Some(_) => 100,
                };
                member.training.perks.clone_from(perks);
            }
            if let Some(percent) = efficiency {
                if percent > 100 {
                    return Ok(false);
                }
                member.training.efficiency_xp = u32::from(percent) * (MAX_EFFICIENCY_XP / 100);
            }
        }
        snapshot.crew = Some(crew);
        Ok(true)
    }
}

pub(super) fn perk_names(resources: &ResourceBundle) -> BTreeMap<u8, String> {
    let translations = localization::localized_names(resources, "text/lc_messages/crew_perks.mo")
        .unwrap_or_default();
    PERK_NAMES
        .iter()
        .filter_map(|&(id, key, fallback)| {
            perk_role(id)?;
            let name = translations
                .get(&format!("{key}/name"))
                .cloned()
                .unwrap_or_else(|| fallback.to_owned());
            Some((id, name))
        })
        .collect()
}

const PERK_NAMES: &[(u8, &str, &str)] = &[
    (6, "repair", "Repairs"),
    (8, "camouflage", "Concealment"),
    (9, "brotherhood", "Brothers in Arms"),
    (16, "commander_tutor", "Mentor"),
    (17, "commander_eagleEye", "Recon"),
    (21, "commander_enemyShotPredictor", "Sound Detection"),
    (22, "commander_practical", "Practicality"),
    (23, "commander_emergency", "Emergency"),
    (24, "commander_coordination", "Coordination"),
    (25, "commander_holdLine", "Hold the Line"),
    (26, "commander_staySharp", "Stay Sharp"),
    (28, "driver_virtuoso", "Clutch Braking"),
    (29, "driver_smoothDriving", "Smooth Ride"),
    (30, "driver_badRoadsKing", "Off-Road Driving"),
    (31, "driver_rammingMaster", "Controlled Impact"),
    (33, "driver_motorExpert", "Engineer"),
    (34, "driver_reliablePlacement", "Reliable Placement"),
    (35, "driver_suspensionRepair", "Suspension Repair"),
    (36, "driver_bulletproof", "Bulletproof"),
    (38, "gunner_sniper", "Deadeye"),
    (39, "gunner_smoothTurret", "Snap Shot"),
    (40, "gunner_rancorous", "Designated Target"),
    (41, "gunner_focus", "Concentration"),
    (42, "gunner_quickAiming", "Quick Aiming"),
    (43, "gunner_armorer", "Armorer"),
    (44, "gunner_pointBlast", "Point Blank"),
    (45, "gunner_loneWolf", "Lone Wolf"),
    (46, "loader_pedant", "Safe Stowage"),
    (47, "loader_desperado", "Adrenaline Rush"),
    (48, "loader_intuition", "Intuition"),
    (49, "loader_perfectCharge", "Perfect Charge"),
    (50, "loader_ammunitionImprove", "Ammo Tuning"),
    (51, "loader_melee", "Close Combat"),
    (52, "loader_magMastery", "Magazine Mastery"),
    (63, "loader_secondChance", "Second Chance"),
    (7, "fireFighting", "Firefighting"),
    (54, "radioman_finder", "Situational Awareness"),
    (57, "radioman_interference", "Jamming"),
    (58, "radioman_signalInterception", "Signal Interception"),
    (59, "radioman_sideBySide", "Side by Side"),
    (60, "radioman_expert", "Communications Expert"),
    (61, "radioman_battleTempered", "Battle Tempered"),
    (62, "radioman_threatSearch", "Threat Search"),
];
