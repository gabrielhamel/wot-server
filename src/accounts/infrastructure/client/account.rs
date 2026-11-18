//! Garage account cache projection, adapted from WG Toolkit; see THIRD_PARTY_NOTICES.md.

use std::collections::{BTreeMap, BTreeSet};
use std::io;

use flate2::Compression;
use flate2::write::ZlibEncoder;

use crate::platform::client_formats::pickle::{self, Value};

use super::crew::{crew_descriptor, recycle_bin};
use super::items::{VehicleDescr, item_type};

/// `VEHICLE_SETTINGS_FLAG`: auto repair and auto load.
const VEHICLE_SETTINGS: i64 = 6;

/// Client cache data projected from an account snapshot.
#[derive(Debug, Clone)]
pub struct AccountData {
    pub vehicles: Vec<Vehicle>,
    pub ids: Vec<i64>,
    pub crew: crate::accounts::CrewRoster,
    pub balances: crate::accounts::Balances,
    pub depot_shells: crate::accounts::ShellStock,
    pub progression: crate::accounts::ProgressionState,
    pub default_unlocks: BTreeSet<i32>,
    pub elite_vehicles: BTreeSet<i32>,
    pub settings: std::collections::BTreeMap<i32, i32>,
}

/// Garage wire model: projected modules, ammunition and equipment.
#[derive(Debug, Clone)]
pub struct Vehicle {
    pub descr: VehicleDescr,
    pub crew_slots: usize,
    /// Its shells, by integer descriptor, and how many of each.
    pub shells: crate::accounts::ShellLayout,
    pub equipment: crate::accounts::EquipmentLayout,
    pub directive: Option<i32>,
    pub consumables: crate::accounts::ConsumableLayout,
}

impl AccountData {
    /// The data of a full sync, bringing the client to the given revision.
    pub fn data(&self, revision: i64) -> Value {
        crate::pickle!({
            "rev": revision,
            "inventory": (self.inventory()),
            "recycleBin": (recycle_bin(&self.crew)),
            "stats": (self.stats()),
            "potapovQuests": (potapov_quests()),
            "cache": (cache()),
            "intUserSettings": (Value::Dict(self.settings.iter().map(|(&key, &value)| (Value::from(key), Value::from(value))).collect())),
        })
    }

    /// The items the account has, by item type: the vehicles' tables of their state by
    /// inventory id, the tankmen's likewise, the crew skins' tables, and the other
    /// items' counts by integer descriptor.
    pub(super) fn inventory(&self) -> Value {
        let mut vehicles = BTreeMap::<&str, BTreeMap<Value, Value>>::new();
        let mut tankmen =
            BTreeMap::from([("compDescr", BTreeMap::new()), ("vehicle", BTreeMap::new())]);
        let mut seats: BTreeMap<_, _> = self
            .ids
            .iter()
            .copied()
            .zip(
                self.vehicles
                    .iter()
                    .map(|vehicle| vec![Value::None; vehicle.crew_slots]),
            )
            .collect();
        for (&id, member) in &self.crew.members {
            if member.dismissed_at.is_some() {
                continue;
            }
            tankmen
                .entry("compDescr")
                .or_default()
                .insert(Value::from(id), Value::Bytes(crew_descriptor(member)));
            if let Some(seat) = member.seat {
                tankmen
                    .entry("vehicle")
                    .or_default()
                    .insert(Value::from(id), Value::from(seat.vehicle));
                if let Some(slot) = seats
                    .get_mut(&seat.vehicle)
                    .and_then(|slots| slots.get_mut(seat.slot))
                {
                    *slot = Value::from(id);
                }
            }
        }
        // The client reads these tables of every vehicle, which have no entry when empty.
        for table in [
            "compDescr",
            "crew",
            "lastCrew",
            "shells",
            "shellsLayout",
            "eqs",
            "eqsLayout",
            "devicesLayout",
            "boosters",
            "boostersLayout",
            "settings",
            "extraSettings",
            "customRoleSlots",
            "customizationExpiryTime",
            "enhancements",
            "igrCustomizationLayout",
            "layoutIndexes",
            "postProgression",
            "rent",
            "repair",
        ] {
            vehicles.insert(table, BTreeMap::new());
        }

        for (&id, vehicle) in self.ids.iter().zip(&self.vehicles) {
            let shells = vehicle.shells_value();
            let crew_ids = Value::List(seats.remove(&id).unwrap_or_default());
            let previous_crew = self
                .crew
                .previous_crews
                .get(&id)
                .map_or(Value::None, |crew| {
                    Value::List(
                        crew.iter()
                            .map(|id| id.map_or(Value::None, Value::from))
                            .collect(),
                    )
                });
            for (table, value) in [
                ("compDescr", Value::Bytes(vehicle.descr.encode())),
                ("crew", crew_ids),
                ("lastCrew", previous_crew),
                ("shells", shells),
                ("shellsLayout", vehicle.shells_layout_value()),
                ("eqs", vehicle.consumables_value()),
                ("eqsLayout", vehicle.consumables_layout_value()),
                ("devicesLayout", vehicle.equipment_layout_value()),
                ("boosters", vehicle.directives_value()),
                ("boostersLayout", vehicle.directives_layout_value()),
                (
                    "settings",
                    Value::from(vehicle_settings(&self.crew, &self.progression, id)),
                ),
                ("extraSettings", Value::from(0)),
            ] {
                vehicles
                    .entry(table)
                    .or_default()
                    .insert(Value::from(id), value);
            }
        }

        let tables = |tables: BTreeMap<&str, BTreeMap<Value, Value>>| {
            Value::dict(
                tables
                    .into_iter()
                    .map(|(name, table)| (name, Value::Dict(table))),
            )
        };
        let mut inventory = BTreeMap::from([
            (Value::from(item_type::VEHICLE as i64), tables(vehicles)),
            (Value::from(item_type::TANKMAN as i64), tables(tankmen)),
            // `SkinInvData`: the skins by id, and those worn by tankman, which the
            // client reads without a default.
            (
                Value::from(item_type::CREW_SKIN as i64),
                super::crew_skins::inventory(&self.crew, &self.progression.depot),
            ),
            (
                Value::from(i32::from(item_type::SHELL)),
                Value::Dict(
                    self.depot_shells
                        .iter()
                        .map(|(&id, &count)| (Value::from(id), Value::from(count)))
                        .collect(),
                ),
            ),
            (
                Value::from(i32::from(item_type::CUSTOMIZATION)),
                super::cosmetic_inventory::CosmeticInventory::project(
                    &self.progression.cosmetics,
                    &self
                        .ids
                        .iter()
                        .copied()
                        .zip(
                            self.vehicles
                                .iter()
                                .map(|v| v.descr.int_descr().to_int() as i32),
                        )
                        .collect(),
                )
                .full(),
            ),
            (Value::from("enhancements"), crate::pickle!({})),
            (Value::from("telecomOrders"), crate::pickle!({})),
        ]);
        for (&id, &count) in &self.progression.depot {
            let kind = super::items::IntCompactDescr::from_int(i64::from(id)).item_type;
            if kind == item_type::CREW_SKIN {
                continue;
            }
            let table = inventory
                .entry(Value::from(i32::from(kind)))
                .or_insert_with(|| crate::pickle!({}));
            if let Value::Dict(table) = table {
                table.insert(Value::from(id), Value::from(count));
            }
        }
        for item_type in item_type::CHASSIS..=item_type::CREW_BOOK {
            inventory
                .entry(Value::from(item_type as i64))
                .or_insert_with(|| crate::pickle!({}));
        }
        Value::Dict(inventory)
    }

