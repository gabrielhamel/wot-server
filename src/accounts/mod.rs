mod application;
mod domain;
mod infrastructure;
use application::{
    AccountNotFound, AccountRepository, Accounts, AmmunitionChange, DepotSaleRequest,
    MAX_DEPOT_SALE_LINES, ProgressionChange, ProgressionCommand, VehicleSaleRequest,
};
use application::{CreatePlayerError, PlayerAccessRepository, PlayerProvision, PlayerRepository};
use contract::PreparedAccount;
use domain::*;
use infrastructure::client::{
    CosmeticGrantEntry, GarageCatalog, StoreCatalog, StoreOrder, StoreOrderError,
    VehicleCatalogEntry,
};
use infrastructure::postgres::PostgresAccounts;

use application::inspection::{GarageInspection, PlayerGarage};

pub mod contract;
pub mod postgres;

#[cfg(any(test, feature = "integration-test-support"))]
#[doc(hidden)]
pub mod test_support;
