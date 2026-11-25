use super::AccountSnapshot;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub type ModuleLoadout = BTreeMap<ModuleKind, i32>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ModuleKind {
    Chassis,
    Turret,
    Gun,
    Engine,
    FuelTank,
    Radio,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProgressionState {
    #[serde(default)]
    pub cosmetics: super::CosmeticState,
    #[serde(default)]
    pub premium_expires_at: u32,
    pub unlocks: BTreeSet<i32>,
    pub vehicle_xp: BTreeMap<i32, i64>,
    pub depot: BTreeMap<i32, u32>,
    pub installed: BTreeMap<i64, ModuleLoadout>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) last_vehicle_id: Option<i64>,
    #[serde(default)]
    pub equipment: BTreeMap<i64, super::EquipmentLayout>,
    #[serde(default)]
    directives: BTreeMap<i64, super::DirectiveSlot>,
    #[serde(default)]
    consumables: BTreeMap<i64, super::VehicleConsumables>,
}

impl ProgressionState {
    pub fn vehicle_consumables(&self, vehicle: i64) -> super::VehicleConsumables {
        self.consumables.get(&vehicle).copied().unwrap_or_default()
    }

    pub fn set_vehicle_consumables(&mut self, vehicle: i64, state: super::VehicleConsumables) {
        if state == super::VehicleConsumables::default() {
            self.consumables.remove(&vehicle);
        } else {
            self.consumables.insert(vehicle, state);
        }
    }

    pub fn directive_slot(&self, vehicle: i64) -> super::DirectiveSlot {
        self.directives.get(&vehicle).copied().unwrap_or_default()
    }

    pub fn set_directive_slot(&mut self, vehicle: i64, slot: super::DirectiveSlot) {
        if slot == super::DirectiveSlot::default() {
            self.directives.remove(&vehicle);
        } else {
            self.directives.insert(vehicle, slot);
        }
    }

