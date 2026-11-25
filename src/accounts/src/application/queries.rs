use super::super::{
    CosmeticChange, CosmeticGrantEntry, GarageCatalog, GarageInspection, PlayerGarage,
    PreparedAccount, StoreCatalog, VehicleCatalogEntry, contract::AccountView,
    infrastructure::client::decode_outfits,
};
use std::sync::Arc;
use wot_platform::client_formats::pickle::Value;

/// The equipped vehicle data needed to enter a battle.
pub struct BattleVehicle {
    pub descriptor: Vec<u8>,
    /// Encoded outfit for a catalog-defined locked style.
    pub outfit: Vec<u8>,
    pub health: i16,
    pub crew_size: usize,
    pub performance: wot_shared_kernel::VehiclePerformance,
    pub ammunition: Vec<(i32, u16)>,
}

#[derive(Clone, Copy, Debug, thiserror::Error)]
pub enum BattleVehicleUnavailable {
    #[error("Crew incomplete")]
    CrewIncomplete,
    #[error("Vehicle unavailable for battle")]
    Unavailable,
}

impl BattleVehicleUnavailable {
    pub fn outcome(self) -> &'static str {
        match self {
            Self::CrewIncomplete => "crew_incomplete",
            Self::Unavailable => "vehicle_unavailable",
        }
    }
}

#[derive(serde::Serialize)]
pub struct BattleVehicleChoice {
    pub id: i64,
    pub name: String,
    pub unavailable_reason: Option<String>,
}

/// Read operations for player sessions and the administration dashboard.
#[derive(Clone)]
pub struct GarageQueries(Arc<GarageCatalog>);

impl GarageQueries {
    pub(crate) fn new(catalog: Arc<GarageCatalog>) -> Self {
        Self(catalog)
    }

    pub fn prepare(&self, account: AccountView) -> anyhow::Result<PreparedAccount> {
        self.0.prepare(account.0)
    }

    pub fn inspect(&self, account: AccountView) -> GarageInspection {
        GarageInspection::from_snapshot(account.0, &self.0)
    }

    pub fn player_garage(&self, account: AccountView) -> PlayerGarage {
        PlayerGarage::from_snapshot(account.0, &self.0)
    }

    pub fn store_catalog(&self, account: &AccountView) -> StoreCatalog {
        self.0.store_catalog(&account.0)
    }

    pub fn shop_revision(&self) -> i32 {
        self.0.shop_revision()
    }

    pub fn shop_data(&self) -> Arc<Vec<u8>> {
        self.0.shop.clone()
    }

    pub fn equipment_upgrades_enabled(&self) -> bool {
        self.0.equipment_upgrades_enabled()
    }

    pub fn garage_sync(&self, revision: i64, expires_at: Option<u32>) -> Value {
        self.0.garage_sync(revision, expires_at)
    }

    pub fn premium_sync(revision: i64, expires_at: Option<u32>) -> Value {
        GarageCatalog::premium_sync(revision, expires_at)
    }

    pub fn training_observer_descriptor(&self) -> Option<&[u8]> {
        self.0.training_observer_descriptor()
    }

    pub fn training_vehicle_descriptor(
        &self,
        account: &AccountView,
        inventory_id: i64,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        self.0.training_vehicle_descriptor(&account.0, inventory_id)
    }

    pub fn battle_vehicle(
        &self,
        account: &AccountView,
        inventory_id: i64,
    ) -> anyhow::Result<Option<BattleVehicle>> {
        self.0.battle_vehicle(&account.0, inventory_id)
    }

    pub fn custom_battle_vehicle(
        &self,
        loadout: &crate::contract::CustomBattleLoadout,
        account_id: Option<i64>,
    ) -> anyhow::Result<Option<BattleVehicle>> {
        self.0.custom_battle_vehicle(loadout, account_id)
    }

    pub fn custom_battle_loadout_options(
        &self,
        vehicle: i32,
    ) -> anyhow::Result<Option<crate::contract::CustomBattleLoadoutOptions>> {
        self.0.custom_battle_loadout_options(vehicle)
    }

    pub fn battle_vehicle_choices(&self, account: &AccountView) -> Vec<BattleVehicleChoice> {
        self.0.battle_vehicle_choices(&account.0)
    }

    pub fn admin_battle_vehicle(
        &self,
        account: &AccountView,
        inventory_id: i64,
    ) -> anyhow::Result<Result<BattleVehicle, BattleVehicleUnavailable>> {
        self.0.admin_battle_vehicle(&account.0, inventory_id)
    }

    pub fn outfit_change<'a>(
        &self,
        account: &AccountView,
        inventory_id: i64,
        seasons: &[i64],
        encoded: impl ExactSizeIterator<Item = &'a [u8]>,
    ) -> Option<CosmeticChange> {
        let emblem = self.0.default_vehicle_emblem(&account.0, inventory_id);
        decode_outfits(seasons, encoded, emblem).map(CosmeticChange::apply)
    }

    pub fn grantable_vehicles(&self) -> impl Iterator<Item = VehicleCatalogEntry<'_>> {
        self.0.grantable_vehicles()
    }

    pub fn grantable_styles_2d(&self) -> impl Iterator<Item = CosmeticGrantEntry<'_>> {
        self.0.grantable_styles_2d()
    }

    pub fn grantable_styles_3d(&self) -> impl Iterator<Item = CosmeticGrantEntry<'_>> {
        self.0.grantable_styles_3d()
    }

    pub fn grantable_crew_skins(&self) -> impl Iterator<Item = CosmeticGrantEntry<'_>> {
        self.0.grantable_crew_skins()
    }

    pub fn grantable_special_recruits(&self) -> Vec<crate::contract::SpecialRecruitGrantEntry> {
        self.0.grantable_special_recruits()
    }

    pub fn grantable_attachments(&self) -> impl Iterator<Item = CosmeticGrantEntry<'_>> {
        self.0.grantable_attachments()
    }

    pub fn grantable_stat_trackers(&self) -> impl Iterator<Item = CosmeticGrantEntry<'_>> {
        self.0.grantable_stat_trackers()
    }

    pub fn grantable_crew_books(&self) -> impl Iterator<Item = (i32, &str)> {
        self.0.grantable_crew_books()
    }

    pub fn grantable_experimental_equipment(&self) -> impl Iterator<Item = (i32, &str)> {
        self.0.grantable_experimental_equipment()
    }

    pub fn grantable_improved_equipment(&self) -> impl Iterator<Item = (i32, &str)> {
        self.0.grantable_improved_equipment()
    }

    pub fn grantable_bounty_equipment(&self) -> impl Iterator<Item = (i32, &str)> {
        self.0.grantable_bounty_equipment()
    }
}
