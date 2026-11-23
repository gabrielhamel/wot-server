use super::{
    item_defs::read_xml,
    item_prices::{ItemPrices, NO_NATION},
    items::{IntCompactDescr, item_type},
};
use crate::{DirectivePrice, DirectiveRules};
use anyhow::{Context, Result};
use wot_resources::ResourceBundle;

pub(super) fn load(fs: &ResourceBundle, prices: &ItemPrices) -> Result<DirectiveRules> {
    let definitions = read_xml(fs, "scripts/item_defs/vehicles/common/equipments.xml")?;
    let mut rules = DirectiveRules::default();
    for (name, directive) in definitions.iter_child_elements() {
        if directive.get_child_string("type") != Some("battleBoosters") {
            continue;
        }
        let id = directive
            .get_child_integer("id")
            .with_context(|| format!("Missing directive ID for {name}"))?;
        let cd = IntCompactDescr {
            item_type: item_type::EQUIPMENT,
            nation: NO_NATION,
            id: u16::try_from(id)?,
        };
        let price = prices
            .prices
            .iter()
            .find(|(id, _)| *id == cd)
            .filter(|(id, _)| !prices.not_in_shop.contains(id))
            .and_then(|(_, price)| match price.currency {
                "credits" => Some(DirectivePrice::Credits(price.amount)),
                "crystal" => Some(DirectivePrice::Bonds(price.amount)),
                _ => None,
            });
        rules.items.insert(i32::try_from(cd.to_int())?, price);
    }
    Ok(rules)
}
