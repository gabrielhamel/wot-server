#![forbid(unsafe_code)]

//! Player identity, inventory and garage progression.
//!
//! ```
//! use wot_accounts::contract::AccountView;
//! fn revision(account: &AccountView) -> i64 { account.revision() }
//! ```
//!
//! ```compile_fail,E0616
//! use wot_accounts::contract::AccountView;
//! fn stored_account(account: &AccountView) { let _ = &account.0; }
//! ```
//!
//! ```compile_fail
//! use wot_accounts::GarageCatalog;
//! ```

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
    CosmeticGrantEntry, GarageCatalog, StoreListing, StoreOffer, StoreOrderError,
    VehicleCatalogEntry,
};
use infrastructure::postgres::PostgresAccounts;

use application::inspection::{GarageInspection, PlayerGarage};

pub mod contract;
pub mod postgres;

#[cfg(any(test, feature = "integration-test-support"))]
#[doc(hidden)]
pub mod test_support;
