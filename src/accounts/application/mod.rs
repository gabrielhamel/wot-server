pub(super) mod players;
pub use players::{CreatePlayerError, PlayerAccounts, PlayerRepository};

use super::domain::{
    AccountSnapshot, AmmoError, AmmunitionRules, ShellPurchase, ShellSelection, VehicleAmmunition,
};
use std::{collections::BTreeMap, future::Future};

#[derive(Debug, thiserror::Error)]
#[error("Account {0} was not found")]
pub struct AccountNotFound(pub i64);

pub trait AccountRepository: Clone + Send + Sync + 'static {
    fn credentials(
        &self,
        username: &str,
    ) -> impl Future<Output = anyhow::Result<Option<(i64, String)>>> + Send;
    fn snapshot(&self, id: i64) -> impl Future<Output = anyhow::Result<AccountSnapshot>> + Send;
    fn save_progression(
        &self,
        change: &ProgressionChange,
    ) -> impl Future<Output = anyhow::Result<bool>> + Send;
    fn save_ammunition(
        &self,
        change: &AmmunitionChange,
    ) -> impl Future<Output = anyhow::Result<bool>> + Send;
    fn save_settings(
        &self,
        id: i64,
        settings: BTreeMap<i32, i32>,
    ) -> impl Future<Output = anyhow::Result<i64>> + Send;
}

#[derive(Clone)]
pub struct Accounts<R> {
    repository: R,
}

impl<R: AccountRepository> Accounts<R> {
    pub fn repository(&self) -> &R {
        &self.repository
    }

    pub fn new(repository: R) -> Self {
        Self { repository }
    }

    #[tracing::instrument(name = "accounts.authenticate", skip_all, fields(ddd.bounded_context.name = "accounts", input.username.bytes = username.len()), err)]
    pub async fn authenticate(
        &self,
        username: &str,
        password: String,
    ) -> anyhow::Result<Option<AccountSnapshot>> {
        use argon2::{Argon2, PasswordHash, PasswordVerifier};
        let Some((id, hash)) = self.repository.credentials(username).await? else {
            return Ok(None);
        };
        let valid = tokio::task::spawn_blocking(crate::platform::observability::in_current_span(
            move || {
                let hash = PasswordHash::new(&hash)
                    .map_err(|e| anyhow::anyhow!("Invalid stored password hash: {e}"))?;
                Ok::<_, anyhow::Error>(
                    Argon2::default()
                        .verify_password(password.as_bytes(), &hash)
                        .is_ok(),
                )
            },
        ))
        .await??;
        if !valid {
            return Ok(None);
        }
        self.repository.snapshot(id).await.map(Some)
    }

    #[tracing::instrument(name = "accounts.snapshot", skip_all, fields(ddd.bounded_context.name = "accounts", input.id = id), err)]
    pub async fn snapshot(&self, id: i64) -> anyhow::Result<AccountSnapshot> {
        self.repository.snapshot(id).await
    }

    #[tracing::instrument(name = "accounts.save_progression", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = change.snapshot.id, input.previous_revision = change.previous_revision), err)]
    pub async fn save_progression(&self, change: &ProgressionChange) -> anyhow::Result<bool> {
        self.repository.save_progression(change).await
    }

    #[tracing::instrument(name = "accounts.save_ammunition", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = change.snapshot.id, input.previous_revision = change.previous_revision), err)]
    pub async fn save_ammunition(&self, change: &AmmunitionChange) -> anyhow::Result<bool> {
        self.repository.save_ammunition(change).await
    }

    #[tracing::instrument(name = "accounts.save_settings", skip_all, fields(ddd.bounded_context.name = "accounts", input.id = id, input.settings.count = settings.len()), err)]
    pub async fn save_settings(
        &self,
        id: i64,
        settings: BTreeMap<i32, i32>,
    ) -> anyhow::Result<i64> {
        self.repository.save_settings(id, settings).await
    }
}

