use super::{GarageCatalog, cosmetic_catalog::StyleDimension};
use crate::{
    AccountSnapshot, CosmeticKind, GrantError, PlayerGrant, ProgressionChange, ProgressionError,
};
use std::collections::BTreeMap;
use wot_platform::client_formats::pickle::Value;

pub struct VehicleCatalogEntry<'a> {
    pub id: i32,
    pub key: &'a str,
    pub name: &'a str,
    pub tier: Option<i64>,
}

pub struct CosmeticGrantEntry<'a> {
    pub id: u16,
    pub name: &'a str,
    pub max_owned: Option<u32>,
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

    pub fn grantable_crew_books(&self) -> impl Iterator<Item = (i32, &str)> {
        self.crew_books
            .iter()
            .map(|(&id, book)| (id, book.name.as_str()))
    }

    pub fn grantable_improved_equipment(&self) -> impl Iterator<Item = (i32, &str)> {
        self.equipment_grants
            .improved
            .iter()
            .map(|(&id, name)| (id, name.as_str()))
    }

    pub fn grantable_bounty_equipment(&self) -> impl Iterator<Item = (i32, &str)> {
        self.equipment_grants
            .bounty
            .iter()
            .map(|(&id, name)| (id, name.as_str()))
    }

    pub fn grantable_experimental_equipment(&self) -> impl Iterator<Item = (i32, &str)> {
        self.equipment_grants
            .experimental
            .iter()
            .map(|(&id, name)| (id, name.as_str()))
    }

    pub fn grantable_styles_2d(&self) -> impl Iterator<Item = CosmeticGrantEntry<'_>> {
        self.grantable_styles(StyleDimension::Two)
    }

    pub fn grantable_styles_3d(&self) -> impl Iterator<Item = CosmeticGrantEntry<'_>> {
        self.grantable_styles(StyleDimension::Three)
    }

    fn grantable_styles(
        &self,
        dimension: StyleDimension,
    ) -> impl Iterator<Item = CosmeticGrantEntry<'_>> {
        self.cosmetic_grant_entries(CosmeticKind::Style)
            .filter(move |entry| self.cosmetics.style_dimension(entry.id) == Some(dimension))
    }

    pub(crate) fn style_counts(
        &self,
        snapshot: &AccountSnapshot,
    ) -> (BTreeMap<u16, u32>, BTreeMap<u16, u32>) {
        let mut styles_2d = BTreeMap::new();
        let mut styles_3d = BTreeMap::new();
        if let Some(styles) = snapshot
            .progression
            .cosmetics
            .owned()
            .get(&CosmeticKind::Style)
        {
            for (&id, &count) in styles {
                match self.cosmetics.style_dimension(id) {
                    Some(StyleDimension::Two) => {
                        styles_2d.insert(id, count);
                    }
                    Some(StyleDimension::Three) => {
                        styles_3d.insert(id, count);
                    }
                    None => {}
                }
            }
        }
        (styles_2d, styles_3d)
    }

    pub fn grantable_crew_skins(&self) -> impl Iterator<Item = CosmeticGrantEntry<'_>> {
        self.crew_skins
            .iter()
            .filter(|(_, skin)| skin.eu)
            .map(|(&id, skin)| CosmeticGrantEntry {
                id,
                name: &skin.name,
                max_owned: skin.max_owned,
            })
    }

    pub fn grantable_attachments(&self) -> impl Iterator<Item = CosmeticGrantEntry<'_>> {
        self.cosmetic_grant_entries(CosmeticKind::Attachment)
    }

    pub fn grantable_stat_trackers(&self) -> impl Iterator<Item = CosmeticGrantEntry<'_>> {
        self.cosmetic_grant_entries(CosmeticKind::StatTracker)
    }

    fn cosmetic_grant_entries(
        &self,
        kind: CosmeticKind,
    ) -> impl Iterator<Item = CosmeticGrantEntry<'_>> {
        self.cosmetics
            .grantable_cosmetics(kind)
            .map(|(id, name, max_owned)| CosmeticGrantEntry {
                id,
                name,
                max_owned,
            })
    }

    pub fn stat_tracker_options(
        &self,
        snapshot: &AccountSnapshot,
        inventory_id: i64,
    ) -> Option<(i32, Vec<u16>)> {
        let (vehicle_type, _, rules) = self
            .vehicle_cosmetic_rules(snapshot, inventory_id, Some(CosmeticKind::StatTracker))
            .ok()?;
        Some((vehicle_type, rules.into_keys().map(|(_, id)| id).collect()))
    }

    #[tracing::instrument(name = "accounts.grants.plan_grant", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = snapshot.id, input.revision = snapshot.revision, input.grant = ?grant, output.revision = tracing::field::Empty), err)]
    pub fn plan_grant(
        &self,
        mut snapshot: AccountSnapshot,
        grant: PlayerGrant,
    ) -> Result<ProgressionChange, GrantError> {
        let previous_revision = snapshot.revision;
        let mut hired_crew_member = None;
        let acquired_vehicle = match grant {
            PlayerGrant::Currency { currency, amount } => {
                currency.grant(&mut snapshot.balances, amount)?;
                None
            }
            PlayerGrant::CrewBook { id, count } => {
                if !self.crew_books.contains_key(&id) {
                    return Err(GrantError::UnknownCrewBook);
                }
                snapshot.progression.grant_depot_stock(id, count)?;
                None
            }
            PlayerGrant::ImprovedEquipment { id, count } => {
                if !self.equipment_grants.improved.contains_key(&id) {
                    return Err(GrantError::UnknownImprovedEquipment);
                }
                snapshot.progression.grant_depot_stock(id, count)?;
                None
            }
            PlayerGrant::BountyEquipment { id, count } => {
                if !self.equipment_grants.bounty.contains_key(&id) {
                    return Err(GrantError::UnknownBountyEquipment);
                }
                snapshot.progression.grant_depot_stock(id, count)?;
                None
            }
            PlayerGrant::ExperimentalEquipment { id, count } => {
                if !self.equipment_grants.experimental.contains_key(&id) {
                    return Err(GrantError::UnknownExperimentalEquipment);
                }
                snapshot.progression.grant_depot_stock(id, count)?;
                None
            }
            PlayerGrant::Style2d { id, count } | PlayerGrant::Style3d { id, count } => {
                let dimension = if matches!(grant, PlayerGrant::Style2d { .. }) {
                    StyleDimension::Two
                } else {
                    StyleDimension::Three
                };
                if !(1..=100).contains(&count) {
                    return Err(GrantError::InvalidDepotCount);
                }
                let limit = self
                    .cosmetics
                    .style_grant_limit(id, dimension)
                    .ok_or(GrantError::UnknownStyle)?;
                snapshot
                    .progression
                    .cosmetics
                    .add_style(id, count, limit)
                    .map_err(|_| GrantError::StyleLimit)?;
                None
            }
            PlayerGrant::CrewSkin { id, count } => {
                if !(1..=100).contains(&count) {
                    return Err(GrantError::InvalidDepotCount);
                }
                if !self.crew_skins.get(&id).is_some_and(|skin| skin.eu) {
                    return Err(GrantError::UnknownCrewSkin);
                }
                let item = super::crew_skins::descriptor(id).to_int() as i32;
                self.check_crew_skin_limit(&snapshot, item, count).map_err(
                    |error| match error {
                        ProgressionError::InvalidRequest => GrantError::CrewSkinLimit,
                        other => GrantError::Vehicle(other),
                    },
                )?;
                snapshot.progression.grant_depot_stock(item, count)?;
                None
            }
            PlayerGrant::SpecialRecruit { id } => {
                hired_crew_member = Some(self.add_special_recruit(&mut snapshot, id)?);
                None
            }
            PlayerGrant::Attachment { id, count } => {
                let rule = self
                    .cosmetics
                    .grantable_rule((CosmeticKind::Attachment, id))
                    .ok_or(GrantError::UnknownAttachment)?;
                snapshot
                    .progression
                    .cosmetics
                    .grant_attachment(id, count, rule.max_owned)?;
                None
            }
            PlayerGrant::StatTracker { vehicle, id, count } => {
                let (vehicle_type, _, rules) = self
                    .vehicle_cosmetic_rules(&snapshot, vehicle, Some(CosmeticKind::StatTracker))
                    .map_err(|_| GrantError::IncompatibleStatTracker)?;
                let rule = rules
                    .get(&(CosmeticKind::StatTracker, id))
                    .ok_or(GrantError::IncompatibleStatTracker)?;
                snapshot.progression.cosmetics.grant_stat_tracker(
                    vehicle_type,
                    id,
                    count,
                    rule.max_owned,
                )?;
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
        tracing::Span::current().record("output.revision", snapshot.revision);
        Ok(ProgressionChange {
            acquired_vehicle,
            sold_vehicle: None,
            consumable_purchases: Vec::new(),
            shell_purchases: Vec::new(),
            hired_crew_member,
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
                use crate::GrantCurrency;
                let key = match currency {
                    GrantCurrency::Credits => "credits",
                    GrantCurrency::Gold => "gold",
                    GrantCurrency::Bonds => "crystal",
                    GrantCurrency::FreeXp => "freeXP",
                    GrantCurrency::EquipmentComponents => "equipCoin",
                };
                Ok(wot_platform::pickle!({
                    "prevRev": (change.previous_revision),
                    "rev": (change.snapshot.revision),
                    "stats": {(key): (currency.balance(&change.snapshot.balances))}
                }))
            }
            PlayerGrant::Vehicle { .. }
            | PlayerGrant::Style2d { .. }
            | PlayerGrant::Style3d { .. }
            | PlayerGrant::CrewSkin { .. }
            | PlayerGrant::SpecialRecruit { .. }
            | PlayerGrant::CrewBook { .. }
            | PlayerGrant::ImprovedEquipment { .. }
            | PlayerGrant::BountyEquipment { .. }
            | PlayerGrant::ExperimentalEquipment { .. }
            | PlayerGrant::Attachment { .. }
            | PlayerGrant::StatTracker { .. } => self.progression_diff(previous, change),
        }
    }
}
