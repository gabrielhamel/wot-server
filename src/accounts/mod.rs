mod application;
mod domain;
mod infrastructure;
pub use application::{
    AccountNotFound, AccountRepository, Accounts, AmmunitionChange, DepotSaleRequest,
    MAX_DEPOT_SALE_LINES, ProgressionChange, ProgressionCommand, VehicleSaleRequest,
};
pub use application::{
    CreatePlayerError, PlayerAccessRepository, PlayerAccounts, PlayerProvision, PlayerRepository,
};
pub use domain::*;
pub use infrastructure::client::{
    CosmeticGrantEntry, GarageCatalog, PreparedAccount, StoreAvailability, StoreCatalog,
    StoreCategory, StoreOffer, StoreOrder, StoreOrderError, StorePrice, VehicleCatalogEntry,
    cosmetic_item, decode_outfits,
};
pub use infrastructure::players::PostgresPlayers;
pub use infrastructure::postgres::PostgresAccounts;

pub mod postgres {
    pub fn directory(pool: sqlx::PgPool) -> std::sync::Arc<dyn super::contract::PlayerDirectory> {
        std::sync::Arc::new(super::infrastructure::directory::PostgresPlayerDirectory::new(pool))
    }

    pub const MIGRATIONS: &sqlx::migrate::Migrator =
        &sqlx::migrate!("./src/accounts/infrastructure/migrations");
}

pub use application::GameAccounts;
pub use application::garage::{CommittedGarageChange, GarageWrite, GarageWrites};
pub type PlayerAccess = PlayerAccounts<PostgresPlayers>;
pub type GarageCommands = GarageWrites<PostgresAccounts>;

pub use application::inspection::{GarageInspection, PlayerGarage};

pub mod contract;