    /// The account's money, its research (the vehicles and their modules) and its
    /// limits, a new account's.
    fn stats(&self) -> Value {
        let unlocks = self
            .default_unlocks
            .iter()
            .chain(&self.progression.unlocks)
            .copied()
            .map(Value::from);
        let set = |items: BTreeSet<Value>| Value::Set(items);
        crate::pickle!({
            "credits": (self.balances.credits),
            "gold": (self.balances.gold),
            "crystal": (self.balances.bonds),
            "freeXP": (self.balances.free_xp),
            "equipCoin": (self.balances.equipment_components),
            "eventCoin": 0,
            "bpcoin": 0,
            "slots": (self.balances.slots),
            "berths": (self.balances.berths),
            "unlocks": (set(unlocks.collect())),
            "eliteVehicles": (set(self.elite_vehicles.iter().copied().map(Value::from).collect())),
            "multipliedXPVehs": (set(BTreeSet::new())),
            "multipliedRankedBattlesVehs": (set(BTreeSet::new())),
            "vehTypeXP": (Value::Dict(self.progression.vehicle_xp.iter().map(|(&id,&xp)|(Value::from(id),Value::from(xp))).collect())),
            "vehTypeLocks": {},
            "globalVehicleLocks": {},
            "maxResearchedLevelByNation": {},
            "prestigeMilestonesAchieved": {},
            "weeklyVehicleCrystals": {},
            "restrictions": {},
            "dossier": (Value::Bytes(Vec::new())),
            "clanInfo": None,
            "language": "en",
            "XPpp": 0.0,
            "accOnline": 0,
            "accOffline": 0,
            "applyAdditionalXPCount": 0,
            "applyAdditionalWoTPlusXPCount": 0,
            "dailyPlayHours": [0],
            "playLimits": ((172800, ""), (1209600, "")),
            "dummySessionStats": {
                "base": {"credits": 0, "xp": 0},
                "premium": {"credits": 0, "xp": 0},
                "totalDailyAppliedAdditionalXP": 0,
            },
            "denunciationsLeft": 10,
            "finPswdAttemptsLeft": 5,
            "hasFinPassword": false,
            "freeTMenLeft": (self.crew.remaining_member_capacity()),
            "freeVehiclesLeft": 0,
            "vehicleSellsLeft": 5,
            "fortResource": 0,
            "tkillIsSuspected": false,
        })
    }
}

