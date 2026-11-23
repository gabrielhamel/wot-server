use super::{
    GarageCatalog,
    crew::now,
    items::{IntCompactDescr, item_type},
};
use crate::{
    AccountSnapshot, DepotSaleRequest, EquipmentPrice, MAX_DEPOT_SALE_LINES, ProgressionError,
    VehicleSaleRequest,
};
use std::collections::BTreeSet;

impl GarageCatalog {
    pub(super) fn sell_vehicle(
        &self,
        snapshot: &mut AccountSnapshot,
        sale: VehicleSaleRequest,
    ) -> Result<(), ProgressionError> {
        let id = sale.inventory_id;
        let owned = snapshot
            .vehicles
            .iter()
            .find(|vehicle| vehicle.inventory_id == id)
            .ok_or(ProgressionError::InvalidVehicleSale)?;
        let vehicle = self
            .vehicle(&owned.catalog_key)
            .map_err(Self::catalog_failure)?;
        if vehicle.equipment.tags.contains("cannot_be_sold") {
            return Err(ProgressionError::VehicleCannotBeSold);
        }
        let mounted_items = unique_items(&sale.mounted_items)?;
        let depot_items = unique_items(&sale.depot_items)?;
        let rules = &vehicle.modules.rules;
        let loadout = rules.loadout(snapshot, id);
        let fittings = vehicle
            .project(owned, snapshot)
            .map_err(Self::catalog_failure)?;
        let depot_sales = depot_items
            .into_iter()
            .map(|item| {
                let cd = IntCompactDescr::from_int(i64::from(item));
                let count = match cd.item_type {
                    item_type::SHELL
                        if vehicle
                            .modules
                            .ammunition
                            .values()
                            .any(|ammo| ammo.prices.contains_key(&item)) =>
                    {
                        snapshot.depot_shells.get(&item)
                    }
                    item_type::CHASSIS..=item_type::RADIO if rules.modules.contains_key(&item) => {
                        snapshot.progression.depot.get(&item)
                    }
                    _ => return Err(ProgressionError::InvalidRequest),
                }
                .copied()
                .filter(|&count| count > 0)
                .ok_or(ProgressionError::InsufficientDepotStock)?;
                self.price_depot_sale(DepotSaleRequest {
                    item_type: cd.item_type,
                    item,
                    count,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let mut credits = self.vehicle_sale_value(rules.vehicle)?;
        for (&kind, &stock) in &rules.stock {
            let installed = loadout
                .get(&kind)
                .copied()
                .ok_or(ProgressionError::InvalidRequest)?;
            if installed != stock {
                let installed_value = self.vehicle_sale_value(installed)?;
                credits = credits
                    .checked_sub(self.vehicle_sale_value(stock)?)
                    .and_then(|price| price.checked_add(installed_value))
                    .ok_or(ProgressionError::InvalidRequest)?;
            }
        }
        // Depot sales use the stock before any fittings from this vehicle are returned.
        snapshot.sell_depot_stock(depot_sales)?;
        let mut selected = mounted_items;
        for (item, count) in fittings.shells {
            if selected.remove(&item) {
                credits = add_proceeds(credits, self.vehicle_sale_value(item)?, count)?;
            } else {
                return_to_depot(&mut snapshot.depot_shells, item, count)?;
            }
        }
        for item in fittings
            .consumables
            .into_iter()
            .chain(fittings.directive)
            .chain(fittings.equipment)
            .filter(|&item| item != 0)
        {
            if selected.remove(&item) {
                credits = add_proceeds(credits, self.vehicle_sale_value(item)?, 1)?;
            } else {
                if let Some(device) = self.equipment.items.get(&item)
                    && !device.removable
                {
                    device
                        .demount_cost
                        .charge(&mut snapshot.balances, 1)
                        .map_err(|error| match (error, device.demount_cost) {
                            (ProgressionError::NotEnoughMoney, EquipmentPrice::Bonds(_)) => {
                                ProgressionError::NotEnoughSaleBonds
                            }
                            (ProgressionError::NotEnoughMoney, EquipmentPrice::Gold(_)) => {
                                ProgressionError::NotEnoughSaleGold
                            }
                            _ => error,
                        })?;
                }
                return_to_depot(&mut snapshot.progression.depot, item, 1)?;
            }
        }
        if !selected.is_empty() {
            return Err(ProgressionError::InvalidRequest);
        }
        snapshot.balances.credits = snapshot
            .balances
            .credits
            .checked_add(credits)
            .ok_or(ProgressionError::InvalidRequest)?;
        if snapshot.balances.credits < 0 {
            return Err(ProgressionError::NotEnoughSaleCredits);
        }

        let mut roster = self.crew_roster(snapshot).map_err(Self::catalog_failure)?;
        let crew: Vec<_> = roster
            .members
            .iter()
            .filter(|(_, member)| member.seat.is_some_and(|seat| seat.vehicle == id))
            .map(|(&id, _)| id)
            .collect();
        if sale.dismiss_crew {
            let at = now();
            for member in crew {
                Self::return_crew_skin(snapshot, &mut roster, member)?;
                roster.dismiss(member, at)?;
            }
        } else if !crew.is_empty() {
            for member in crew {
                roster.active_mut(member)?.seat = None;
            }
            roster.validate_berths(snapshot.balances.berths)?;
        }
        roster.previous_crews.remove(&id);
        roster.auto_return.remove(&id);
        roster.accelerated_training.remove(&id);
        snapshot.crew = Some(roster);
        snapshot.progression.unlocks.insert(rules.vehicle);
        snapshot.remove_sold_vehicle(id);
        Ok(())
    }

    fn vehicle_sale_value(&self, item: i32) -> Result<i64, ProgressionError> {
        self.sale_unit_credits(IntCompactDescr::from_int(i64::from(item)))
    }
}

fn unique_items(items: &[i32]) -> Result<BTreeSet<i32>, ProgressionError> {
    if items.len() > MAX_DEPOT_SALE_LINES {
        return Err(ProgressionError::InvalidRequest);
    }
    let unique: BTreeSet<_> = items.iter().copied().collect();
    if unique.len() != items.len() || unique.first().is_some_and(|&item| item <= 0) {
        return Err(ProgressionError::InvalidRequest);
    }
    Ok(unique)
}

fn add_proceeds(credits: i64, unit: i64, count: u32) -> Result<i64, ProgressionError> {
    unit.checked_mul(i64::from(count))
        .and_then(|proceeds| credits.checked_add(proceeds))
        .ok_or(ProgressionError::InvalidRequest)
}

fn return_to_depot(
    depot: &mut std::collections::BTreeMap<i32, u32>,
    item: i32,
    count: u32,
) -> Result<(), ProgressionError> {
    if count > 0 {
        let stock = depot.entry(item).or_default();
        *stock = stock
            .checked_add(count)
            .ok_or(ProgressionError::InvalidRequest)?;
    }
    Ok(())
}
