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

    pub async fn commit_progression(&self, change: &ProgressionChange) -> anyhow::Result<bool> {
        self.accounts.save_progression(change).await
    }

    pub async fn commit_ammunition(&self, change: &AmmunitionChange) -> anyhow::Result<bool> {
        self.accounts.save_ammunition(change).await
    }

    pub async fn snapshot(&self, id: i64) -> anyhow::Result<AccountSnapshot> {
        self.accounts.snapshot(id).await
    }

    pub async fn change_settings(
        &self,
        id: i64,
        changes: BTreeMap<i32, i32>,
    ) -> anyhow::Result<AccountSnapshot> {
        self.accounts.save_settings(id, changes).await?;
        self.accounts.snapshot(id).await
    }
}
