use std::sync::Arc;

use crate::accounts::{
    Accounts, AmmoError, AmmunitionChange, GarageCatalog, GrantError, PlayerGrant, PreparedAccount,
    ProgressionChange, ProgressionCommand, ProgressionError, ShellSelection, StoreOrder,
    StoreOrderError,
};
use crate::platform::{client_formats::pickle::Value, observability::in_current_span};

#[derive(Clone)]
pub struct GarageCommands {
    accounts: Accounts,
    catalog: Arc<GarageCatalog>,
}

pub struct CommittedGarageChange {
    pub previous_revision: i64,
    pub prepared: PreparedAccount,
    pub diff: Value,
}

pub enum GarageWrite<E> {
    Committed {
        change: Box<CommittedGarageChange>,
        receipt: Value,
    },
    Unchanged,
    Rejected(E),
    Stale,
}

impl GarageCommands {
    pub(in crate::accounts) fn new(accounts: Accounts, catalog: Arc<GarageCatalog>) -> Self {
        Self { accounts, catalog }
    }

    pub async fn purchase_offer(
        &self,
        id: i64,
        order: StoreOrder,
    ) -> anyhow::Result<Result<Option<CommittedGarageChange>, StoreOrderError>> {
        let previous = self.accounts.snapshot(id).await?;
        let catalog = self.catalog.clone();
        let projection = tokio::task::spawn_blocking(in_current_span(move || {
            let change = match catalog.plan_store_purchase(previous.clone(), order) {
                Ok(change) => change,
                Err(error) => return Ok(Err(error)),
            };
            let diff = catalog.progression_diff(&previous, &change)?;
            let prepared = catalog.prepare(change.snapshot.clone())?;
            Ok::<_, anyhow::Error>(Ok((change, prepared, diff)))
        }))
        .await??;
        let (change, prepared, diff) = match projection {
            Ok(projection) => projection,
            Err(error) => return Ok(Err(error)),
        };
        let committed = self.commit_progression(&change).await?;
        Ok(Ok(committed.then_some(CommittedGarageChange {
            previous_revision: change.previous_revision,
            prepared,
            diff,
        })))
    }

    pub async fn grant(
        &self,
        id: i64,
        grant: PlayerGrant,
    ) -> anyhow::Result<Result<Option<CommittedGarageChange>, GrantError>> {
        let previous = self.accounts.snapshot(id).await?;
        let catalog = self.catalog.clone();
        let projection = tokio::task::spawn_blocking(in_current_span(move || {
            let change = match catalog.plan_grant(previous.clone(), grant) {
                Ok(change) => change,
                Err(error) => return Ok(Err(error)),
            };
            let diff = catalog.grant_diff(&previous, &change, grant)?;
            let prepared = catalog.prepare(change.snapshot.clone())?;
            Ok::<_, anyhow::Error>(Ok((change, prepared, diff)))
        }))
        .await??;
        let (change, prepared, diff) = match projection {
            Ok(projection) => projection,
            Err(error) => return Ok(Err(error)),
        };
        let committed = self.commit_progression(&change).await?;
        Ok(Ok(committed.then_some(CommittedGarageChange {
            previous_revision: change.previous_revision,
            prepared,
            diff,
        })))
    }

    pub async fn change_progression(
        &self,
        id: i64,
        expected_revision: i64,
        command: ProgressionCommand,
    ) -> anyhow::Result<GarageWrite<ProgressionError>> {
        let previous = self.accounts.snapshot(id).await?;
        if previous.revision != expected_revision {
            return Ok(GarageWrite::Stale);
        }
        let catalog = self.catalog.clone();
        let projection = tokio::task::spawn_blocking(in_current_span(move || {
            let change = match catalog.plan_progression(previous.clone(), command.clone()) {
                Ok(change) => change,
                Err(error) => return Ok(Err(error)),
            };
            if change.snapshot.revision == change.previous_revision {
                return Ok(Ok(None));
            }
            let diff = catalog.progression_diff(&previous, &change)?;
            let receipt = catalog.progression_response(&previous, &change, &command);
            let prepared = catalog.prepare(change.snapshot.clone())?;
            Ok::<_, anyhow::Error>(Ok(Some((change, prepared, diff, receipt))))
        }))
        .await??;
        let (change, prepared, diff, receipt) = match projection {
            Ok(Some(values)) => values,
            Ok(None) => return Ok(GarageWrite::Unchanged),
            Err(error) => return Ok(GarageWrite::Rejected(error)),
        };
        if !self.commit_progression(&change).await? {
            return Ok(GarageWrite::Stale);
        }
        Ok(GarageWrite::Committed {
            change: Box::new(CommittedGarageChange {
                previous_revision: change.previous_revision,
                prepared,
                diff,
            }),
            receipt,
        })
    }

    pub async fn change_ammunition(
        &self,
        id: i64,
        expected_revision: i64,
        inventory_id: i64,
        shells: Vec<ShellSelection>,
    ) -> anyhow::Result<GarageWrite<AmmoError>> {
        let previous = self.accounts.snapshot(id).await?;
        if previous.revision != expected_revision {
            return Ok(GarageWrite::Stale);
        }
        let catalog = self.catalog.clone();
        let projection = tokio::task::spawn_blocking(in_current_span(move || {
            let change = match catalog.plan_ammunition(previous.clone(), inventory_id, &shells) {
                Ok(change) => change,
                Err(error) => return Ok(Err(error)),
            };
            if !change.changed() {
                return Ok(Ok(None));
            }
            let diff = catalog.ammunition_diff(&previous, &change)?;
            let prepared = catalog.prepare(change.snapshot.clone())?;
            let receipt = catalog.ammunition_response(&change);
            Ok::<_, anyhow::Error>(Ok(Some((change, prepared, diff, receipt))))
        }))
        .await??;
        let (change, prepared, diff, receipt) = match projection {
            Ok(Some(values)) => values,
            Ok(None) => return Ok(GarageWrite::Unchanged),
            Err(error) => return Ok(GarageWrite::Rejected(error)),
        };
        if !self.commit_ammunition(&change).await? {
            return Ok(GarageWrite::Stale);
        }
        Ok(GarageWrite::Committed {
            change: Box::new(CommittedGarageChange {
                previous_revision: change.previous_revision,
                prepared,
                diff,
            }),
            receipt,
        })
    }
    #[tracing::instrument(name = "accounts.garage.commit_progression", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = change.snapshot.id, input.previous_revision = change.previous_revision), err)]
    async fn commit_progression(&self, change: &ProgressionChange) -> anyhow::Result<bool> {
        self.accounts.save_progression(change).await
    }

    #[tracing::instrument(name = "accounts.garage.commit_ammunition", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = change.snapshot.id, input.previous_revision = change.previous_revision), err)]
    async fn commit_ammunition(&self, change: &AmmunitionChange) -> anyhow::Result<bool> {
        self.accounts.save_ammunition(change).await
    }
}
