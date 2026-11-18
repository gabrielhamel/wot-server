use super::GarageCatalog;
use crate::accounts::{AccountSnapshot, ProgressionChange, ProgressionCommand, ProgressionError};

#[derive(Clone, serde::Serialize)]
pub struct StoreOffer {
    pub item: i32,
    pub name: &'static str,
    pub credits: i64,
}

pub struct StoreOrder {
    pub item: i32,
    pub revision: i64,
    pub credits: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreOrderError {
    #[error("The offer or account changed. Review the current price and try again.")]
    Stale,
    #[error("This offer is unavailable.")]
    Unavailable,
    #[error(transparent)]
    Purchase(#[from] ProgressionError),
}

impl GarageCatalog {
    pub fn store_offers(&self) -> &[StoreOffer] {
        &self.store_offers
    }

    #[tracing::instrument(name = "accounts.store.plan_purchase", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = account.id, input.revision = account.revision, input.previous_revision = order.revision, input.item = order.item, input.price.credits = order.credits), err)]
    pub fn plan_store_purchase(
        &self,
        account: AccountSnapshot,
        order: StoreOrder,
    ) -> Result<ProgressionChange, StoreOrderError> {
        let offer = self
            .store_offers
            .iter()
            .find(|offer| offer.item == order.item)
            .ok_or(StoreOrderError::Unavailable)?;
        if order.revision != account.revision || offer.credits != order.credits {
            return Err(StoreOrderError::Stale);
        }
        Ok(self.plan_progression(
            account,
            ProgressionCommand::Buy {
                item: offer.item,
                count: 1,
            },
        )?)
    }
}
