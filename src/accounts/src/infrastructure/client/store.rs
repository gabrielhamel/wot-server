use super::{GarageCatalog, items::IntCompactDescr};
use crate::{
    AccountSnapshot, EquipmentPrice, ProgressionChange, ProgressionCommand, ProgressionError,
    VehicleCost, VehiclePurchaseOptions,
};

mod catalog;
pub(super) use catalog::{listing, load};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(
    tag = "currency",
    content = "amount",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum StorePrice {
    Credits(i64),
    Gold(i64),
    Bonds(i64),
}

impl StorePrice {
    pub fn currency(self) -> &'static str {
        match self {
            Self::Credits(_) => "credits",
            Self::Gold(_) => "gold",
            Self::Bonds(_) => "bonds",
        }
    }

    pub fn amount(self) -> i64 {
        match self {
            Self::Credits(amount) | Self::Gold(amount) | Self::Bonds(amount) => amount,
        }
    }

    fn available(self, account: &AccountSnapshot) -> i64 {
        match self {
            Self::Credits(_) => account.balances.credits,
            Self::Gold(_) => account.balances.gold,
            Self::Bonds(_) => account.balances.bonds,
        }
    }

    fn charge(self, account: &mut AccountSnapshot) -> Result<(), ProgressionError> {
        match self {
            Self::Credits(amount) => EquipmentPrice::Credits(amount),
            Self::Gold(amount) => EquipmentPrice::Gold(amount),
            Self::Bonds(amount) => EquipmentPrice::Bonds(amount),
        }
        .charge(&mut account.balances, 1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreCategory {
    PremiumAccount,
    PremiumVehicles,
    CollectorsVehicles,
    CrewSkins,
    Styles2d,
    Styles3d,
    Equipment,
    Consumables,
    CrewDirectives,
    BondVehicles,
    ImprovedEquipment,
    EquipmentDirectives,
}

impl StoreCategory {
    fn is_style(self) -> bool {
        matches!(self, Self::Styles2d | Self::Styles3d)
    }

    fn is_vehicle(self) -> bool {
        matches!(
            self,
            Self::PremiumVehicles | Self::CollectorsVehicles | Self::BondVehicles
        )
    }
}

#[derive(Clone, serde::Serialize)]
pub struct StoreOffer {
    pub item: i32,
    pub name: String,
    pub category: StoreCategory,
    pub price: StorePrice,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nation: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<i64>,
}

#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreAvailability {
    Available,
    InsufficientFunds,
    ResearchRequired,
    InGarage,
    GarageFull,
    OwnershipLimit,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreOrderError {
    #[error("The offer or account changed. Review the current price and try again.")]
    Stale,
    #[error("This offer is unavailable.")]
    Unavailable,
    #[error("Research a Tech Tree vehicle of this tier or higher in the same nation.")]
    ResearchRequired,
    #[error(transparent)]
    Purchase(#[from] ProgressionError),
}

impl GarageCatalog {
    pub fn store_offers(&self) -> &[StoreOffer] {
        &self.store_offers
    }

    #[tracing::instrument(name = "accounts.tech_tree.economics", skip_all, fields(ddd.bounded_context.name = "accounts", output.unlocked_vehicle_count = tracing::field::Empty))]
    pub(super) fn tech_tree_economics(&self) -> wot_platform::client_formats::pickle::Value {
        use wot_platform::client_formats::pickle::Value;
        let unlocks: std::collections::BTreeSet<_> = self
            .offers
            .iter()
            .filter(|(_, vehicle)| {
                vehicle.grantable() && vehicle.price.is_some_and(|price| price.gold > 0)
            })
            .map(|(&item, _)| Value::from(item))
            .collect();
        tracing::Span::current().record("output.unlocked_vehicle_count", unlocks.len());
        wot_platform::pickle!({"unlocks": (Value::Set(unlocks))})
    }

    pub(crate) fn store_depot_count(&self, account: &AccountSnapshot, offer: &StoreOffer) -> u32 {
        if offer.category.is_style() {
            account
                .progression
                .cosmetics
                .style_in_depot(IntCompactDescr::from_int(i64::from(offer.item)).id)
        } else {
            account
                .progression
                .depot
                .get(&offer.item)
                .copied()
                .unwrap_or_default()
        }
    }

    pub fn store_availability(
        &self,
        account: &AccountSnapshot,
        tiers: &std::collections::BTreeMap<u8, i64>,
        offer: &StoreOffer,
    ) -> StoreAvailability {
        if offer.category.is_vehicle()
            && let Some(vehicle) = self.offers.get(&offer.item)
        {
            if account
                .vehicles
                .iter()
                .any(|owned| owned.catalog_key == vehicle.key)
            {
                return StoreAvailability::InGarage;
            }
            if offer.category == StoreCategory::CollectorsVehicles
                && !Self::collector_unlocked(tiers, offer.item, offer.tier)
            {
                return StoreAvailability::ResearchRequired;
            }
            if account.vehicle_slots_full() {
                return StoreAvailability::GarageFull;
            }
        }
        if offer.category.is_style() {
            let id = IntCompactDescr::from_int(i64::from(offer.item)).id;
            if self
                .cosmetics
                .store_style_limit(id)
                .is_some_and(|limit| account.progression.cosmetics.style_owned(id) >= limit)
            {
                return StoreAvailability::OwnershipLimit;
            }
        }
        if offer.category == StoreCategory::CrewSkins {
            let id = IntCompactDescr::from_int(i64::from(offer.item)).id;
            if self
                .crew_skins
                .get(&id)
                .and_then(|skin| skin.max_owned)
                .is_some_and(|limit| {
                    self.crew_skin_counts(account)
                        .get(&id)
                        .copied()
                        .unwrap_or_default()
                        >= limit
                })
            {
                return StoreAvailability::OwnershipLimit;
            }
        }
        if offer.price.available(account) < offer.price.amount() {
            return StoreAvailability::InsufficientFunds;
        }
        StoreAvailability::Available
    }

    #[tracing::instrument(name = "accounts.store.plan_purchase", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = account.id, input.revision = account.revision, input.previous_revision = expected_revision, input.item = offer.item, input.price.currency = offer.price.currency(), input.price.amount = offer.price.amount()), err)]
    pub(crate) fn fulfill_store_offer(
        &self,
        mut account: AccountSnapshot,
        expected_revision: i64,
        offer: &StoreOffer,
    ) -> Result<ProgressionChange, StoreOrderError> {
        if expected_revision != account.revision {
            return Err(StoreOrderError::Stale);
        }

        let acquired_vehicle = match offer.category {
            StoreCategory::PremiumAccount => {
                return Ok(self.plan_progression(
                    account,
                    ProgressionCommand::BuyPremium { days: -offer.item },
                )?);
            }
            category if category.is_vehicle() => {
                if category == StoreCategory::CollectorsVehicles
                    && !Self::collector_unlocked(
                        &self.max_researched_tiers(&account),
                        offer.item,
                        offer.tier,
                    )
                {
                    return Err(StoreOrderError::ResearchRequired);
                }
                let mut rules = self
                    .purchase_rules(offer.item)
                    .map_err(Self::catalog_failure)?;
                account.crew = Some(self.crew_roster(&account).map_err(Self::catalog_failure)?);
                // The Store quote covers the vehicle. Crew and ammunition are acquired separately in the garage.
                offer.price.charge(&mut account)?;
                rules.price = Some(VehicleCost::default());
                rules.requires_research = false;
                let inventory_id = rules.buy(
                    &mut account,
                    VehiclePurchaseOptions {
                        crew: None,
                        ammunition: false,
                    },
                    false,
                )?;
                account.progression.unlocks.insert(offer.item);
                Some(inventory_id)
            }
            category if category.is_style() => {
                let id = IntCompactDescr::from_int(i64::from(offer.item)).id;
                account.progression.cosmetics.add_style(
                    id,
                    1,
                    self.cosmetics.store_style_limit(id),
                )?;
                offer.price.charge(&mut account)?;
                None
            }
            StoreCategory::CrewSkins => {
                self.check_crew_skin_limit(&account, offer.item, 1)?;
                let stock = account
                    .progression
                    .depot
                    .get(&offer.item)
                    .copied()
                    .unwrap_or_default()
                    .checked_add(1)
                    .ok_or(ProgressionError::InvalidRequest)?;
                offer.price.charge(&mut account)?;
                account.progression.depot.insert(offer.item, stock);
                None
            }
            _ => {
                return Ok(self.plan_progression(
                    account,
                    ProgressionCommand::Buy {
                        item: offer.item,
                        count: 1,
                    },
                )?);
            }
        };
        let previous_revision = account.revision;
        account.revision = previous_revision
            .checked_add(1)
            .ok_or(ProgressionError::InvalidRequest)?;
        Ok(ProgressionChange {
            acquired_vehicle,
            sold_vehicle: None,
            consumable_purchases: Vec::new(),
            shell_purchases: Vec::new(),
            hired_crew_member: None,
            snapshot: account,
            previous_revision,
        })
    }
}

#[derive(serde::Serialize)]
pub struct StoreListing {
    #[serde(flatten)]
    pub offer: StoreOffer,
    pub in_depot: u32,
    pub availability: StoreAvailability,
}

impl GarageCatalog {
    #[tracing::instrument(name = "accounts.store.catalog", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = snapshot.id, input.revision = snapshot.revision, output.offers.count = self.store_offers().len()))]
    pub fn store_listings(&self, snapshot: &AccountSnapshot) -> Vec<StoreListing> {
        let tiers = self.max_researched_tiers(snapshot);
        self.store_offers()
            .iter()
            .map(|offer| StoreListing {
                in_depot: self.store_depot_count(snapshot, offer),
                availability: self.store_availability(snapshot, &tiers, offer),
                offer: offer.clone(),
            })
            .collect()
    }
}
