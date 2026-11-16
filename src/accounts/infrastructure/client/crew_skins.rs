use super::{
    GarageCatalog,
    item_defs::{NATIONS, read_xml},
    item_prices::ItemPrices,
    items::{IntCompactDescr, item_type},
};
use crate::{
    accounts::{AccountSnapshot, CrewRoster, ProgressionError},
    client_formats::pickle::Value,
};
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use wot_resources::ResourceBundle;

pub(super) struct CrewSkin {
    nation: Option<u8>,
    female_only: bool,
}

fn descriptor(skin: u16) -> IntCompactDescr {
    IntCompactDescr {
        item_type: item_type::CREW_SKIN,
        nation: 1,
        id: skin,
    }
}

pub(super) fn load(
    fs: &ResourceBundle,
    prices: &mut ItemPrices,
) -> Result<BTreeMap<u16, CrewSkin>> {
    let groups = read_xml(fs, "scripts/item_defs/crewSkins/priceGroups.xml")?;
    let skins = read_xml(fs, "scripts/item_defs/crewSkins/crewSkins.xml")?;
    let groups: BTreeMap<_, _> = groups
        .iter_child_elements()
        .filter_map(|(_, group)| Some((group.get_child_string("name")?.trim(), group)))
        .collect();
    let mut result = BTreeMap::new();
    for (_, skin) in skins.iter_child_elements() {
        let id = u16::try_from(
            skin.get_child_integer("id")
                .context("Missing crew skin id")?,
        )?;
        let cd = descriptor(id);
        let group = skin
            .get_child_string("priceGroup")
            .context("Missing crew skin price group")?;
        prices.add(
            cd,
            groups
                .get(group.trim())
                .context("Unknown crew skin price group")?,
        );
        if skin.get_child_boolean("notInShop") == Some(true)
            || skin.get_child_string("realms").is_some_and(|realms| {
                !realms.trim().is_empty()
                    && !realms
                        .split_whitespace()
                        .any(|realm| realm.eq_ignore_ascii_case("eu"))
            })
        {
            prices.not_in_shop.push(cd);
        }
        let filters = skin.get_child_element("filters");
        let nation = filters
            .and_then(|filters| filters.get_child_string("nation"))
            .map(str::trim)
            .filter(|nation| !nation.is_empty())
            .map(|nation| {
                NATIONS
                    .iter()
                    .position(|&name| name == nation)
                    .map(|id| id as u8)
                    .context("Unknown crew skin nation")
            })
            .transpose()?;
        let female_only = filters
            .and_then(|filters| filters.get_child_string("sex"))
            .map(str::trim)
            == Some("female");
        result.insert(
            id,
            CrewSkin {
                nation,
                female_only,
            },
        );
    }
    Ok(result)
}

impl GarageCatalog {
    pub(super) fn equip_crew_skin(
        &self,
        snapshot: &mut AccountSnapshot,
        crew: &mut CrewRoster,
        member: i64,
        skin: Option<u16>,
    ) -> Result<(), ProgressionError> {
        let tankman = crew.active(member)?;
        self.check_crew_seat(snapshot, tankman, false)?;
        if tankman.skin == skin {
            return Ok(());
        }
        if let Some(skin) = skin {
            let def = self
                .crew_skins
                .get(&skin)
                .ok_or(ProgressionError::InvalidRequest)?;
            if def.female_only || def.nation.is_some_and(|nation| nation != tankman.nation) {
                return Err(ProgressionError::InvalidRequest);
            }
            let id = descriptor(skin).to_int() as i32;
            let stock = snapshot
                .progression
                .depot
                .get_mut(&id)
                .ok_or(ProgressionError::InsufficientDepotStock)?;
            *stock = stock
                .checked_sub(1)
                .ok_or(ProgressionError::InsufficientDepotStock)?;
            if *stock == 0 {
                snapshot.progression.depot.remove(&id);
            }
        }
        Self::return_crew_skin(snapshot, crew, member)?;
        crew.active_mut(member)?.skin = skin;
        Ok(())
    }

    pub(super) fn return_crew_skin(
        snapshot: &mut AccountSnapshot,
        crew: &mut CrewRoster,
        member: i64,
    ) -> Result<(), ProgressionError> {
        if let Some(skin) = crew.active_mut(member)?.skin.take() {
            let stock = snapshot
                .progression
                .depot
                .entry(descriptor(skin).to_int() as i32)
                .or_default();
            *stock = stock
                .checked_add(1)
                .ok_or(ProgressionError::InvalidRequest)?;
        }
        Ok(())
    }
}

pub(super) fn inventory(crew: &CrewRoster, depot: &BTreeMap<i32, u32>) -> Value {
    let stock = depot
        .iter()
        .filter_map(|(&id, &count)| {
            let cd = IntCompactDescr::from_int(i64::from(id));
            (cd.item_type == item_type::CREW_SKIN)
                .then_some((Value::from(i32::from(cd.id)), Value::from(count)))
        })
        .collect();
    let outfits = crew
        .members
        .iter()
        .filter_map(|(&id, member)| Some((Value::from(id), Value::from(i32::from(member.skin?)))))
        .collect();
    crate::pickle!({1: (Value::Dict(stock)), 2: (Value::Dict(outfits))})
}