    pub fn equipment_layout(&self, inventory_id: i64) -> super::EquipmentLayout {
        self.equipment
            .get(&inventory_id)
            .copied()
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub struct ResearchStep {
    pub item: i32,
    pub xp: i64,
    pub prerequisites: Vec<i32>,
}

#[derive(Debug, Clone)]
pub struct ModuleDefinition {
    pub kind: ModuleKind,
    pub price: Option<i64>,
    pub weight: f32,
    pub max_weight: Option<f32>,
}

#[derive(Debug, Clone, Default)]
pub struct ModuleRules {
    pub vehicle: i32,
    pub stock: ModuleLoadout,
    pub modules: BTreeMap<i32, ModuleDefinition>,
    pub turret_guns: BTreeMap<i32, BTreeSet<i32>>,
    pub research: Vec<ResearchStep>,
    pub hull_weight: f32,
}

impl ModuleRules {
    pub(crate) fn loadout_weight(&self, loadout: &ModuleLoadout) -> Option<f32> {
        loadout.values().try_fold(self.hull_weight, |weight, id| {
            self.modules.get(id).map(|module| weight + module.weight)
        })
    }

    pub fn loadout(&self, snapshot: &AccountSnapshot, inventory_id: i64) -> ModuleLoadout {
        snapshot
            .progression
            .installed
            .get(&inventory_id)
            .cloned()
            .unwrap_or_else(|| self.stock.clone())
    }

    pub(crate) fn validate_loadout(&self, loadout: &ModuleLoadout) -> Result<(), ProgressionError> {
        if loadout.len() != self.stock.len()
            || loadout.iter().any(|(kind, id)| {
                !self.stock.contains_key(kind)
                    || self
                        .modules
                        .get(id)
                        .is_none_or(|module| module.kind != *kind)
            })
        {
            return Err(ProgressionError::InvalidRequest);
        }
        let turret = loadout
            .get(&ModuleKind::Turret)
            .ok_or(ProgressionError::InvalidRequest)?;
        let gun = loadout
            .get(&ModuleKind::Gun)
            .ok_or(ProgressionError::InvalidRequest)?;
        if !self
            .turret_guns
            .get(turret)
            .is_some_and(|guns| guns.contains(gun))
        {
            return Err(ProgressionError::InvalidRequest);
        }
        let weight = self
            .loadout_weight(loadout)
            .ok_or(ProgressionError::InvalidRequest)?;
        let limit = loadout
            .get(&ModuleKind::Chassis)
            .and_then(|id| self.modules.get(id))
            .and_then(|m| m.max_weight);
        if !weight.is_finite() || limit.is_some_and(|limit| weight > limit) {
            return Err(ProgressionError::InvalidRequest);
        }
        Ok(())
    }

    #[tracing::instrument(name = "accounts.modules.research", skip_all, fields(ddd.bounded_context.name = "accounts", input.index = index), err)]
    pub fn research(
        &self,
        snapshot: &mut AccountSnapshot,
        index: usize,
        defaults: &BTreeSet<i32>,
    ) -> Result<(), ProgressionError> {
        let step = self
            .research
            .get(index)
            .ok_or(ProgressionError::InvalidRequest)?;
        if defaults.contains(&step.item) || snapshot.progression.unlocks.contains(&step.item) {
            return Err(ProgressionError::InvalidRequest);
        }
        if step
            .prerequisites
            .iter()
            .any(|id| !defaults.contains(id) && !snapshot.progression.unlocks.contains(id))
        {
            return Err(ProgressionError::InvalidRequest);
        }
        let xp = snapshot
            .progression
            .vehicle_xp
            .get(&self.vehicle)
            .copied()
            .unwrap_or(0);
        if step.xp < 0 || xp < 0 {
            return Err(ProgressionError::InvalidRequest);
        }
        let used = xp.min(step.xp);
        let free = step.xp - used;
        if snapshot.balances.free_xp < free {
            return Err(ProgressionError::NotEnoughMoney);
        }
        snapshot.balances.free_xp -= free;
        snapshot
            .progression
            .vehicle_xp
            .insert(self.vehicle, xp - used);
        snapshot.progression.unlocks.insert(step.item);
        Ok(())
    }

    #[tracing::instrument(name = "accounts.modules.equip", skip_all, fields(ddd.bounded_context.name = "accounts", input.inventory_id = inventory_id, input.item = item, input.gun = gun, input.buy = buy), err)]
    pub fn equip(
        &self,
        snapshot: &mut AccountSnapshot,
        inventory_id: i64,
        item: i32,
        gun: i32,
        buy: bool,
        defaults: &BTreeSet<i32>,
    ) -> Result<(), ProgressionError> {
        let module = self
            .modules
            .get(&item)
            .ok_or(ProgressionError::InvalidRequest)?;
        let mut next = self.loadout(snapshot, inventory_id);
        next.insert(module.kind, item);
        if gun != 0 {
            if module.kind != ModuleKind::Turret
                || self
                    .modules
                    .get(&gun)
                    .is_none_or(|m| m.kind != ModuleKind::Gun)
            {
                return Err(ProgressionError::InvalidRequest);
            }
            next.insert(ModuleKind::Gun, gun);
        }
        self.validate_loadout(&next)?;
        let current = self.loadout(snapshot, inventory_id);
        let mut depot = snapshot.progression.depot.clone();
        let mut cost = 0_i64;
        for (&kind, &id) in &next {
            if current.get(&kind) == Some(&id) {
                continue;
            }
            if !defaults.contains(&id) && !snapshot.progression.unlocks.contains(&id) {
                return Err(ProgressionError::InvalidRequest);
            }
            let count = depot.get(&id).copied().unwrap_or(0);
            if count > 0 {
                if count == 1 {
                    depot.remove(&id);
                } else {
                    depot.insert(id, count - 1);
                }
            } else if buy {
                let price = self
                    .modules
                    .get(&id)
                    .and_then(|m| m.price)
                    .filter(|&p| p >= 0)
                    .ok_or(ProgressionError::NotForSale)?;
                cost = cost
                    .checked_add(price)
                    .ok_or(ProgressionError::NotForSale)?;
            } else {
                return Err(ProgressionError::InvalidRequest);
            }
            if let Some(&old) = current.get(&kind) {
                let count = depot
                    .get(&old)
                    .copied()
                    .unwrap_or(0)
                    .checked_add(1)
                    .ok_or(ProgressionError::InvalidRequest)?;
                depot.insert(old, count);
            }
        }
        if snapshot.balances.credits < cost {
            return Err(ProgressionError::NotEnoughMoney);
        }
        snapshot.balances.credits -= cost;
        snapshot.progression.depot = depot;
        snapshot.progression.installed.insert(inventory_id, next);
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProgressionError {
    #[error("catalogUnavailable")]
    CatalogUnavailable,
    #[error("already_bought")]
    VehicleAlreadyOwned,
    #[error("slot_error")]
    NoFreeSlot,
    #[error("vehicleSlotLimit")]
    VehicleSlotLimit,
    #[error("not_enough_space")]
    NoFreeBerth,
    #[error("tankman_not_found")]
    CrewMemberNotFound,
    #[error("return_unavailable")]
    CrewReturnUnavailable,
    #[error("invalidModuleRequest")]
    InvalidRequest,
    #[error("vehicleNotFound")]
    VehicleNotFound,
    #[error("invalid_vehicle")]
    InvalidVehicleSale,
    #[error("vehicle_cannot_be_sold")]
    VehicleCannotBeSold,
    #[error("not_enough_gold")]
    NotEnoughSaleGold,
    #[error("not_enough_credits")]
    NotEnoughSaleCredits,
    #[error("not_enough_bonds")]
    NotEnoughSaleBonds,
    #[error("notEnoughMoney")]
    NotEnoughMoney,
    #[error("incompatibleEquipment")]
    InvalidEquipment,
    #[error("invalidCosmetic")]
    InvalidCosmetic,
    #[error("invalidConsumables")]
    InvalidConsumables,
    #[error(transparent)]
    Ammunition(super::AmmoError),
    #[error("invalidDirective")]
    InvalidDirective,
    #[error("demountRequired")]
    DemountRequired,
    #[error("equipmentNotInDepot")]
    EquipmentNotInDepot,
    #[error("notEnoughItems")]
    InsufficientDepotStock,
    #[error("notForSale")]
    NotForSale,
}
