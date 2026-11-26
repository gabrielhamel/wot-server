use crate::domain::{StoreOrder, resolve_quote};
use std::sync::Arc;
use tokio::sync::Semaphore;
use wot_accounts::contract::{AccountView, GameAccounts, StoreListing, garage::GarageQueries};
use wot_player_sessions::contract::{GarageStoreAccess, StoreCheckout, StoreCustomer, StoreError};

#[derive(serde::Serialize)]
pub(crate) struct StoreCatalog {
    revision: String,
    premium_expires_at: Option<u32>,
    offers: Vec<StoreListing>,
}

#[derive(Clone)]
pub struct Store {
    accounts: GameAccounts,
    catalog: GarageQueries,
    garage: GarageStoreAccess,
    catalog_slots: Arc<Semaphore>,
}

impl Store {
    pub fn new(accounts: GameAccounts, catalog: GarageQueries, garage: GarageStoreAccess) -> Self {
        Self {
            accounts,
            catalog,
            garage,
            catalog_slots: Arc::new(Semaphore::new(16)),
        }
    }

    pub(crate) async fn request(
        &self,
        customer: StoreCustomer,
        order: Option<StoreOrder>,
    ) -> Result<StoreCatalog, StoreError> {
        let permit = self
            .catalog_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| StoreError::Busy)?;
        let account = match (customer, order) {
            (customer, Some(order)) => {
                let revision = order.revision;
                let checkout = resolve_quote(self.catalog.purchasable_offers(), order)
                    .map(|offer| StoreCheckout { offer, revision });
                self.garage.checkout(customer, checkout).await?
            }
            (StoreCustomer::Garage(token), None) => self.garage.garage_account(token).await?,
            (StoreCustomer::Dashboard(id), None) => self.dashboard_account(id).await?,
        };
        tracing::Span::current().record("user.id", account.id());
        let catalog = self.catalog.clone();
        tokio::task::spawn_blocking(wot_platform::observability::in_current_span(move || {
            let _permit = permit;
            StoreCatalog {
                revision: account.revision().to_string(),
                premium_expires_at: account.active_premium_until(),
                offers: catalog.store_listings(&account),
            }
        }))
        .await
        .map_err(|error| StoreError::Internal(error.into()))
    }

    #[tracing::instrument(name = "store.dashboard_account", skip_all, fields(
        ddd.bounded_context.name = "store", user.id = id,
    ), err)]
    async fn dashboard_account(&self, id: i64) -> Result<AccountView, StoreError> {
        self.accounts.snapshot(id).await.map_err(StoreError::from)
    }
}
