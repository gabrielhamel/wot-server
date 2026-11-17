use super::{AccountSnapshot, Balances, ProgressionError};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_EQUIPMENT_SLOTS: usize = 4;
pub type EquipmentLayout = [i32; MAX_EQUIPMENT_SLOTS];

#[derive(Debug, Clone)]
pub enum EquipmentChange {
    Apply(Vec<i32>),
    SetSlot {
        slot: usize,
        item: i32,
        paid_removal: bool,
    },
}

#[derive(Default)]
pub struct EquipmentRules {
    pub items: BTreeMap<i32, EquipmentDefinition>,
}

pub struct EquipmentDefinition {
    pub price: Option<EquipmentPrice>,
    pub removable: bool,
    pub demount_cost: EquipmentPrice,
    pub tags: BTreeSet<String>,
    pub incompatible: BTreeSet<String>,
    pub include: Vec<EquipmentVehicleFilter>,
    pub exclude: Vec<EquipmentVehicleFilter>,
}

#[derive(Clone, Copy)]
pub enum EquipmentPrice {
    Credits(i64),
    Gold(i64),
    Bonds(i64),
    Components(i64),
}

impl EquipmentPrice {
    #[tracing::instrument(name = "accounts.equipment.charge", skip_all, fields(ddd.bounded_context.name = "accounts", input.count = count), err)]
    pub fn charge(self, balances: &mut Balances, count: u32) -> Result<(), ProgressionError> {
        let (balance, unit_price) = match self {
            Self::Credits(price) => (&mut balances.credits, price),
            Self::Gold(price) => (&mut balances.gold, price),
            Self::Bonds(price) => (&mut balances.bonds, price),
            Self::Components(price) => (&mut balances.equipment_components, price),
        };
        if unit_price < 0 {
            return Err(ProgressionError::NotForSale);
        }
        let cost = unit_price
            .checked_mul(i64::from(count))
            .ok_or(ProgressionError::InvalidRequest)?;
        if *balance < cost {
            return Err(ProgressionError::NotEnoughMoney);
        }
        *balance -= cost;
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct EquipmentVehicle {
    pub slots: usize,
    pub nation: String,
    pub level: i64,
    pub tags: BTreeSet<String>,
}

#[derive(Default)]
pub struct EquipmentVehicleFilter {
    pub nations: BTreeSet<String>,
    pub min_level: Option<i64>,
    pub max_level: Option<i64>,
    pub tags: BTreeSet<String>,
    pub mandatory_tags: BTreeSet<String>,
}

impl EquipmentVehicleFilter {
    pub(super) fn matches(&self, vehicle: &EquipmentVehicle) -> bool {
        (self.nations.is_empty() || self.nations.contains(&vehicle.nation))
            && self.min_level.is_none_or(|min| vehicle.level >= min)
            && self.max_level.is_none_or(|max| vehicle.level <= max)
            && (self.tags.is_empty() || !self.tags.is_disjoint(&vehicle.tags))
            && self.mandatory_tags.is_subset(&vehicle.tags)
    }
}

impl EquipmentRules {
    #[tracing::instrument(name = "accounts.equipment.buy", skip_all, fields(ddd.bounded_context.name = "accounts", input.item = item, input.count = count), err)]
    pub fn buy(
        &self,
        snapshot: &mut AccountSnapshot,
        item: i32,
        count: u32,
    ) -> Result<(), ProgressionError> {
        if !(1..=100).contains(&count) {
            return Err(ProgressionError::InvalidEquipment);
        }
        let price = self.purchase_price(item)?;
        let stock = snapshot.progression.depot.get(&item).copied().unwrap_or(0);
        let stock = stock
            .checked_add(count)
            .ok_or(ProgressionError::InvalidEquipment)?;
        price.charge(&mut snapshot.balances, count)?;
        snapshot.progression.depot.insert(item, stock);
        Ok(())
    }

    #[tracing::instrument(name = "accounts.equipment.apply", skip_all, fields(ddd.bounded_context.name = "accounts", input.inventory_id = inventory_id), err)]
    pub fn apply(
        &self,
        snapshot: &mut AccountSnapshot,
        inventory_id: i64,
        vehicle: &EquipmentVehicle,
        change: EquipmentChange,
    ) -> Result<bool, ProgressionError> {
        let current = snapshot.progression.equipment_layout(inventory_id);
        let (target, buy_missing, paid_removal) = match change {
            EquipmentChange::Apply(items) => {
                let mut layout = EquipmentLayout::default();
                layout
                    .get_mut(..items.len())
                    .filter(|_| items.len() <= vehicle.slots)
                    .ok_or(ProgressionError::InvalidEquipment)?
                    .copy_from_slice(&items);
                (layout, true, false)
            }
            EquipmentChange::SetSlot {
                slot,
                item,
                paid_removal,
            } => {
                let mut layout = current;
                let entry = layout
                    .get_mut(slot)
                    .filter(|_| slot < vehicle.slots)
                    .ok_or(ProgressionError::InvalidEquipment)?;
                if item != 0 && *entry != 0 && *entry != item {
                    return Err(ProgressionError::DemountRequired);
                }
                *entry = item;
                (layout, false, paid_removal)
            }
        };
        self.validate(vehicle, &target)?;
        if current == target {
            return Ok(false);
        }

        let mut stock = snapshot.progression.depot.clone();
        let mut balances = snapshot.balances.clone();
        for item in current
            .into_iter()
            .filter(|item| *item != 0 && !target.contains(item))
        {
            let device = self.definition(item)?;
            if !device.removable {
                if buy_missing {
                    return Err(ProgressionError::DemountRequired);
                }
                if !paid_removal {
                    continue;
                }
                device.demount_cost.charge(&mut balances, 1)?;
            }
            let count = stock.entry(item).or_default();
            *count = count
                .checked_add(1)
                .ok_or(ProgressionError::InvalidEquipment)?;
        }
        for item in target
            .into_iter()
            .filter(|item| *item != 0 && !current.contains(item))
        {
            match stock.get_mut(&item) {
                Some(count) if *count > 0 => {
                    *count -= 1;
                    if *count == 0 {
                        stock.remove(&item);
                    }
                }
                _ if buy_missing => self.purchase_price(item)?.charge(&mut balances, 1)?,
                _ => return Err(ProgressionError::EquipmentNotInDepot),
            }
        }

        snapshot.balances = balances;
        snapshot.progression.depot = stock;
        if target == EquipmentLayout::default() {
            snapshot.progression.equipment.remove(&inventory_id);
        } else {
            snapshot.progression.equipment.insert(inventory_id, target);
        }
        Ok(true)
    }

    fn validate(
        &self,
        vehicle: &EquipmentVehicle,
        target: &EquipmentLayout,
    ) -> Result<(), ProgressionError> {
        let mut mounted = Vec::<(i32, &EquipmentDefinition)>::new();
        for (slot, item) in target
            .iter()
            .copied()
            .enumerate()
            .filter(|&(_, item)| item != 0)
        {
            if slot >= vehicle.slots {
                return Err(ProgressionError::InvalidEquipment);
            }
            let device = self.definition(item)?;
            if (!device.include.is_empty()
                && !device.include.iter().any(|rule| rule.matches(vehicle)))
                || device.exclude.iter().any(|rule| rule.matches(vehicle))
                || mounted.iter().any(|&(other, definition)| {
                    item == other
                        || !device.incompatible.is_disjoint(&definition.tags)
                        || !definition.incompatible.is_disjoint(&device.tags)
                })
            {
                return Err(ProgressionError::InvalidEquipment);
            }
            mounted.push((item, device));
        }
        Ok(())
    }

    fn definition(&self, item: i32) -> Result<&EquipmentDefinition, ProgressionError> {
        self.items
            .get(&item)
            .ok_or(ProgressionError::InvalidEquipment)
    }

    fn purchase_price(&self, item: i32) -> Result<EquipmentPrice, ProgressionError> {
        self.definition(item)?
            .price
            .ok_or(ProgressionError::NotForSale)
    }
}
