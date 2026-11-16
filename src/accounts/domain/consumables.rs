use super::{
    AccountSnapshot, EquipmentVehicle, EquipmentVehicleFilter, ProgressionError, VehicleCost,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_CONSUMABLE_SLOTS: usize = 3;
pub type ConsumableLayout = [i32; MAX_CONSUMABLE_SLOTS];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VehicleConsumables {
    pub layout: ConsumableLayout,
    pub auto_resupply: bool,
}

impl Default for VehicleConsumables {
    fn default() -> Self {
        Self {
            layout: ConsumableLayout::default(),
            auto_resupply: true,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ConsumableSelection {
    pub item: i32,
    pub alternate_price: bool,
}

pub struct ConsumablePurchase {
    pub item: i32,
    pub cost: VehicleCost,
}

pub struct ConsumableDefinition {
    pub price: Option<VehicleCost>,
    pub tags: BTreeSet<String>,
    pub incompatible: BTreeSet<String>,
    pub include: Vec<EquipmentVehicleFilter>,
    pub exclude: Vec<EquipmentVehicleFilter>,
}

pub struct ConsumableRules {
    pub items: BTreeMap<i32, ConsumableDefinition>,
    pub gold_to_credits: i64,
}

impl ConsumableRules {
    pub(crate) fn buy_for_vehicle(
        &self,
        snapshot: &mut AccountSnapshot,
        inventory_id: i64,
        vehicle: &EquipmentVehicle,
        slots: usize,
        requested: &[ConsumableSelection],
    ) -> Result<(), ProgressionError> {
        if requested.len() > slots || requested.len() > MAX_CONSUMABLE_SLOTS {
            return Err(ProgressionError::InvalidConsumables);
        }
        let mut state = VehicleConsumables::default();
        for (slot, selection) in state.layout.iter_mut().zip(requested) {
            *slot = selection.item;
        }
        self.validate(vehicle, &state.layout)?;
        let cost = requested
            .iter()
            .filter(|selection| selection.item != 0)
            .try_fold(VehicleCost::default(), |cost, selection| {
                cost.checked_add(self.price(selection.item, selection.alternate_price)?)
            })?;
        cost.charge(snapshot)?;
        snapshot
            .progression
            .set_vehicle_consumables(inventory_id, state);
        Ok(())
    }

    pub fn buy(
        &self,
        snapshot: &mut AccountSnapshot,
        item: i32,
        count: u32,
    ) -> Result<(), ProgressionError> {
        if !(1..=100).contains(&count) {
            return Err(ProgressionError::InvalidRequest);
        }
        let price = self.price(item, false)?.checked_mul(count)?;
        let count = snapshot
            .progression
            .depot
            .get(&item)
            .copied()
            .unwrap_or(0)
            .checked_add(count)
            .ok_or(ProgressionError::InvalidRequest)?;
        price.charge(snapshot)?;
        snapshot.progression.depot.insert(item, count);
        Ok(())
    }

    pub fn fill(
        &self,
        snapshot: &mut AccountSnapshot,
        inventory_id: i64,
        vehicle: &EquipmentVehicle,
        slots: usize,
        requested: &[ConsumableSelection],
    ) -> Result<Vec<ConsumablePurchase>, ProgressionError> {
        if slots > MAX_CONSUMABLE_SLOTS || requested.len() != slots {
            return Err(ProgressionError::InvalidConsumables);
        }
        let mut target = ConsumableLayout::default();
        for (slot, selection) in target.iter_mut().zip(requested) {
            *slot = selection.item;
        }
        self.validate(vehicle, &target)?;

        let mut state = snapshot.progression.vehicle_consumables(inventory_id);
        if target == state.layout {
            return Ok(Vec::new());
        }
        let mut stock = snapshot.progression.depot.clone();
        for item in state
            .layout
            .into_iter()
            .filter(|&item| item != 0 && !target.contains(&item))
        {
            let count = stock.entry(item).or_default();
            *count = count
                .checked_add(1)
                .ok_or(ProgressionError::InvalidRequest)?;
        }
        let mut cost = VehicleCost::default();
        let mut purchases = Vec::new();
        for selection in requested
            .iter()
            .filter(|slot| slot.item != 0 && !state.layout.contains(&slot.item))
        {
            let item = selection.item;
            match stock.get_mut(&item) {
                Some(count) if *count > 0 => {
                    *count -= 1;
                    if *count == 0 {
                        stock.remove(&item);
                    }
                }
                _ => {
                    let price = self.price(item, selection.alternate_price)?;
                    cost = cost.checked_add(price)?;
                    purchases.push(ConsumablePurchase { item, cost: price });
                }
            }
        }

        cost.charge(snapshot)?;
        state.layout = target;
        snapshot
            .progression
            .set_vehicle_consumables(inventory_id, state);
        snapshot.progression.depot = stock;
        Ok(purchases)
    }

    fn validate(
        &self,
        vehicle: &EquipmentVehicle,
        layout: &ConsumableLayout,
    ) -> Result<(), ProgressionError> {
        let mut mounted = Vec::<(i32, &ConsumableDefinition)>::new();
        for item in layout.iter().copied().filter(|&item| item != 0) {
            let definition = self
                .items
                .get(&item)
                .ok_or(ProgressionError::InvalidConsumables)?;
            if (!definition.include.is_empty()
                && !definition
                    .include
                    .iter()
                    .any(|filter| filter.matches(vehicle)))
                || definition
                    .exclude
                    .iter()
                    .any(|filter| filter.matches(vehicle))
                || mounted.iter().any(|&(other, previous)| {
                    item == other
                        || !definition.incompatible.is_disjoint(&previous.tags)
                        || !previous.incompatible.is_disjoint(&definition.tags)
                })
            {
                return Err(ProgressionError::InvalidConsumables);
            }
            mounted.push((item, definition));
        }
        Ok(())
    }

    fn price(&self, item: i32, alternate: bool) -> Result<VehicleCost, ProgressionError> {
        let price = self
            .items
            .get(&item)
            .and_then(|item| item.price)
            .ok_or(ProgressionError::NotForSale)?;
        if alternate && price.gold > 0 {
            let credits = price
                .gold
                .checked_mul(self.gold_to_credits)
                .ok_or(ProgressionError::InvalidRequest)?;
            Ok(VehicleCost { credits, gold: 0 })
        } else {
            Ok(price)
        }
    }
}
