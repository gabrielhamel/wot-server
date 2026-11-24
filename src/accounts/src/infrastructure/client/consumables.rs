use super::{
    equipment::{vehicle_filters, words},
    item_defs::read_xml,
    item_prices::{ItemPrices, NO_NATION},
    items::{IntCompactDescr, item_type},
    shop::AMMO_EXCHANGE_RATE,
};
use crate::{ConsumableDefinition, ConsumableRules, VehicleCost};
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use wot_resources::ResourceBundle;

pub(super) fn load(
    fs: &ResourceBundle,
    prices: &ItemPrices,
) -> Result<(ConsumableRules, BTreeMap<i32, String>)> {
    let translations = super::localization::localized_names(fs, "text/lc_messages/artefacts.mo")
        .unwrap_or_default();
    let mut names = BTreeMap::new();
    let definitions = read_xml(fs, "scripts/item_defs/vehicles/common/equipments.xml")?;
    let mut items = BTreeMap::new();
    for (name, item) in definitions.iter_child_elements() {
        if item.get_child_string("type").unwrap_or("regular") != "regular" {
            continue;
        }
        let tags = words(item, "tags");
        if tags.contains("builtin") || tags.contains("notForRegularSlot") {
            continue;
        }
        let id = item
            .get_child_integer("id")
            .with_context(|| format!("Missing consumable ID for {name}"))?;
        let cd = IntCompactDescr {
            item_type: item_type::EQUIPMENT,
            nation: NO_NATION,
            id: u16::try_from(id)?,
        };
        let price = prices
            .prices
            .iter()
            .find(|(id, _)| *id == cd && !prices.not_in_shop.contains(id))
            .and_then(|(_, price)| match price.currency {
                "credits" => Some(VehicleCost {
                    credits: price.amount,
                    gold: 0,
                }),
                "gold" => Some(VehicleCost {
                    credits: 0,
                    gold: price.amount,
                }),
                _ => None,
            });
        let (include, exclude) = vehicle_filters(name, item)?;
        let incompatible = item
            .get_child_element("incompatibleTags")
            .map(|filter| {
                words(filter, "installed")
                    .into_iter()
                    .chain(words(filter, "active"))
                    .collect()
            })
            .unwrap_or_default();
        let id = i32::try_from(cd.to_int())?;
        let label = item
            .get_child_string("userString")
            .and_then(|value| value.trim().strip_prefix("#artefacts:"))
            .and_then(|key| translations.get(key))
            .cloned()
            .unwrap_or_else(|| super::localization::humanize_item_name(name));
        names.insert(id, label);
        items.insert(
            id,
            ConsumableDefinition {
                price,
                tags,
                incompatible,
                include,
                exclude,
            },
        );
    }
    Ok((
        ConsumableRules {
            items,
            gold_to_credits: AMMO_EXCHANGE_RATE,
        },
        names,
    ))
}
