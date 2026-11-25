use super::{GarageCatalog, localization};
use crate::{
    AccountSnapshot, AmmunitionRules, CrewRoster, CrewSeat, MAX_CONSUMABLE_SLOTS,
    MAX_EFFICIENCY_XP, MAX_EQUIPMENT_SLOTS, MAX_MAJOR_PERKS, ModuleKind, OwnedVehicle,
    VehicleAmmunition, VehicleConsumables, contract::BattleVehicle, perk_role,
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomBattleLoadout {
    pub vehicle: i32,
    #[serde(default)]
    pub equipment: Vec<i32>,
    #[serde(default)]
    pub consumables: Vec<i32>,
    #[serde(default)]
    pub crew_perks: Vec<Vec<u8>>,
    #[serde(default)]
    pub crew_perk_efficiency: Vec<u8>,
    pub hit_points: Option<i16>,
    pub ammunition: Option<Vec<(i32, u16)>>,
}

#[derive(Serialize)]
pub struct CustomBattleLoadoutOptions {
    equipment_slots: usize,
    consumable_slots: usize,
    equipment: Vec<LoadoutItem>,
    consumables: Vec<LoadoutItem>,
    crew: Vec<CrewPosition>,
    stock_hit_points: i16,
    ammunition_capacity: u32,
    ammunition: Vec<LoadoutShell>,
}

#[derive(Serialize)]
struct LoadoutItem {
    id: i32,
    name: String,
}

#[derive(Serialize)]
struct LoadoutShell {
    id: i32,
    name: String,
    default_count: u32,
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
        output.vehicle.hit_points = tracing::field::Empty,
    ), err)]
    pub(crate) fn custom_battle_loadout_options(
        &self,
        vehicle: i32,
    ) -> Result<Option<CustomBattleLoadoutOptions>> {
        let Some(offer) = self.offers.get(&vehicle).filter(|offer| offer.grantable()) else {
            return Ok(None);
        };
        let template = self.vehicle(&offer.key)?;
        let stock = &template.modules.rules.stock;
        let stock_hit_points = *stock
            .get(&ModuleKind::Turret)
            .and_then(|turret| template.modules.training_health.get(turret))
            .context("Missing stock vehicle health")?;
        let rules = template
            .ammunition_rules(stock)
            .context("Missing stock gun ammunition rules")?;
        let (nation, _) = offer
            .key
            .split_once(':')
            .context("Missing vehicle nation")?;
        let names = localization::localized_names(
            &self.filesystem,
            &format!("text/lc_messages/{nation}_vehicles.mo"),
        )
        .unwrap_or_default();
        let ammunition: Vec<_> = default_custom_ammunition(rules)
            .enumerate()
            .map(|(index, (id, default_count))| LoadoutShell {
                id,
                default_count,
                name: template
                    .modules
                    .shell_name_keys
                    .get(&id)
                    .and_then(|key| key.split_once(':'))
                    .and_then(|(_, key)| names.get(key))
                    .cloned()
                    .unwrap_or_else(|| format!("Shell type {}", index + 1)),
            })
            .collect();
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
        span.record("output.ammunition.capacity", rules.capacity);
        span.record("output.vehicle.hit_points", stock_hit_points);
        Ok(Some(CustomBattleLoadoutOptions {
            stock_hit_points,
            ammunition_capacity: rules.capacity,
            ammunition,
            equipment_slots: template.equipment.slots,
            consumable_slots: template.supply_slots.consumables,
            equipment,
            consumables,
            crew,
        }))
    }

    #[tracing::instrument(name = "accounts.custom_battle_vehicle", skip_all, fields(
        ddd.bounded_context.name = "accounts", input.vehicle_type = loadout.vehicle, user.id = account_id,
        input.equipment.count = loadout.equipment.len(), input.consumables.count = loadout.consumables.len(),
        input.crew.count = loadout.crew_perks.len(),
        input.crew.perk_efficiency_min = loadout.crew_perk_efficiency.iter().min().copied(),
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
        let template = self.vehicle(&offer.key)?;
        let mut equipment = [0; MAX_EQUIPMENT_SLOTS];
        let mut consumables = [0; MAX_CONSUMABLE_SLOTS];
        let (Some(devices), Some(supplies)) = (
            equipment.get_mut(..loadout.equipment.len()),
            consumables.get_mut(..loadout.consumables.len()),
        ) else {
            return Ok(None);
        };
        if loadout.equipment.len() > template.equipment.slots
            || loadout.consumables.len() > template.supply_slots.consumables
            || loadout.crew_perks.len() > template.crew_roles.len()
            || loadout.crew_perk_efficiency.len() > template.crew_roles.len()
            || loadout
                .crew_perk_efficiency
                .iter()
                .any(|&percent| percent > 100)
            || loadout.hit_points.is_some_and(|hp| hp < 1)
        {
            return Ok(None);
        }
        devices.copy_from_slice(&loadout.equipment);
        supplies.copy_from_slice(&loadout.consumables);
        if self
            .equipment
            .validate(&template.equipment, &equipment)
            .is_err()
            || self
                .consumables
                .validate(&template.equipment, &consumables)
                .is_err()
        {
            return Ok(None);
        }
        let rules = template
            .ammunition_rules(&template.modules.rules.stock)
            .context("Missing stock gun ammunition rules")?;
        let loaded = if let Some(shells) = &loadout.ammunition {
            let layout = shells.iter().map(|&(id, count)| (id, u32::from(count)));
            if rules.validate_layout(layout.clone()).is_err() {
                return Ok(None);
            }
            layout.collect()
        } else {
            default_custom_ammunition(rules).collect()
        };
        let definition = self.crew_vehicle(loadout.vehicle)?;
        let mut crew = CrewRoster::default();
        for (slot, roles) in definition.roles.iter().enumerate() {
            let role = *roles.first().context("Missing primary crew role")?;
            let perks = loadout.crew_perks.get(slot).map_or(&[][..], Vec::as_slice);
            if perks.len() > MAX_MAJOR_PERKS
                || perks.iter().copied().collect::<BTreeSet<_>>().len() != perks.len()
                || !perks.iter().all(|&perk| eligible_perk(perk, role))
            {
                return Ok(None);
            }
            let mut member = self.new_crew_member(&definition, role, slot)?;
            member.seat = Some(CrewSeat {
                vehicle: VEHICLE,
                slot,
            });
            let efficiency = loadout
                .crew_perk_efficiency
                .get(slot)
                .copied()
                .unwrap_or(100);
            member.training.efficiency_xp = u32::from(efficiency) * (MAX_EFFICIENCY_XP / 100);
            member.training.perks = perks.to_vec();
            member.training.last_perk_level = if perks.is_empty() { 0 } else { 100 };
            crew.insert(member)?;
        }
        let mut snapshot = AccountSnapshot {
            id: 0,
            display_name: String::new(),
            revision: 0,
            balances: Default::default(),
            crew: Some(crew),
            vehicles: vec![OwnedVehicle {
                inventory_id: VEHICLE,
                catalog_key: offer.key.clone(),
                ammunition: Some(VehicleAmmunition { loaded }),
                has_crew: true,
            }],
            depot_shells: Default::default(),
            progression: Default::default(),
            settings: Default::default(),
        };
        snapshot.progression.equipment.insert(VEHICLE, equipment);
        snapshot.progression.set_vehicle_consumables(
            VEHICLE,
            VehicleConsumables {
                layout: consumables,
                ..Default::default()
            },
        );
        let mut vehicle = self.battle_vehicle(&snapshot, VEHICLE)?;
        if let (Some(vehicle), Some(hit_points)) = (&mut vehicle, loadout.hit_points) {
            vehicle.health = hit_points;
        }
        Ok(vehicle)
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
