use super::{
    GarageCatalog, item_defs,
    item_prices::{ItemPrices, Price},
    items::IntCompactDescr,
    localization::localized_names,
};
use crate::{AccountSnapshot, ProgressionError, ShellCurrency, VehicleCost, VehiclePurchaseRules};
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use wot_resources::ResourceBundle;

const FORBIDDEN_FOR_BATTLE: &str =
    "scripts/item_defs/vehicles/common/forbidden_vehicles_to_battle_config.xml";
pub(super) const MAX_PLACEHOLDER_VEHICLE_PRICE: i64 = 100;
const NON_GARAGE_TAGS: &[&str] = &[
    "event_battles",
    "fallout",
    "epic_battles",
    "bob",
    "battle_royale",
    "clanWarsBattles",
    "fun_random",
    "comp7",
    "comp7_light",
    "fort_rush",
    "maps_training",
    "premiumIGR",
    "observer",
    "pillbox",
    "mode_hidden",
];

pub(super) struct VehicleOffer {
    pub key: String,
    pub name: String,
    pub price: Option<VehicleCost>,
    pub definition_price: Price,
    pub requires_research: bool,
    pub level: Option<i64>,
    pub tags: BTreeSet<String>,
    forbidden_for_battle: bool,
    acquisition_unavailable: bool,
}

impl VehicleOffer {
    fn is_private_or_test(&self) -> bool {
        self.tags.contains("private") || self.tags.contains("testTank")
    }

    pub(super) fn grantable(&self) -> bool {
        self.level.is_some_and(|level| (1..=11).contains(&level))
            && !self.forbidden_for_battle
            && !self.acquisition_unavailable
            && !NON_GARAGE_TAGS.iter().any(|tag| self.tags.contains(*tag))
    }

    pub(super) fn store_eligible(&self) -> bool {
        self.grantable()
            && (!self.is_private_or_test()
                || (matches!(self.definition_price.currency, "gold" | "credits")
                    && self.definition_price.amount > MAX_PLACEHOLDER_VEHICLE_PRICE))
    }
}

impl GarageCatalog {
    #[tracing::instrument(name = "accounts.catalog.validate_vehicle_variants", skip_all, fields(ddd.bounded_context.name = "accounts", input.vehicle_count = self.offers.len(), output.accepted_count = tracing::field::Empty, output.rejected_count = tracing::field::Empty))]
    pub(super) fn validate_vehicle_variants(&mut self) {
        let mut accepted = 0;
        let mut unavailable = Vec::new();
        let variants = self
            .offers
            .iter()
            .filter(|(_, offer)| offer.grantable() && offer.is_private_or_test());
        for (&id, offer) in variants {
            let result = self.vehicle(&offer.key).and_then(|vehicle| {
                let modules = &vehicle.modules;
                let stock = &modules.rules.stock;
                let has_health = stock
                    .get(&crate::ModuleKind::Turret)
                    .and_then(|turret| modules.training_health.get(turret))
                    .is_some_and(|&health| health > 0);
                anyhow::ensure!(has_health, "Missing stock vehicle health");
                anyhow::ensure!(
                    modules.battle_performance(stock).is_some(),
                    "Missing stock vehicle performance"
                );
                anyhow::ensure!(!vehicle.crew_roles.is_empty(), "Missing vehicle crew");
                anyhow::ensure!(
                    vehicle
                        .ammunition_rules(stock)
                        .is_some_and(|ammo| ammo.defaults.iter().any(|&(_, count)| count > 0)),
                    "Missing stock vehicle ammunition"
                );
                Ok(())
            });
            match result {
                Ok(()) => accepted += 1,
                Err(error) => {
                    tracing::warn!(vehicle.id = id, vehicle.key = %offer.key, %error, "Vehicle variant is unavailable for acquisition");
                    unavailable.push(id);
                }
            }
        }
        let span = tracing::Span::current();
        span.record("output.accepted_count", accepted);
        span.record("output.rejected_count", unavailable.len());
        for id in unavailable {
            if let Some(offer) = self.offers.get_mut(&id) {
                offer.acquisition_unavailable = true;
            }
        }
    }
}

