use super::{
    StoreOffer,
    item_defs::read_xml,
    item_prices::{ItemPrices, NO_NATION},
    items::{IntCompactDescr, item_type},
    shop::{
        DELUXE_DEMOUNT_BONDS, EQUIPMENT_DEMOUNT_GOLD, MODERNIZED_2_DEMOUNT_COMPONENTS,
        MODERNIZED_3_DEMOUNT_COMPONENTS,
    },
};
use crate::{
    accounts::{
        EquipmentDefinition, EquipmentPrice, EquipmentRules, EquipmentVehicle,
        EquipmentVehicleFilter, MAX_EQUIPMENT_SLOTS,
    },
    platform::client_formats::pxml::Element,
};
use anyhow::{Context, Result};
use std::collections::BTreeSet;
use wot_resources::ResourceBundle;

const VEHICLE_COMMON: &str = "scripts/item_defs/vehicles/common";

pub(super) fn load(
    fs: &ResourceBundle,
    prices: &ItemPrices,
) -> Result<(EquipmentRules, Vec<StoreOffer>)> {
    let definitions = read_xml(fs, &format!("{VEHICLE_COMMON}/optional_devices.xml"))?;
    let mut rules = EquipmentRules::default();
    let mut offers = Vec::new();
    for (name, device) in definitions.iter_child_elements() {
        let Some(id) = device.get_child_integer("id") else {
            continue;
        };
        let cd = IntCompactDescr {
            item_type: item_type::OPTIONAL_DEVICE,
            nation: NO_NATION,
            id: u16::try_from(id)?,
        };
        let price = prices
            .prices
            .iter()
            .find(|(id, _)| *id == cd && !prices.not_in_shop.contains(id))
            .and_then(|(_, p)| match p.currency {
                "credits" => Some(EquipmentPrice::Credits(p.amount)),
                "gold" => Some(EquipmentPrice::Gold(p.amount)),
                "crystal" => Some(EquipmentPrice::Bonds(p.amount)),
                "equipCoin" => Some(EquipmentPrice::Components(p.amount)),
                _ => None,
            });
        let tags = words(device, "tags");
        let demount_cost = if tags.contains("deluxe") {
            EquipmentPrice::Bonds(DELUXE_DEMOUNT_BONDS)
        } else if tags.contains("modernized_2") {
            EquipmentPrice::Components(MODERNIZED_2_DEMOUNT_COMPONENTS)
        } else if tags.contains("modernized_3") {
            EquipmentPrice::Components(MODERNIZED_3_DEMOUNT_COMPONENTS)
        } else {
            EquipmentPrice::Gold(EQUIPMENT_DEMOUNT_GOLD)
        };
        let (include, exclude) = vehicle_filters(name, device)?;
        let incompatible = device
            .get_child_element("incompatibleTags")
            .map_or_else(BTreeSet::new, |filter| words(filter, "installed"));
        let item = i32::try_from(cd.to_int())?;
        if let Some(EquipmentPrice::Credits(credits)) = price
            && credits > 0
            && !prices.not_for_sale.contains(&cd)
            && let Some(name) = store_equipment_name(name)
        {
            offers.push(StoreOffer {
                item,
                name,
                credits,
            });
        }
        rules.items.insert(
            item,
            EquipmentDefinition {
                price,
                removable: device.get_child_boolean("removable").unwrap_or(false),
                demount_cost,
                tags,
                incompatible,
                include,
                exclude,
            },
        );
    }
    offers.sort_by_key(|offer| (offer.credits, offer.name));
    Ok((rules, offers))
}

fn store_equipment_name(key: &str) -> Option<&'static str> {
    match key {
        "improvedVentilation_tier3" => Some("Improved Ventilation Class 3"),
        "improvedVentilation_tier2" => Some("Improved Ventilation Class 2"),
        "improvedVentilation_tier1" => Some("Improved Ventilation Class 1"),
        "turbocharger_tier3" => Some("Turbocharger Class 3"),
        "turbocharger_tier2" => Some("Turbocharger Class 2"),
        "turbocharger_tier1" => Some("Turbocharger Class 1"),
        _ => None,
    }
}

pub(super) fn vehicle(
    nation: &str,
    offer: &super::vehicle_defs::VehicleOffer,
    slots: usize,
) -> Result<EquipmentVehicle> {
    anyhow::ensure!(
        slots <= MAX_EQUIPMENT_SLOTS,
        "Too many vehicle equipment slots"
    );
    Ok(EquipmentVehicle {
        slots,
        nation: nation.to_owned(),
        level: offer.level.context("Missing vehicle level")?,
        tags: offer.tags.clone(),
    })
}

pub(super) fn vehicle_filters(
    name: &str,
    item: &Element,
) -> Result<(Vec<EquipmentVehicleFilter>, Vec<EquipmentVehicleFilter>)> {
    let mut include = Vec::new();
    let mut exclude = Vec::new();
    if let Some(filter) = item.get_child_element("vehicleFilter") {
        for (kind, clause) in filter.iter_child_elements() {
            anyhow::ensure!(
                clause.get_child("componentFilters").is_none(),
                "Unsupported equipment component filter for {name}"
            );
            match kind {
                "include" => include.push(vehicle_filter(clause)),
                "exclude" => exclude.push(vehicle_filter(clause)),
                _ => anyhow::bail!("Unknown equipment filter clause {kind} for {name}"),
            }
        }
    }
    Ok((include, exclude))
}

fn vehicle_filter(clause: &Element) -> EquipmentVehicleFilter {
    let mut filter = EquipmentVehicleFilter {
        nations: words(clause, "nations"),
        ..EquipmentVehicleFilter::default()
    };
    if let Some(vehicle) = clause.get_child_element("vehicle") {
        filter.min_level = vehicle.get_child_integer("minLevel");
        filter.max_level = vehicle.get_child_integer("maxLevel");
        filter.tags = words(vehicle, "tags");
        filter.mandatory_tags = words(vehicle, "mandatoryTags");
    }
    filter
}

pub(super) fn words(element: &Element, field: &str) -> BTreeSet<String> {
    element
        .get_child_string(field)
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}
