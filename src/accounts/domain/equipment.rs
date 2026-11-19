use super::{AccountSnapshot, Balances, MAX_GRANTED_BALANCE, ProgressionError};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_EQUIPMENT_SLOTS: usize = 4;
pub type EquipmentLayout = [i32; MAX_EQUIPMENT_SLOTS];

#[derive(Debug, Clone)]
pub enum EquipmentChange {
    Apply(Vec<i32>),
    Disassemble {
        item: i32,
    },
    SetSlot {
        slot: usize,
        item: i32,
        paid_removal: bool,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum BountyUpgradeSource {
    Depot { item: i32 },
    Mounted { inventory_id: i64, slot: usize },
}

#[derive(Clone, Copy)]
pub struct BountyUpgrade {
    pub target: i32,
    pub credits: i64,
}

#[derive(Default)]
pub struct EquipmentRules {
    pub items: BTreeMap<i32, EquipmentDefinition>,
}

pub struct EquipmentDefinition {
    pub price: Option<EquipmentPrice>,
    pub removable: bool,
    pub demount_cost: EquipmentPrice,
    pub disassembly_value: Option<i64>,
    pub bounty_upgrade: Option<BountyUpgrade>,
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

#[tracing::instrument(name = "accounts.equipment.credit_disassembly", skip_all, fields(ddd.bounded_context.name = "accounts", input.item = item, input.count = count, input.components.per_item = unit_value, output.components.received = tracing::field::Empty, output.components.balance = tracing::field::Empty), err)]
pub(super) fn credit_disassembly(
    balances: &mut Balances,
    item: i32,
    count: u32,
    unit_value: i64,
) -> Result<(), ProgressionError> {
    let received = unit_value
        .checked_mul(i64::from(count))
        .filter(|&value| value > 0)
        .ok_or(ProgressionError::InvalidRequest)?;
    let balance = balances
        .equipment_components
        .checked_add(received)
        .filter(|&value| value <= MAX_GRANTED_BALANCE)
        .ok_or(ProgressionError::InvalidRequest)?;
    balances.equipment_components = balance;
    let span = tracing::Span::current();
    span.record("output.components.received", received);
    span.record("output.components.balance", balance);
    Ok(())
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

    #[tracing::instrument(name = "accounts.equipment.apply", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = snapshot.id, input.inventory_id = inventory_id, input.equipment_change = ?change), err)]
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
            EquipmentChange::Disassemble { item } => {
                self.disassembly_value(item)
                    .ok_or(ProgressionError::InvalidEquipment)?;
                let mut layout = current;
                let mounted = layout
                    .iter_mut()
                    .find(|slot| **slot == item)
                    .ok_or(ProgressionError::InvalidEquipment)?;
                *mounted = 0;
                (layout, false, false)
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
            if !buy_missing
                && !paid_removal
                && let Some(value) = device.disassembly_value
            {
                credit_disassembly(&mut balances, item, 1, value)?;
                continue;
            }
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

    #[tracing::instrument(name = "accounts.equipment.upgrade_depot", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = snapshot.id, input.item = item, input.price.credits = tracing::field::Empty, output.item = tracing::field::Empty), err)]
    pub(crate) fn upgrade_depot(
        &self,
        snapshot: &mut AccountSnapshot,
        item: i32,
    ) -> Result<(), ProgressionError> {
        let upgrade = self.bounty_upgrade(item)?;
        let stock = &mut snapshot.progression.depot;
        let remaining = stock
            .get(&item)
            .and_then(|&count| count.checked_sub(1))
            .ok_or(ProgressionError::EquipmentNotInDepot)?;
        let upgraded_count = stock
            .get(&upgrade.target)
            .copied()
            .unwrap_or_default()
            .checked_add(1)
            .ok_or(ProgressionError::InvalidEquipment)?;
        EquipmentPrice::Credits(upgrade.credits).charge(&mut snapshot.balances, 1)?;
        if remaining == 0 {
            stock.remove(&item);
        } else {
            stock.insert(item, remaining);
        }
        stock.insert(upgrade.target, upgraded_count);
        tracing::Span::current().record("output.item", upgrade.target);
        Ok(())
    }

    #[tracing::instrument(name = "accounts.equipment.upgrade_mounted", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = snapshot.id, input.inventory_id = inventory_id, input.slot = slot, input.item = tracing::field::Empty, input.price.credits = tracing::field::Empty, output.item = tracing::field::Empty), err)]
    pub(crate) fn upgrade_mounted(
        &self,
        snapshot: &mut AccountSnapshot,
        inventory_id: i64,
        vehicle: &EquipmentVehicle,
        slot: usize,
    ) -> Result<(), ProgressionError> {
        let mut layout = snapshot.progression.equipment_layout(inventory_id);
        let mounted = layout
            .get_mut(slot)
            .filter(|_| slot < vehicle.slots)
            .ok_or(ProgressionError::InvalidEquipment)?;
        tracing::Span::current().record("input.item", *mounted);
        let upgrade = self.bounty_upgrade(*mounted)?;
        *mounted = upgrade.target;
        self.validate(vehicle, &layout)?;
        EquipmentPrice::Credits(upgrade.credits).charge(&mut snapshot.balances, 1)?;
        snapshot.progression.equipment.insert(inventory_id, layout);
        tracing::Span::current().record("output.item", upgrade.target);
        Ok(())
    }

    fn bounty_upgrade(&self, item: i32) -> Result<BountyUpgrade, ProgressionError> {
        let upgrade = self
            .definition(item)?
            .bounty_upgrade
            .ok_or(ProgressionError::InvalidEquipment)?;
        tracing::Span::current().record("input.price.credits", upgrade.credits);
        Ok(upgrade)
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

    pub(crate) fn disassembly_value(&self, item: i32) -> Option<i64> {
        self.items.get(&item)?.disassembly_value
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
