use super::{AccountSnapshot, Balances, ProgressionError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectiveSlot {
    pub installed: Option<i32>,
    pub auto_resupply: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum DirectivePrice {
    Credits(i64),
    Bonds(i64),
}

impl DirectivePrice {
    fn charge(self, balances: &mut Balances, count: u32) -> Result<(), ProgressionError> {
        let (balance, price) = match self {
            Self::Credits(price) => (&mut balances.credits, price),
            Self::Bonds(price) => (&mut balances.bonds, price),
        };
        let cost = price
            .checked_mul(i64::from(count))
            .filter(|&cost| cost >= 0)
            .ok_or(ProgressionError::InvalidRequest)?;
        if *balance < cost {
            return Err(ProgressionError::NotEnoughMoney);
        }
        *balance -= cost;
        Ok(())
    }
}

#[derive(Default)]
pub struct DirectiveRules {
    // An entry with no price is installable from stock but unavailable for purchase.
    pub items: BTreeMap<i32, Option<DirectivePrice>>,
}

impl DirectiveRules {
    #[tracing::instrument(name = "accounts.directives.buy", skip_all, fields(ddd.bounded_context.name = "accounts", input.item = item, input.count = count), err)]
    pub fn buy(
        &self,
        snapshot: &mut AccountSnapshot,
        item: i32,
        count: u32,
    ) -> Result<(), ProgressionError> {
        if !(1..=100).contains(&count) {
            return Err(ProgressionError::InvalidRequest);
        }
        let stock = snapshot.progression.depot.get(&item).copied().unwrap_or(0);
        let stock = stock
            .checked_add(count)
            .ok_or(ProgressionError::InvalidRequest)?;
        self.price(item)?.charge(&mut snapshot.balances, count)?;
        snapshot.progression.depot.insert(item, stock);
        Ok(())
    }

    #[tracing::instrument(name = "accounts.directives.install", skip_all, fields(ddd.bounded_context.name = "accounts", input.vehicle = vehicle), err)]
    pub fn install(
        &self,
        snapshot: &mut AccountSnapshot,
        vehicle: i64,
        item: Option<i32>,
    ) -> Result<bool, ProgressionError> {
        if item.is_some_and(|id| !self.items.contains_key(&id)) {
            return Err(ProgressionError::InvalidDirective);
        }
        let mut slot = snapshot.progression.directive_slot(vehicle);
        if slot.installed == item {
            return Ok(false);
        }
        let depot = &mut snapshot.progression.depot;
        let returned = slot
            .installed
            .map(|old| {
                let count = depot.get(&old).copied().unwrap_or(0);
                Ok((
                    old,
                    count
                        .checked_add(1)
                        .ok_or(ProgressionError::InvalidRequest)?,
                ))
            })
            .transpose()?;
        if let Some(item) = item {
            match depot.get(&item).copied().unwrap_or(0) {
                0 => self.price(item)?.charge(&mut snapshot.balances, 1)?,
                1 => {
                    depot.remove(&item);
                }
                count => {
                    depot.insert(item, count - 1);
                }
            }
        }
        if let Some((old, count)) = returned {
            depot.insert(old, count);
        }
        slot.installed = item;
        snapshot.progression.set_directive_slot(vehicle, slot);
        Ok(true)
    }

    fn price(&self, item: i32) -> Result<DirectivePrice, ProgressionError> {
        self.items
            .get(&item)
            .copied()
            .flatten()
            .ok_or(ProgressionError::NotForSale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> AccountSnapshot {
        AccountSnapshot {
            id: 1,
            display_name: "Player".into(),
            revision: 1,
            balances: Balances {
                credits: 100,
                bonds: 5,
                ..Default::default()
            },
            crew: None,
            vehicles: Vec::new(),
            depot_shells: Default::default(),
            progression: Default::default(),
            settings: Default::default(),
        }
    }

    #[test]
    fn failed_replacement_preserves_stock_slot_and_money() {
        let rules = DirectiveRules {
            items: BTreeMap::from([
                (1, Some(DirectivePrice::Credits(10))),
                (2, Some(DirectivePrice::Bonds(6))),
                (3, None),
            ]),
        };
        let mut account = snapshot();
        rules.install(&mut account, 42, Some(1)).unwrap();
        let old_state = account.progression.clone();
        let old_balances = account.balances.clone();
        for (item, error) in [
            (2, ProgressionError::NotEnoughMoney),
            (3, ProgressionError::NotForSale),
        ] {
            assert_eq!(rules.install(&mut account, 42, Some(item)), Err(error));
            assert_eq!(account.progression, old_state);
            assert_eq!(account.balances, old_balances);
        }
        account.progression.depot.insert(3, 1);
        rules.install(&mut account, 42, Some(3)).unwrap();
        assert_eq!(account.progression.depot, BTreeMap::from([(1, 1)]));
        assert_eq!(account.balances, old_balances);

        account.progression.depot.insert(3, u32::MAX);
        let old_state = account.progression.clone();
        assert!(rules.install(&mut account, 42, Some(1)).is_err());
        assert_eq!(account.progression, old_state);
        assert_eq!(account.balances, old_balances);
    }
}
