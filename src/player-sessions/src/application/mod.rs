pub(crate) mod admin_battles;
use std::collections::BTreeMap;
use wot_accounts::contract::{AccountView, GameAccounts};
pub(crate) mod grants;
pub(crate) mod store;
pub use grants::{GrantDelivery, GrantRequestError, PlayerGrants};

#[derive(Clone)]
pub struct ClientAccounts {
    accounts: GameAccounts,
}

impl ClientAccounts {
    pub fn new(accounts: GameAccounts) -> Self {
        Self { accounts }
    }

    #[tracing::instrument(name = "player_sessions.authenticate", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.username.bytes = username.len()), err)]
    pub async fn authenticate(
        &self,
        username: &str,
        password: String,
    ) -> anyhow::Result<Option<AccountView>> {
        if username.is_empty()
            || username.len() > 128
            || password.is_empty()
            || password.len() > 1024
        {
            return Ok(None);
        }
        self.accounts.authenticate(username, password).await
    }

    #[tracing::instrument(name = "player_sessions.snapshot", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.id = id), err)]
    pub async fn snapshot(&self, id: i64) -> anyhow::Result<AccountView> {
        self.accounts.snapshot(id).await
    }

    #[tracing::instrument(name = "player_sessions.change_settings", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.id = id, input.changes.count = changes.len()), err)]
    pub async fn change_settings(
        &self,
        id: i64,
        changes: BTreeMap<i32, i32>,
    ) -> anyhow::Result<AccountView> {
        self.accounts.change_settings(id, changes).await
    }
}
