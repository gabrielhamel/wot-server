use super::{
    item_defs::read_xml,
    item_prices::{ItemPrices, NO_NATION},
    items::{IntCompactDescr, item_type},
    localization::{humanize_item_name, localized_names},
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
use std::collections::{BTreeMap, BTreeSet};
use wot_resources::ResourceBundle;

const VEHICLE_COMMON: &str = "scripts/item_defs/vehicles/common";

#[derive(Default)]
pub(super) struct EquipmentGrantCatalog {
    pub(super) bounty: BTreeMap<i32, String>,
    pub(super) experimental: BTreeMap<i32, String>,
}

pub(super) fn load(
    fs: &ResourceBundle,
    prices: &ItemPrices,
) -> Result<(EquipmentRules, EquipmentGrantCatalog)> {
    let definitions = read_xml(fs, &format!("{VEHICLE_COMMON}/optional_devices.xml"))?;
    let names = localized_names(fs, "text/lc_messages/artefacts.mo").unwrap_or_default();
    let mut grants = EquipmentGrantCatalog::default();
    let mut rules = EquipmentRules::default();
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
        let localized_name = || {
            device
                .get_child_string("userString")
                .and_then(|value| value.trim().strip_prefix("#artefacts:"))
                .and_then(|key| names.get(key))
                .map(|name| name.replace('\u{a0}', " "))
        };
        if let Some((prefix, level)) = [("trophyBasic", "Basic"), ("trophyUpgraded", "Upgraded")]
            .into_iter()
            .find(|(tag, _)| tags.contains(*tag))
        {
            let label = localized_name().unwrap_or_else(|| {
                let family = name.strip_prefix(prefix).unwrap_or(name);
                format!("Bounty {}", humanize_item_name(family))
            });
            grants.bounty.insert(item, format!("{label} · {level}"));
        }
        if let Some(tier) = ["modernized_1", "modernized_2", "modernized_3"]
            .iter()
            .position(|&tag| tags.contains(tag))
            .map(|index| index + 1)
        {
            let label = localized_name().unwrap_or_else(|| {
                let family = match device.get_child_string("archetype").map(str::trim) {
                    Some("modernizedAimDrivesAimingStabilizer") => "Fire-Control System",
                    Some("modernizedTurbochargerRotationMechanism") => {
                        "Mobility Improvement System"
                    }
                    Some("modernizedExtraHealthReserveAntifragmentationLining") => {
                        "Survival Improvement Suite"
                    }
                    Some("modernizedImprovedSightsEnhancedAimDrives") => {
                        "Accuracy Improvement System"
                    }
                    _ => "Experimental Equipment",
                };
                format!("{family} T{tier}")
            });
            grants.experimental.insert(item, label);
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
    Ok((rules, grants))
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
