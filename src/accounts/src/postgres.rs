//! Accounts assembly for the server composition root.

use std::sync::Arc;

use sqlx::{PgPool, migrate::Migrator};
use wot_resources::ResourceBundle;

use super::{
    Accounts, GarageCatalog, PostgresAccounts,
    contract::{
        GameAccounts, PlayerAccess, PlayerDirectory,
        garage::{GarageCommands, GarageQueries},
    },
    infrastructure::{directory::PostgresPlayerDirectory, players::PostgresPlayers},
};
use wot_platform::observability::in_current_span;

pub struct AccountsServices {
    repository: PostgresAccounts,
    catalog: Arc<GarageCatalog>,
}

impl AccountsServices {
    pub fn accounts(&self) -> GameAccounts {
        GameAccounts::new(Accounts::new(self.repository.clone()))
    }

    pub fn garage_queries(&self) -> GarageQueries {
        GarageQueries::new(self.catalog.clone())
    }

    pub fn garage_commands(&self) -> GarageCommands {
        GarageCommands::new(Accounts::new(self.repository.clone()), self.catalog.clone())
    }

    pub fn player_access(&self) -> anyhow::Result<PlayerAccess> {
        PostgresPlayers::new(&self.repository, self.catalog.clone()).map(PlayerAccess::new)
    }
}

pub async fn service(pool: PgPool, resources: ResourceBundle) -> anyhow::Result<AccountsServices> {
    let repository = PostgresAccounts::new(pool);
    let keys = repository.catalog_keys().await?;
    let catalog = tokio::task::spawn_blocking(in_current_span(move || {
        GarageCatalog::load(resources, &keys)
    }))
    .await??;
    Ok(AccountsServices {
        repository,
        catalog: Arc::new(catalog),
    })
}

pub fn directory(pool: PgPool) -> Arc<dyn PlayerDirectory> {
    Arc::new(PostgresPlayerDirectory::new(pool))
}

pub fn migrations() -> &'static Migrator {
    &sqlx::migrate!("./src/infrastructure/migrations")
}
