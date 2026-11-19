//! Accounts fixtures for verification of storage and garage decisions.

use std::sync::Arc;

use super::{
    contract::{
        AccountView, GameAccounts, GarageCommands, GarageQueries, PlayerAccess, PlayerDirectory,
    },
    infrastructure::directory::PostgresPlayerDirectory,
};

pub use super::application::{
    AccountNotFound, AccountRepository, Accounts, AmmunitionChange, DepotSaleRequest,
    MAX_DEPOT_SALE_LINES, PlayerProvision, ProgressionChange, ProgressionCommand,
    VehicleSaleRequest,
};
pub use super::domain::*;
pub use super::infrastructure::{
    client::{
        GarageCatalog, StoreCategory, StoreOffer, StoreOrder, StoreOrderError, StorePrice,
        decode_outfits,
    },
    players::PostgresPlayers,
    postgres::PostgresAccounts,
};

pub fn account_snapshot(account: &AccountView) -> &AccountSnapshot {
    &account.0
}

pub fn currency_balance(currency: GrantCurrency, balances: &Balances) -> i64 {
    currency.balance(balances)
}

pub fn garage_queries(catalog: Arc<GarageCatalog>) -> GarageQueries {
    GarageQueries::new(catalog)
}

pub fn garage_commands(accounts: Accounts, catalog: Arc<GarageCatalog>) -> GarageCommands {
    GarageCommands::new(accounts, catalog)
}

pub fn game_accounts(accounts: Accounts) -> GameAccounts {
    GameAccounts::new(accounts)
}

pub fn player_access(repository: PostgresPlayers) -> PlayerAccess {
    PlayerAccess::new(repository)
}

pub fn directory(pool: sqlx::PgPool) -> Arc<dyn PlayerDirectory> {
    Arc::new(PostgresPlayerDirectory::new(pool))
}