pub(super) fn vehicle_settings(
    crew: &crate::accounts::CrewRoster,
    progression: &crate::accounts::ProgressionState,
    id: i64,
) -> i64 {
    const ACCELERATED_TRAINING: i64 = 1;
    const AUTO_EQUIP: i64 = 8;
    const AUTO_EQUIP_BOOSTER: i64 = 128;
    const AUTO_RETURN: i64 = 512;
    let mut flags = VEHICLE_SETTINGS;
    if crew.accelerated_training.contains(&id) {
        flags |= ACCELERATED_TRAINING;
    }
    if crew.auto_return.get(&id) == Some(&true) {
        flags |= AUTO_RETURN;
    }
    if progression.vehicle_consumables(id).auto_resupply {
        flags |= AUTO_EQUIP;
    }
    if progression.directive_slot(id).auto_resupply {
        flags |= AUTO_EQUIP_BOOSTER;
    }
    flags
}

impl Vehicle {
    pub(super) fn consumables_value(&self) -> Value {
        let len = self
            .consumables
            .iter()
            .rposition(|&item| item != 0)
            .map_or(0, |slot| slot + 1);
        Value::List(
            self.consumables
                .iter()
                .take(len)
                .copied()
                .map(Value::from)
                .collect(),
        )
    }

    pub(super) fn consumables_layout_value(&self) -> Value {
        Value::List(vec![self.consumables_value()])
    }

    pub(super) fn directives_value(&self) -> Value {
        Value::List(self.directive.into_iter().map(Value::from).collect())
    }

    pub(super) fn directives_layout_value(&self) -> Value {
        Value::List(vec![self.directives_value()])
    }

    pub(super) fn equipment_layout_value(&self) -> Value {
        let count = self
            .equipment
            .iter()
            .rposition(|&item| item != 0)
            .map_or(0, |slot| slot + 1);
        Value::List(vec![Value::List(
            self.equipment
                .iter()
                .take(count)
                .copied()
                .map(Value::from)
                .collect(),
        )])
    }

    /// Its shells as the account lists them, `[shell, count, ...]`.
    pub(super) fn shells_value(&self) -> Value {
        Value::List(
            self.shells
                .iter()
                .flat_map(|&(cd, count)| [Value::from(cd), Value::from(count)])
                .collect(),
        )
    }
    pub(super) fn shells_layout_value(&self) -> Value {
        let shells = self.shells_value();
        let key = crate::pickle!((
            (self.descr.turret_int_descr().to_int()),
            (self.descr.gun_int_descr().to_int())
        ));
        Value::Dict(BTreeMap::from([(key, Value::List(vec![shells]))]))
    }
}

/// The personal missions of an account that has none.
fn potapov_quests() -> Value {
    let campaign = || {
        crate::pickle!({
            "lastIDs": {},
            "rewards": ({}, {}),
            "selected": (Value::Set(BTreeSet::new())),
            "slots": 3,
        })
    };
    crate::pickle!({
        "activeCampaigns": [],
        // The progress of no quest.
        "compDescr": (Value::Bytes(Vec::new())),
        "lastActiveOperationID": None,
        "regular": (campaign()),
        "pm2": (campaign()),
        "pm3": (campaign()),
        "pm4": (campaign()),
        "tiles": (Value::Set(BTreeSet::new())),
    })
}

/// What the client caches of the account's state on the server, notably that it may
/// spend its money.
fn cache() -> Value {
    crate::pickle!({
        "isResourcesConsumptionAllowed": true,
        "isEmergencyModeEnabled": false,
        "isFinPswdVerified": false,
        "isSsrPlayEnabled": false,
        "SPA": {},
        "activeOrders": {},
        "clanFortState": 0,
        "clanMembers": {},
        "comp7": {"isOnline": false, "qualification": {"battles": [], "state": "in_progress"}},
        "dynamicCurrencies": {},
        "entitlements": {},
        "gameRestrictions": {"payment": {}, "privateChat": {}, "publicChat": {}, "session": {}, "settings": {}},
        "giftsData": {"isReady": false},
        "potapovQuestIDs": {},
        "vehsLock": {},
    })
}

/// A value as streamed: pickled, then zlib-compressed.
pub fn compress(value: &Value) -> io::Result<Vec<u8>> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    pickle::to_writer(&mut encoder, value)?;
    encoder.finish()
}

pub(super) fn shell_purchases_value(purchases: &[crate::accounts::ShellPurchase]) -> Value {
    Value::List(
        purchases
            .iter()
            .map(|purchase| {
                let amount = purchase.cost.amount;
                let (credits, gold) = match purchase.cost.currency {
                    crate::accounts::ShellCurrency::Credits => (amount, 0),
                    crate::accounts::ShellCurrency::Gold => (0, amount),
                };
                crate::pickle!(((purchase.id), (credits, gold), (purchase.count)))
            })
            .collect(),
    )
}
