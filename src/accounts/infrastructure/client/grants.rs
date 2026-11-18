use super::GarageCatalog;
use crate::{
    accounts::{AccountSnapshot, GrantError, PlayerGrant, ProgressionChange, ProgressionError},
    platform::client_formats::pickle::Value,
};

pub struct VehicleCatalogEntry<'a> {
    pub id: i32,
    pub key: &'a str,
    pub name: &'a str,
    pub tier: Option<i64>,
}

impl GarageCatalog {
    pub fn grantable_vehicles(&self) -> impl Iterator<Item = VehicleCatalogEntry<'_>> {
        self.offers
            .iter()
            .filter(|(_, offer)| offer.grantable())
            .map(|(&id, offer)| VehicleCatalogEntry {
                id,
                key: &offer.key,
                name: &offer.name,
                tier: offer.level,
            })
    }

    pub fn vehicle_catalog_entry(&self, key: &str) -> Option<VehicleCatalogEntry<'_>> {
        let id = *self.keys.get(key)?;
        let offer = self.offers.get(&id)?;
        Some(VehicleCatalogEntry {
            id,
            key: &offer.key,
            name: &offer.name,
            tier: offer.level,
        })
    }

    #[tracing::instrument(name = "accounts.grants.plan_grant", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = snapshot.id, input.revision = snapshot.revision), err)]
    pub fn plan_grant(
        &self,
        mut snapshot: AccountSnapshot,
        grant: PlayerGrant,
    ) -> Result<ProgressionChange, GrantError> {
        let previous_revision = snapshot.revision;
        let acquired_vehicle = match grant {
            PlayerGrant::Currency { currency, amount } => {
                currency.grant(&mut snapshot.balances, amount)?;
                None
            }
            PlayerGrant::Attachment { id, count } => {
                let rule = self
                    .cosmetics
                    .grantable_attachment(id)
                    .ok_or(GrantError::UnknownAttachment)?;
                snapshot
                    .progression
                    .cosmetics
                    .grant_attachment(id, count, rule.max_owned)?;
                None
            }
            PlayerGrant::Vehicle { vehicle } => {
                let offer = self
                    .offers
                    .get(&vehicle)
                    .filter(|offer| offer.grantable())
                    .ok_or(ProgressionError::VehicleNotFound)?;
                snapshot.check_vehicle_capacity(&offer.key)?;
                let template = self.vehicle(&offer.key).map_err(Self::catalog_failure)?;
                let ammunition = template
                    .ammunition_rules(&template.modules.rules.stock)
                    .ok_or(ProgressionError::CatalogUnavailable)?
                    .defaults
                    .clone();
                let mut roster = self.crew_roster(&snapshot).map_err(Self::catalog_failure)?;
                let inventory_id = snapshot.next_vehicle_id()?;
                snapshot.add_vehicle(inventory_id, &offer.key, ammunition, true);
                let spec = self.owned_crew_vehicle(&snapshot, inventory_id)?;
                self.add_vehicle_crew(&mut roster, &spec)?;
                snapshot.crew = Some(roster);
                snapshot.progression.unlocks.insert(vehicle);
                Some(inventory_id)
            }
        };
        snapshot.revision = previous_revision
            .checked_add(1)
            .ok_or(ProgressionError::InvalidRequest)?;
        Ok(ProgressionChange {
            acquired_vehicle,
            sold_vehicle: None,
            consumable_purchases: Vec::new(),
            shell_purchases: Vec::new(),
            hired_crew_member: None,
            snapshot,
            previous_revision,
        })
    }

    pub fn grant_diff(
        &self,
        previous: &AccountSnapshot,
        change: &ProgressionChange,
        grant: PlayerGrant,
    ) -> anyhow::Result<Value> {
        match grant {
            PlayerGrant::Currency { currency, .. } => {
                use crate::accounts::GrantCurrency;
                let key = match currency {
                    GrantCurrency::Credits => "credits",
                    GrantCurrency::Gold => "gold",
                    GrantCurrency::Bonds => "crystal",
                    GrantCurrency::FreeXp => "freeXP",
                };
                Ok(crate::pickle!({
                    "prevRev": (change.previous_revision),
                    "rev": (change.snapshot.revision),
                    "stats": {(key): (currency.balance(&change.snapshot.balances))}
                }))
            }
            PlayerGrant::Vehicle { .. } | PlayerGrant::Attachment { .. } => {
                self.progression_diff(previous, change)
            }
        }
    }
}
