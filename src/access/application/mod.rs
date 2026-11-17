use crate::accounts::{
    AccountRepository, AccountSnapshot, Accounts, AmmunitionChange, ProgressionChange,
};
use std::collections::BTreeMap;
pub(crate) mod grants;
pub use grants::{GrantDelivery, GrantRequestError, PlayerGrants};

#[derive(Clone)]
pub struct AccountAccess<R> {
    accounts: Accounts<R>,
}

impl<R: AccountRepository> AccountAccess<R> {
    pub fn new(accounts: Accounts<R>) -> Self {
        Self { accounts }
    }

    #[tracing::instrument(name = "access.authenticate", skip_all, fields(ddd.bounded_context.name = "access", input.username.bytes = username.len()), err)]
    pub async fn authenticate(
        &self,
        username: &str,
        password: String,
    ) -> anyhow::Result<Option<AccountSnapshot>> {
        if username.is_empty()
            || username.len() > 128
            || password.is_empty()
            || password.len() > 1024
        {
            return Ok(None);
        }
        self.accounts.authenticate(username, password).await
    }

    #[tracing::instrument(name = "access.commit_progression", skip_all, fields(ddd.bounded_context.name = "access", user.id = change.snapshot.id, input.previous_revision = change.previous_revision), err)]
    pub async fn commit_progression(&self, change: &ProgressionChange) -> anyhow::Result<bool> {
        self.accounts.save_progression(change).await
    }

    #[tracing::instrument(name = "access.commit_ammunition", skip_all, fields(ddd.bounded_context.name = "access", user.id = change.snapshot.id, input.previous_revision = change.previous_revision), err)]
    pub async fn commit_ammunition(&self, change: &AmmunitionChange) -> anyhow::Result<bool> {
        self.accounts.save_ammunition(change).await
    }

    #[tracing::instrument(name = "access.snapshot", skip_all, fields(ddd.bounded_context.name = "access", input.id = id), err)]
    pub async fn snapshot(&self, id: i64) -> anyhow::Result<AccountSnapshot> {
        self.accounts.snapshot(id).await
    }

    #[tracing::instrument(name = "access.change_settings", skip_all, fields(ddd.bounded_context.name = "access", input.id = id, input.changes.count = changes.len()), err)]
    pub async fn change_settings(
        &self,
        id: i64,
        changes: BTreeMap<i32, i32>,
    ) -> anyhow::Result<AccountSnapshot> {
        self.accounts.save_settings(id, changes).await?;
        self.accounts.snapshot(id).await
    }
}