/// A proposed account revision. It is committed only if the source revision still matches.
pub struct AmmunitionChange {
    pub snapshot: AccountSnapshot,
    pub inventory_id: i64,
    pub previous_revision: i64,
    pub purchases: Vec<ShellPurchase>,
}

impl AmmunitionChange {
    #[tracing::instrument(name = "accounts.plan", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = snapshot.id, input.revision = snapshot.revision, input.inventory_id = inventory_id), err)]
    pub fn plan(
        mut snapshot: AccountSnapshot,
        inventory_id: i64,
        rules: &AmmunitionRules,
        requested: &[ShellSelection],
    ) -> Result<Self, AmmoError> {
        let vehicle = snapshot
            .vehicles
            .iter_mut()
            .find(|vehicle| vehicle.inventory_id == inventory_id)
            .ok_or(AmmoError::VehicleNotFound)?;
        let current = vehicle
            .ammunition
            .as_ref()
            .map_or(&rules.defaults, |ammo| &ammo.loaded);
        let (ammunition, purchases) = rules.apply(
            current,
            requested,
            &mut snapshot.depot_shells,
            &mut snapshot.balances,
        )?;
        let previous_revision = snapshot.revision;
        if &ammunition.loaded != current {
            vehicle.ammunition = Some(ammunition);
            snapshot.revision = previous_revision
                .checked_add(1)
                .ok_or(AmmoError::InvalidLayout)?;
        }
        Ok(Self {
            snapshot,
            inventory_id,
            previous_revision,
            purchases,
        })
    }

    pub fn ammunition(&self) -> Option<&VehicleAmmunition> {
        self.snapshot
            .vehicles
            .iter()
            .find(|vehicle| vehicle.inventory_id == self.inventory_id)?
            .ammunition
            .as_ref()
    }

    pub fn changed(&self) -> bool {
        self.snapshot.revision != self.previous_revision
    }
}

pub struct ProgressionChange {
    pub acquired_vehicle: Option<i64>,
    pub sold_vehicle: Option<i64>,
    pub consumable_purchases: Vec<super::ConsumablePurchase>,
    pub shell_purchases: Vec<ShellPurchase>,
    pub hired_crew_member: Option<i64>,
    pub snapshot: AccountSnapshot,
    pub previous_revision: i64,
}

#[derive(Debug, Clone)]
pub enum ProgressionCommand {
    Crew(super::domain::CrewCommand),
    BuyVehicleSlot,
    Cosmetics {
        inventory_id: i64,
        change: super::CosmeticChange,
    },
    BuyVehicle {
        vehicle: i32,
        options: super::domain::VehiclePurchaseOptions,
        consumables: Vec<super::ConsumableSelection>,
    },
    SellVehicle(VehicleSaleRequest),
    Buy {
        item: i32,
        count: u32,
    },
    Sell(Vec<DepotSaleRequest>),
    Equipment {
        inventory_id: i64,
        change: super::domain::EquipmentChange,
    },
    FillConsumables {
        inventory_id: i64,
        slots: Vec<super::ConsumableSelection>,
        shells: Vec<super::ShellSelection>,
    },
    ConsumableAutoResupply {
        inventory_id: i64,
        enabled: bool,
    },
    InstallDirective {
        inventory_id: i64,
        installed_consumables: Vec<i32>,
        item: Option<i32>,
    },
    DirectiveAutoResupply {
        inventory_id: i64,
        enabled: bool,
    },
    Research {
        vehicle: i32,
        index: usize,
    },
    Equip {
        inventory_id: i64,
        item: i32,
        gun: i32,
        buy: bool,
    },
}

pub const MAX_DEPOT_SALE_LINES: usize = 80;

#[derive(Debug, Clone)]
pub struct DepotSaleRequest {
    pub item_type: u8,
    pub item: i32,
    pub count: u32,
}

#[derive(Debug, Clone)]
pub struct VehicleSaleRequest {
    pub inventory_id: i64,
    pub dismiss_crew: bool,
    pub mounted_items: Vec<i32>,
    pub depot_items: Vec<i32>,
}
