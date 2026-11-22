//! Operations, command inputs, and projections published to runtime contexts.

pub use super::application::queries::BattleVehicle;
pub use super::application::{
    AccountNotFound, CreatePlayerError, DepotSaleRequest, GameAccounts, MAX_DEPOT_SALE_LINES,
    PlayerAccess, ProgressionCommand, VehicleSaleRequest,
    garage::{CommittedGarageChange, GarageCommands, GarageWrite},
    inspection::{GarageInspection, PlayerGarage},
    queries::GarageQueries,
};
pub use super::domain::{
    AmmoError, ConsumableSelection, CosmeticChange, CrewCommand, CrewRecruitment, EquipmentChange,
    EquipmentUpgradeSource, GrantCurrency, GrantError, MAX_CONSUMABLE_SLOTS, MAX_EQUIPMENT_SLOTS,
    MAX_MAJOR_PERKS, NewPlayer, NewPlayerError, PassportChoice, PlayerAccount, PlayerGrant,
    PlayerPage, PlayerPagination, PlayerRole, ProgressionError, RegistrationError, RoleChange,
    ShellSelection, VehiclePurchaseOptions, is_admin_email, is_player_login,
    is_registration_username,
};
pub use super::infrastructure::client::{
    CosmeticGrantEntry, StoreAvailability, StoreCatalog, StoreCategory, StoreListing, StoreOffer,
    StoreOrder, StoreOrderError, StorePrice, VehicleCatalogEntry,
};

use super::AccountSnapshot;
use std::{future::Future, pin::Pin, sync::Arc};

/// Account view and encoded garage data for session admission and updates.
pub struct PreparedAccount {
    pub snapshot: AccountView,
    pub cache: Arc<Vec<u8>>,
}

/// Player state returned to other contexts. Accounts owns the underlying snapshot.
#[derive(Clone)]
pub struct AccountView(pub(super) AccountSnapshot);

impl AccountView {
    pub fn id(&self) -> i64 {
        self.0.id
    }

    pub fn display_name(&self) -> &str {
        &self.0.display_name
    }

    pub fn revision(&self) -> i64 {
        self.0.revision
    }

    pub fn vehicle_count(&self) -> usize {
        self.0.vehicles.len()
    }

    /// Client customization display preference for other vehicles in battle.
    pub fn customization_display_type(&self) -> u8 {
        const GAME_EXTENDED_2: i32 = 102;
        const ALL_STYLES: u8 = 2;
        self.0
            .settings
            .get(&GAME_EXTENDED_2)
            .map(|settings| ((settings >> 2) & 0b11) as u8)
            .filter(|&value| value <= ALL_STYLES)
            .unwrap_or(ALL_STYLES)
    }

    pub fn active_premium_until(&self) -> Option<u32> {
        self.0.active_premium_until()
    }

    pub fn expired_premium_at(&self) -> Option<u32> {
        self.0.expired_premium_at()
    }
}

pub fn cosmetic_purchase(lines: &[[i32; 2]]) -> Option<CosmeticChange> {
    if !(1..=super::MAX_COSMETIC_PURCHASE_LINES).contains(&lines.len()) {
        return None;
    }
    let items = lines
        .iter()
        .map(|&[item, count]| {
            let item = super::infrastructure::client::cosmetic_item(item)?;
            Some((item, u32::try_from(count).ok()?))
        })
        .collect::<Option<_>>()?;
    Some(CosmeticChange::buy(items))
}

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
