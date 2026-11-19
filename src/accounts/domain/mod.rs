mod cosmetics;
pub use cosmetics::*;
mod consumables;
pub use consumables::*;
mod ammunition;
mod crew;
pub use crew::*;
mod depot;
mod directives;
pub use directives::*;
mod equipment;
pub use depot::*;
pub use equipment::*;
mod grants;
pub use grants::*;
mod modules;
mod premium;
pub(crate) use premium::{PREMIUM_PACKAGES, unix_now};
mod players;
pub use players::*;
mod vehicles;
pub use ammunition::*;
pub use modules::*;
pub use vehicles::*;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Balances {
    pub credits: i64,
    pub gold: i64,
    pub bonds: i64,
    #[serde(default)]
    pub equipment_components: i64,
    pub free_xp: i64,
    pub slots: i64,
    pub berths: i64,
}

impl Default for Balances {
    fn default() -> Self {
        Self {
            credits: 100_000_000,
            gold: 1_000_000,
            bonds: 0,
            equipment_components: 0,
            free_xp: 0,
            slots: 100,
            berths: 100,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedVehicle {
    pub inventory_id: i64,
    pub catalog_key: String,
    pub ammunition: Option<VehicleAmmunition>,
    pub has_crew: bool,
}

#[derive(Debug, Clone)]
pub struct AccountSnapshot {
    pub id: i64,
    pub display_name: String,
    pub revision: i64,
    pub balances: Balances,
    pub crew: Option<CrewRoster>,
    pub vehicles: Vec<OwnedVehicle>,
    pub depot_shells: ShellStock,
    pub progression: ProgressionState,
    pub settings: BTreeMap<i32, i32>,
}