pub(super) fn offers(
    fs: &ResourceBundle,
    prices: &ItemPrices,
) -> Result<BTreeMap<i32, VehicleOffer>> {
    let forbidden = match item_defs::read_xml(fs, FORBIDDEN_FOR_BATTLE) {
        Ok(config) => config
            .get_child_element("forbiddenVehicles")
            .into_iter()
            .flat_map(|section| section.iter_children_all())
            .filter_map(|(_, value)| {
                value
                    .as_string()
                    .or_else(|| value.as_element()?.value.as_string())
            })
            .map(str::trim)
            .map(str::to_owned)
            .collect::<BTreeSet<_>>(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeSet::new(),
        Err(error) => return Err(error.into()),
    };
    let mut offers = BTreeMap::new();
    for (nation, nation_name) in item_defs::NATIONS.iter().enumerate() {
        let locale = if *nation_name == "uk" && fs.has_file("text/lc_messages/gb_vehicles.mo") {
            "gb"
        } else {
            nation_name
        };
        let path = format!("text/lc_messages/{locale}_vehicles.mo");
        let names = localized_names(fs, &path).unwrap_or_default();
        for dir in item_defs::vehicle_dirs(fs, nation_name) {
            let list = item_defs::read_xml(fs, &format!("{dir}/list.xml"))?;
            for (name, vehicle) in list.iter_child_elements() {
                let Some(id) = vehicle.get_child_integer("id") else {
                    continue;
                };
                let cd = IntCompactDescr {
                    item_type: 1,
                    nation: u8::try_from(nation)?,
                    id: u16::try_from(id)?,
                };
                let price = prices
                    .prices
                    .iter()
                    .find(|(id, _)| *id == cd)
                    .filter(|(id, p)| p.amount >= 0 && !prices.not_in_shop.contains(id))
                    .and_then(|(_, p)| match p.currency {
                        "credits" => Some(VehicleCost {
                            credits: p.amount,
                            gold: 0,
                        }),
                        "gold" => Some(VehicleCost {
                            credits: 0,
                            gold: p.amount,
                        }),
                        _ => None,
                    });
                let premium = vehicle
                    .get_child_string("tags")
                    .is_some_and(|tags| tags.split_whitespace().any(|tag| tag == "premium"));
                let key = format!("{nation_name}:{name}");
                offers.entry(cd.to_int() as i32).or_insert(VehicleOffer {
                    forbidden_for_battle: forbidden.contains(&key),
                    acquisition_unavailable: false,
                    key,
                    name: vehicle
                        .get_child_string("userString")
                        .and_then(|label| label.split_once(':'))
                        .and_then(|(_, label)| names.get(label))
                        .cloned()
                        .unwrap_or_else(|| {
                            name.split_once('_')
                                .map_or(name, |(_, name)| name)
                                .replace('_', " ")
                        }),
                    price,
                    definition_price: ItemPrices::definition_price(vehicle),
                    level: vehicle.get_child_integer("level"),
                    tags: vehicle
                        .get_child_string("tags")
                        .unwrap_or_default()
                        .split_whitespace()
                        .map(str::to_owned)
                        .collect(),
                    requires_research: !premium && price.is_none_or(|p| p.gold == 0),
                });
            }
        }
    }
    Ok(offers)
}

impl GarageCatalog {
    pub(crate) fn max_researched_tiers(&self, account: &AccountSnapshot) -> BTreeMap<u8, i64> {
        let mut tiers = BTreeMap::new();
        for id in account.progression.unlocks.iter().copied().chain(
            account
                .vehicles
                .iter()
                .filter_map(|vehicle| self.keys.get(&vehicle.catalog_key).copied()),
        ) {
            if self.tech_tree.contains(&id)
                && let Some(level) = self.offers.get(&id).and_then(|vehicle| vehicle.level)
            {
                let nation = IntCompactDescr::from_int(i64::from(id)).nation;
                let maximum = tiers.entry(nation).or_insert(1);
                *maximum = (*maximum).max(level);
            }
        }
        tiers
    }

    pub(super) fn collector_unlocked(
        tiers: &BTreeMap<u8, i64>,
        item: i32,
        tier: Option<i64>,
    ) -> bool {
        let nation = IntCompactDescr::from_int(i64::from(item)).nation;
        tier.is_some_and(|tier| tiers.get(&nation).copied().unwrap_or(1) >= tier)
    }

    pub(super) fn purchase_rules(&self, id: i32) -> Result<VehiclePurchaseRules> {
        let offer = self.offers.get(&id).context("Unknown vehicle offer")?;
        let template = self.vehicle(&offer.key)?;
        let ammo = template
            .ammunition_rules(&template.modules.rules.stock)
            .context("Missing stock gun ammunition")?;
        let ammunition_cost = ammo
            .defaults
            .iter()
            .filter(|(_, count)| *count > 0)
            .try_fold(VehicleCost::default(), |cost, &(id, count)| {
                let price = ammo.prices.get(&id)?;
                let amount = price
                    .amount
                    .checked_mul(i64::from(count))
                    .filter(|&n| n >= 0)?;
                cost.checked_add(match price.currency {
                    ShellCurrency::Credits => VehicleCost {
                        credits: amount,
                        gold: 0,
                    },
                    ShellCurrency::Gold => VehicleCost {
                        credits: 0,
                        gold: amount,
                    },
                })
                .ok()
            });
        Ok(VehiclePurchaseRules {
            catalog_key: offer.key.clone(),
            price: offer.price,
            requires_research: offer.requires_research,
            crew_size: u32::try_from(template.crew_roles.len())?,
            ammunition: ammo.defaults.clone(),
            ammunition_cost,
        })
    }

    pub(super) fn vehicle_by_id(&self, id: i32) -> Result<std::sync::Arc<super::CatalogVehicle>> {
        self.vehicle(&self.offers.get(&id).context("Unknown vehicle type")?.key)
    }

    pub(super) fn catalog_failure(error: anyhow::Error) -> ProgressionError {
        tracing::error!(%error,"Load garage vehicle definitions failed");
        ProgressionError::CatalogUnavailable
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wot_platform::client_formats::pxml::Element;

    #[test]
    fn store_requires_private_and_test_variants_to_have_a_price_above_100() {
        let mut vehicle = VehicleOffer {
            key: String::new(),
            name: String::new(),
            price: None,
            definition_price: ItemPrices::definition_price(&Element::new()),
            requires_research: false,
            level: Some(8),
            tags: BTreeSet::new(),
            forbidden_for_battle: false,
            acquisition_unavailable: false,
        };
        let undefined = vehicle.definition_price;
        for tag in ["private", "testTank"] {
            vehicle.tags = BTreeSet::from([tag.to_owned()]);
            vehicle.definition_price = undefined;
            assert!(vehicle.grantable());
            assert!(!vehicle.store_eligible());
            for currency in ["gold", "credits"] {
                for (amount, eligible) in [(1, false), (100, false), (101, true)] {
                    vehicle.definition_price = Price { currency, amount };
                    assert!(vehicle.grantable());
                    assert_eq!(
                        vehicle.store_eligible(),
                        eligible,
                        "{tag}: {amount} {currency}"
                    );
                }
            }
        }
        vehicle.tags.clear();
        vehicle.definition_price = Price {
            currency: "gold",
            amount: 1,
        };
        assert!(vehicle.store_eligible());
    }
}
