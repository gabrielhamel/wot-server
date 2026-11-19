use crate::accounts::{AccountSnapshot, CosmeticKind, GarageCatalog};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize)]
struct GarageVehicle {
    inventory_id: i64,
    stat_trackers: BTreeMap<u16, u32>,
    key: String,
    name: String,
    tier: Option<i64>,
}

#[derive(Serialize)]
struct GarageSummary {
    display_name: String,
    credits: String,
    gold: String,
    bonds: String,
    free_xp: String,
    equipment_components: String,
    slots: i64,
}

impl From<&AccountSnapshot> for GarageSummary {
    fn from(snapshot: &AccountSnapshot) -> Self {
        Self {
            display_name: snapshot.display_name.clone(),
            credits: snapshot.balances.credits.to_string(),
            gold: snapshot.balances.gold.to_string(),
            bonds: snapshot.balances.bonds.to_string(),
            free_xp: snapshot.balances.free_xp.to_string(),
            equipment_components: snapshot.balances.equipment_components.to_string(),
            slots: snapshot.balances.slots,
        }
    }
}

#[derive(Serialize)]
pub struct GarageInspection {
    #[serde(flatten)]
    summary: GarageSummary,
    vehicles: Vec<GarageVehicle>,
    attachments: BTreeMap<u16, u32>,
    depot: BTreeMap<i32, u32>,
}

impl GarageInspection {
    pub fn vehicle_count(&self) -> usize {
        self.vehicles.len()
    }

    pub fn from_snapshot(snapshot: AccountSnapshot, catalog: &GarageCatalog) -> Self {
        let mut vehicles: Vec<_> = snapshot
            .vehicles
            .iter()
            .map(|vehicle| {
                let entry = catalog.vehicle_catalog_entry(&vehicle.catalog_key);
                let name = entry
                    .as_ref()
                    .map_or(vehicle.catalog_key.as_str(), |entry| entry.name)
                    .to_owned();
                let stat_trackers = catalog
                    .stat_tracker_options(&snapshot, vehicle.inventory_id)
                    .map(|(vehicle_type, eligible)| {
                        let owned = snapshot
                            .progression
                            .cosmetics
                            .stat_trackers()
                            .get(&vehicle_type);
                        eligible
                            .into_iter()
                            .map(|id| {
                                (
                                    id,
                                    owned
                                        .and_then(|items| items.get(&id))
                                        .copied()
                                        .unwrap_or_default(),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                GarageVehicle {
                    inventory_id: vehicle.inventory_id,
                    stat_trackers,
                    tier: entry.and_then(|entry| entry.tier),
                    key: vehicle.catalog_key.clone(),
                    name,
                }
            })
            .collect();
        vehicles.sort_unstable_by(|a, b| a.name.cmp(&b.name).then_with(|| a.key.cmp(&b.key)));
        Self {
            summary: GarageSummary::from(&snapshot),
            vehicles,
            depot: catalog
                .grantable_crew_books()
                .chain(catalog.grantable_bounty_equipment())
                .chain(catalog.grantable_experimental_equipment())
                .filter_map(|(id, _)| Some((id, *snapshot.progression.depot.get(&id)?)))
                .collect(),
            attachments: snapshot
                .progression
                .cosmetics
                .owned()
                .get(&CosmeticKind::Attachment)
                .cloned()
                .unwrap_or_default(),
        }
    }
}
#[derive(Serialize)]
pub struct PlayerGarage {
    #[serde(flatten)]
    summary: GarageSummary,
    vehicles: Vec<PlayerGarageVehicle>,
}

#[derive(Serialize)]
struct PlayerGarageVehicle {
    key: String,
    name: String,
    tier: Option<i64>,
}

impl PlayerGarage {
    pub fn from_snapshot(snapshot: AccountSnapshot, catalog: &GarageCatalog) -> Self {
        let summary = GarageSummary::from(&snapshot);
        let mut vehicles: Vec<_> = snapshot
            .vehicles
            .into_iter()
            .map(|vehicle| {
                let catalog_entry = catalog.vehicle_catalog_entry(&vehicle.catalog_key);
                PlayerGarageVehicle {
                    name: catalog_entry
                        .as_ref()
                        .map_or("Unknown tank", |entry| entry.name)
                        .to_owned(),
                    tier: catalog_entry.and_then(|entry| entry.tier),
                    key: vehicle.catalog_key,
                }
            })
            .collect();
        vehicles.sort_unstable_by(|a, b| a.name.cmp(&b.name).then_with(|| a.key.cmp(&b.key)));
        Self { summary, vehicles }
    }

    pub fn vehicle_count(&self) -> usize {
        self.vehicles.len()
    }
}
