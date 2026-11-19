use super::{AccountSnapshot, OwnedVehicle, ProgressionError, ShellLayout, VehicleAmmunition};

const MAX_OWNED_VEHICLES: usize = 1024;

impl AccountSnapshot {
    #[tracing::instrument(name = "accounts.vehicles.buy_vehicle_slot", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = self.id, input.slots = self.balances.slots), err)]
    pub(in crate::accounts) fn buy_vehicle_slot(
        &mut self,
        cost: VehicleCost,
    ) -> Result<(), ProgressionError> {
        let slots = self
            .balances
            .slots
            .checked_add(1)
            .filter(|&slots| slots <= MAX_OWNED_VEHICLES as i64)
            .ok_or(ProgressionError::VehicleSlotLimit)?;
        cost.charge(self)?;
        self.balances.slots = slots;
        Ok(())
    }

    pub(in crate::accounts) fn next_vehicle_id(&self) -> Result<i64, ProgressionError> {
        self.vehicles
            .iter()
            .map(|vehicle| vehicle.inventory_id)
            .chain(self.progression.last_vehicle_id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .filter(|&id| id > 0 && id <= i64::from(i32::MAX))
            .ok_or(ProgressionError::InvalidRequest)
    }

    #[tracing::instrument(name = "accounts.vehicles.check_vehicle_capacity", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = self.id, input.catalog_key.bytes = catalog_key.len()), err)]
    pub(in crate::accounts) fn check_vehicle_capacity(
        &self,
        catalog_key: &str,
    ) -> Result<(), ProgressionError> {
        if self
            .vehicles
            .iter()
            .any(|vehicle| vehicle.catalog_key == catalog_key)
        {
            return Err(ProgressionError::VehicleAlreadyOwned);
        }
        if self.vehicle_slots_full() {
            return Err(ProgressionError::NoFreeSlot);
        }
        Ok(())
    }

    pub(in crate::accounts) fn vehicle_slots_full(&self) -> bool {
        self.vehicles.len() >= MAX_OWNED_VEHICLES
            || i64::try_from(self.vehicles.len()).map_or(true, |count| count >= self.balances.slots)
    }

    pub(in crate::accounts) fn add_vehicle(
        &mut self,
        inventory_id: i64,
        catalog_key: &str,
        ammunition: ShellLayout,
        has_crew: bool,
    ) {
        self.vehicles.push(OwnedVehicle {
            inventory_id,
            catalog_key: catalog_key.to_owned(),
            ammunition: Some(VehicleAmmunition { loaded: ammunition }),
            has_crew,
        });
    }

    pub(in crate::accounts) fn remove_sold_vehicle(&mut self, inventory_id: i64) {
        self.progression.last_vehicle_id = Some(
            self.progression
                .last_vehicle_id
                .unwrap_or(0)
                .max(inventory_id),
        );
        self.vehicles
            .retain(|vehicle| vehicle.inventory_id != inventory_id);
        self.progression.installed.remove(&inventory_id);
        self.progression.cosmetics.outfits.remove(&inventory_id);
        self.progression.equipment.remove(&inventory_id);
        self.progression
            .set_vehicle_consumables(inventory_id, Default::default());
        self.progression
            .set_directive_slot(inventory_id, Default::default());
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VehicleCost {
    pub credits: i64,
    pub gold: i64,
}

impl VehicleCost {
    pub fn checked_add(self, other: Self) -> Result<Self, ProgressionError> {
        if self.credits < 0 || self.gold < 0 || other.credits < 0 || other.gold < 0 {
            return Err(ProgressionError::NotForSale);
        }
        Ok(Self {
            credits: self
                .credits
                .checked_add(other.credits)
                .ok_or(ProgressionError::InvalidRequest)?,
            gold: self
                .gold
                .checked_add(other.gold)
                .ok_or(ProgressionError::InvalidRequest)?,
        })
    }

    #[tracing::instrument(name = "accounts.vehicles.checked_mul", skip_all, fields(ddd.bounded_context.name = "accounts", input.count = count), err)]
    pub(super) fn checked_mul(self, count: u32) -> Result<Self, ProgressionError> {
        let count = i64::from(count);
        Ok(Self {
            credits: self
                .credits
                .checked_mul(count)
                .ok_or(ProgressionError::InvalidRequest)?,
            gold: self
                .gold
                .checked_mul(count)
                .ok_or(ProgressionError::InvalidRequest)?,
        })
    }

    #[tracing::instrument(name = "accounts.vehicles.charge", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = snapshot.id, input.gold = self.gold, input.credits = self.credits), err)]
    pub(in crate::accounts) fn charge(
        self,
        snapshot: &mut AccountSnapshot,
    ) -> Result<(), ProgressionError> {
        if self.credits < 0 || self.gold < 0 {
            return Err(ProgressionError::NotForSale);
        }
        if snapshot.balances.credits < self.credits || snapshot.balances.gold < self.gold {
            return Err(ProgressionError::NotEnoughMoney);
        }
        snapshot.balances.credits -= self.credits;
        snapshot.balances.gold -= self.gold;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrewRecruitment {
    Free,
    Credits,
    Gold,
}

#[derive(Debug, Clone, Copy)]
pub struct VehiclePurchaseOptions {
    pub crew: Option<CrewRecruitment>,
    pub ammunition: bool,
}

pub struct VehiclePurchaseRules {
    pub catalog_key: String,
    pub price: Option<VehicleCost>,
    pub requires_research: bool,
    pub crew_size: u32,
    pub ammunition: ShellLayout,
    pub ammunition_cost: Option<VehicleCost>,
}

impl VehiclePurchaseRules {
    #[tracing::instrument(name = "accounts.vehicles.buy", skip_all, fields(ddd.bounded_context.name = "accounts", input.researched = researched), err)]
    pub fn buy(
        &self,
        snapshot: &mut AccountSnapshot,
        options: VehiclePurchaseOptions,
        researched: bool,
    ) -> Result<i64, ProgressionError> {
        if self.requires_research && !researched {
            return Err(ProgressionError::InvalidRequest);
        }
        snapshot.check_vehicle_capacity(&self.catalog_key)?;
        let mut cost = self.price.ok_or(ProgressionError::NotForSale)?;
        if let Some(crew) = options.crew {
            cost = cost.checked_add(crew.cost().checked_mul(self.crew_size)?)?;
        }
        if options.ammunition {
            cost = cost.checked_add(self.ammunition_cost.ok_or(ProgressionError::NotForSale)?)?;
        }
        let ammunition = self
            .ammunition
            .iter()
            .map(|&(id, count)| (id, if options.ammunition { count } else { 0 }))
            .collect();
        let inventory_id = snapshot.next_vehicle_id()?;
        cost.charge(snapshot)?;
        snapshot.add_vehicle(
            inventory_id,
            &self.catalog_key,
            ammunition,
            options.crew.is_some(),
        );
        Ok(inventory_id)
    }
}
