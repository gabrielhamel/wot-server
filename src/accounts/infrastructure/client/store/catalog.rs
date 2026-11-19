#[path = "vehicle_offers.rs"]
mod vehicle_offers;

use super::{StoreCategory as Category, StoreOffer, StorePrice};
use crate::accounts::infrastructure::client::{
    item_defs::{NATIONS, read_xml},
    item_prices::{ItemPrices, NO_NATION},
    items::{IntCompactDescr, item_type},
    localization::localized_names,
    vehicle_defs::VehicleOffer,
};
use crate::accounts::{
    ConsumableRules, DirectivePrice, DirectiveRules, EquipmentPrice, EquipmentRules,
    PREMIUM_PACKAGES, VehicleCost,
};
use anyhow::Result;
use std::collections::BTreeMap;
use wot_resources::ResourceBundle;

pub(in crate::accounts::infrastructure::client) fn load(
    fs: &ResourceBundle,
    prices: &ItemPrices,
    vehicles: &BTreeMap<i32, VehicleOffer>,
    equipment: &EquipmentRules,
    consumables: &ConsumableRules,
    directives: &DirectiveRules,
) -> Result<Vec<StoreOffer>> {
    let mut offers = PREMIUM_PACKAGES
        .into_iter()
        .map(|(days, price)| {
            let name = if days == 1 {
                "1 day".to_owned()
            } else {
                format!("{days} days")
            };
            listing(
                -days,
                name,
                Category::PremiumAccount,
                StorePrice::Gold(price),
            )
        })
        .collect::<Vec<_>>();
    let artefacts = localized_names(fs, "text/lc_messages/artefacts.mo").unwrap_or_default();
    for (path, kind) in [
        ("optional_devices.xml", item_type::OPTIONAL_DEVICE),
        ("equipments.xml", item_type::EQUIPMENT),
    ] {
        let definitions = read_xml(fs, &format!("scripts/item_defs/vehicles/common/{path}"))?;
        for (key, def) in definitions.iter_child_elements() {
            let Some(id) = def.get_child_integer("id") else {
                continue;
            };
            let cd = IntCompactDescr {
                item_type: kind,
                nation: NO_NATION,
                id: u16::try_from(id)?,
            };
            let item = i32::try_from(cd.to_int())?;
            if prices.not_in_shop.contains(&cd) {
                continue;
            }
            let offer = if let Some(equipment) = equipment.items.get(&item) {
                if prices.not_for_sale.contains(&cd) {
                    continue;
                }
                match equipment.price {
                    Some(EquipmentPrice::Credits(amount)) => {
                        Some((Category::Equipment, StorePrice::Credits(amount)))
                    }
                    Some(EquipmentPrice::Bonds(amount)) if equipment.tags.contains("deluxe") => {
                        Some((Category::ImprovedEquipment, StorePrice::Bonds(amount)))
                    }
                    _ => None,
                }
            } else if let Some(consumable) = consumables.items.get(&item) {
                match consumable.price {
                    Some(VehicleCost { credits, gold: 0 }) => {
                        Some((Category::Consumables, StorePrice::Credits(credits)))
                    }
                    _ => None,
                }
            } else {
                match directives.items.get(&item).copied().flatten() {
                    Some(DirectivePrice::Credits(amount)) if matches!(id, 109..=113 | 115..=116 | 205..=206) => {
                        Some((Category::CrewDirectives, StorePrice::Credits(amount)))
                    }
                    Some(DirectivePrice::Bonds(amount)) if (103..=108).contains(&id) => {
                        Some((Category::EquipmentDirectives, StorePrice::Bonds(amount)))
                    }
                    _ => None,
                }
            };
            if let Some((category, price)) = offer
                && price.amount() > 0
            {
                let name = def
                    .get_child_string("userString")
                    .and_then(|name| name.strip_prefix("#artefacts:"))
                    .and_then(|key| artefacts.get(key))
                    .map(|name| name.replace('\u{a0}', " "))
                    .unwrap_or_else(|| {
                        let base = humanize(key);
                        match (category, key.rsplit_once("_tier")) {
                            (Category::Equipment, Some((_, tier @ ("1" | "2" | "3")))) => {
                                format!("{base} Class {tier}")
                            }
                            _ => base,
                        }
                    });
                offers.push(listing(item, name, category, price));
            }
        }
    }
    offers.extend(vehicle_listings(vehicles));
    offers.extend(super::super::crew_skins::store_offers(fs, prices)?);
    offers.sort_by(|a, b| {
        a.category
            .cmp(&b.category)
            .then_with(|| b.tier.cmp(&a.tier))
            .then_with(|| {
                if a.category == Category::PremiumAccount {
                    a.price.amount().cmp(&b.price.amount())
                } else {
                    a.name.cmp(&b.name)
                }
            })
    });
    Ok(offers)
}

fn vehicle_listings(
    vehicles: &BTreeMap<i32, VehicleOffer>,
) -> impl Iterator<Item = StoreOffer> + '_ {
    let bonds: BTreeMap<_, _> = vehicle_offers::BONDS
        .iter()
        .map(|&(key, name, price)| (key, (name, StorePrice::Bonds(price))))
        .collect();
    vehicles
        .iter()
        .filter(|(_, vehicle)| vehicle.grantable())
        .flat_map(move |(&item, vehicle)| {
            let nation = NATIONS
                .get(usize::from(
                    IntCompactDescr::from_int(i64::from(item)).nation,
                ))
                .copied();
            let listing = |category, name: &str, price| StoreOffer {
                nation,
                tier: vehicle.level,
                ..listing(item, name.to_owned(), category, price)
            };
            let priced = match (vehicle.store_gold_price, vehicle.price) {
                (Some(gold), _) => Some(listing(
                    Category::PremiumVehicles,
                    &vehicle.name,
                    StorePrice::Gold(gold),
                )),
                (None, Some(VehicleCost { credits, gold: 0 }))
                    if vehicle.tags.contains("collectorVehicle") =>
                {
                    Some(listing(
                        Category::CollectorsVehicles,
                        &vehicle.name,
                        StorePrice::Credits(credits),
                    ))
                }
                _ => None,
            };
            let bond = bonds
                .get(vehicle.key.as_str())
                .map(|&(name, price)| listing(Category::BondVehicles, name, price));
            priced.into_iter().chain(bond)
        })
}

pub(in crate::accounts::infrastructure::client) fn listing(
    item: i32,
    name: String,
    category: Category,
    price: StorePrice,
) -> StoreOffer {
    StoreOffer {
        item,
        name,
        category,
        price,
        nation: None,
        tier: None,
    }
}

fn humanize(key: &str) -> String {
    let name = key.split_once("_tier").map_or(key, |(name, _)| name);
    let name = name.strip_suffix("BattleBooster").unwrap_or(name);
    let mut result = String::new();
    let mut previous_lowercase = false;
    for ch in name.chars() {
        if ch == '_' {
            result.push(' ');
        } else if result.is_empty() {
            result.extend(ch.to_uppercase());
        } else {
            if previous_lowercase && ch.is_ascii_uppercase() {
                result.push(' ');
            }
            result.push(ch);
        }
        previous_lowercase = ch.is_ascii_lowercase();
    }
    result
}
