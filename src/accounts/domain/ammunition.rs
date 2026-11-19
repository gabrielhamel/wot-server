use super::Balances;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type ShellId = i32;
pub type ShellStock = BTreeMap<ShellId, u32>;
pub type ShellLayout = Vec<(ShellId, u32)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellCurrency {
    Credits,
    Gold,
}

#[derive(Clone, Copy, Debug)]
pub struct ShellPrice {
    pub currency: ShellCurrency,
    pub amount: i64,
}

#[derive(Clone, Copy, Debug)]
pub struct ShellSelection {
    pub id: ShellId,
    pub count: u32,
    pub alternate_price: bool,
}

#[derive(Debug)]
pub struct ShellPurchase {
    pub id: ShellId,
    pub count: u32,
    pub cost: ShellPrice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AmmoError {
    #[error("invalidShells")]
    InvalidLayout,
    #[error("vehicleNotFound")]
    VehicleNotFound,
    #[error("notInShop")]
    NotForSale,
    #[error("notEnoughMoney")]
    NotEnoughMoney,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VehicleAmmunition {
    pub loaded: ShellLayout,
}

pub struct AmmunitionRules {
    pub capacity: u32,
    pub defaults: ShellLayout,
    pub prices: BTreeMap<ShellId, ShellPrice>,
    pub gold_to_credits: i64,
}

impl AmmunitionRules {
    #[tracing::instrument(name = "accounts.ammunition.apply", skip_all, fields(ddd.bounded_context.name = "accounts", input.requested.count = requested.len(), input.depot.count = depot.len()), err)]
    pub(in crate::accounts) fn apply(
        &self,
        current: &ShellLayout,
        requested: &[ShellSelection],
        depot: &mut ShellStock,
        balances: &mut Balances,
    ) -> Result<(VehicleAmmunition, Vec<ShellPurchase>), AmmoError> {
        let mut desired = BTreeMap::new();
        let mut remaining = self.capacity;
        for shell in requested {
            if !self.defaults.iter().any(|&(id, _)| id == shell.id)
                || desired.insert(shell.id, shell).is_some()
            {
                return Err(AmmoError::InvalidLayout);
            }
            remaining = remaining
                .checked_sub(shell.count)
                .ok_or(AmmoError::InvalidLayout)?;
        }

        let mut purchases = Vec::new();
        // Retain the submitted shell order, followed by any omitted compatible types.
        let loaded = requested
            .iter()
            .map(|shell| (shell.id, shell.count))
            .chain(
                self.defaults
                    .iter()
                    .filter(|(id, _)| !desired.contains_key(id))
                    .map(|&(id, _)| (id, 0)),
            )
            .collect();
        for &(id, _) in &self.defaults {
            let old_count = current
                .iter()
                .find(|&&(old_id, _)| old_id == id)
                .map_or(0, |&(_, count)| count);
            let selection = desired.get(&id);
            let new_count = selection.map_or(0, |shell| shell.count);
            let stock = depot.entry(id).or_default();
            if new_count <= old_count {
                *stock = stock
                    .checked_add(old_count - new_count)
                    .ok_or(AmmoError::InvalidLayout)?;
            } else {
                let extra = new_count - old_count;
                let from_depot = extra.min(*stock);
                *stock -= from_depot;
                let shortfall = extra - from_depot;
                if shortfall > 0 {
                    let price = self.prices.get(&id).ok_or(AmmoError::NotForSale)?;
                    let (currency, unit_price) = match (
                        price.currency,
                        selection.is_some_and(|shell| shell.alternate_price),
                    ) {
                        (ShellCurrency::Gold, true) => (
                            ShellCurrency::Credits,
                            price
                                .amount
                                .checked_mul(self.gold_to_credits)
                                .ok_or(AmmoError::NotForSale)?,
                        ),
                        _ => (price.currency, price.amount),
                    };
                    let amount = unit_price
                        .checked_mul(i64::from(shortfall))
                        .filter(|&amount| amount >= 0)
                        .ok_or(AmmoError::NotForSale)?;
                    let balance = match currency {
                        ShellCurrency::Credits => &mut balances.credits,
                        ShellCurrency::Gold => &mut balances.gold,
                    };
                    *balance = balance
                        .checked_sub(amount)
                        .filter(|&balance| balance >= 0)
                        .ok_or(AmmoError::NotEnoughMoney)?;
                    purchases.push(ShellPurchase {
                        id,
                        count: shortfall,
                        cost: ShellPrice { currency, amount },
                    });
                }
            }
            if *stock == 0 {
                depot.remove(&id);
            }
        }
        Ok((VehicleAmmunition { loaded }, purchases))
    }
}
