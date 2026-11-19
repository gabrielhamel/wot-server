//! Operations, command inputs, and projections published to runtime contexts.

pub use super::{
    AccountNotFound, AccountSnapshot, AmmoError, CommittedGarageChange, ConsumableSelection,
    CosmeticChange, CosmeticGrantEntry, CreatePlayerError, CrewCommand, CrewRecruitment,
    DepotSaleRequest, EquipmentChange, EquipmentUpgradeSource, GameAccounts, GarageCatalog,
    GarageCommands, GarageInspection, GarageWrite, GrantCurrency, GrantError, MAX_CONSUMABLE_SLOTS,
    MAX_COSMETIC_PURCHASE_LINES, MAX_DEPOT_SALE_LINES, MAX_EQUIPMENT_SLOTS, MAX_MAJOR_PERKS,
    NewPlayer, NewPlayerError, PassportChoice, PlayerAccess, PlayerAccount, PlayerGarage,
    PlayerGrant, PlayerPagination, PlayerRole, PreparedAccount, ProgressionCommand,
    ProgressionError, RegistrationError, RoleChange, ShellSelection, StoreCatalog, StoreOrder,
    StoreOrderError, StorePrice, VehicleCatalogEntry, VehiclePurchaseOptions, VehicleSaleRequest,
    cosmetic_item, decode_outfits, is_admin_email, is_player_login, is_registration_username,
};

use std::{future::Future, pin::Pin};

pub struct PlayerName {
    pub id: i64,
    pub display_name: String,
}

pub type DirectoryLookup<'a> =
    Pin<Box<dyn Future<Output = anyhow::Result<Vec<PlayerName>>> + Send + 'a>>;

/// Resolves identities for other contexts without sharing their storage.
pub trait PlayerDirectory: Send + Sync {
    fn find<'a>(&'a self, ids: &'a [i64]) -> DirectoryLookup<'a>;

    fn search<'a>(&'a self, prefix: &'a str, ids: Option<&'a [i64]>) -> DirectoryLookup<'a>;
}
