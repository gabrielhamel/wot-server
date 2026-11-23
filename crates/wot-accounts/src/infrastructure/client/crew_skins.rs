use super::{
    GarageCatalog,
    item_defs::{NATIONS, read_xml},
    item_prices::ItemPrices,
    items::{IntCompactDescr, item_type},
};
use crate::{AccountSnapshot, CrewRoster, ProgressionError};
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use wot_platform::client_formats::pickle::Value;
use wot_resources::ResourceBundle;

pub(super) struct CrewSkin {
    nation: Option<u8>,
    female_only: bool,
    max_owned: Option<u32>,
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
                max_owned: skin
                    .get_child_integer("maxCount")
                    .map(u32::try_from)
                    .transpose()?,
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
    wot_platform::pickle!({1: (Value::Dict(stock)), 2: (Value::Dict(outfits))})
}

// The EU Store catalog in docs/store-eu-catalog.md lists a gold price for these skins.
// The native garage uses the extracted credit price groups.
const STORE_SKINS: &[(u16, &str)] = &[
    (2, "Pavel Rybalko"),
    (3, "Mikhail Katukov"),
    (4, "George Patton"),
    (7, "Charles de Gaulle"),
    (101, "Vasily Chapaev"),
    (102, "Oksana Shevchenko"),
    (103, "Oliver Murdoc"),
    (104, "Gabriel Lyman"),
    (105, "Emran Shankar"),
    (106, "Chester O’Brien"),
    (107, "Conrad Vitmaer"),
    (108, "Wilhelm Berghoff"),
    (109, "Christophe Lambert"),
    (110, "Audrey Dupont"),
    (111, "Hideo Tanaka"),
    (112, "Josef Švejk"),
    (113, "Tzinedi Tzidane"),
    (114, "Grzegorz Brzęczyszczykiewicz"),
    (115, "Björn Ragnarsson"),
    (116, "Alessandro Lotti"),
    (120, "Hellu Van Oob"),
    (121, "Kannot Gitgud"),
    (122, "Toma Teu"),
    (123, "Uni Corn"),
    (124, "Arengee Penetrael"),
];

pub(super) fn store_offers(
    fs: &ResourceBundle,
    prices: &ItemPrices,
) -> Result<Vec<super::StoreOffer>> {
    use super::store::{StoreCategory, StorePrice, listing};
    let names = super::localization::localized_names(fs, "text/lc_messages/crew_skins.mo")
        .unwrap_or_default();
    let skins = read_xml(fs, "scripts/item_defs/crewSkins/crewSkins.xml")?;
    let offers: BTreeMap<_, _> = STORE_SKINS.iter().copied().collect();
    let mut result = Vec::new();
    for (_, skin) in skins.iter_child_elements() {
        let Some(id) = skin.get_child_integer("id") else {
            continue;
        };
        let id = u16::try_from(id)?;
        let cd = descriptor(id);
        let Some(&fallback) = offers
            .get(&id)
            .filter(|_| !prices.not_in_shop.contains(&cd))
        else {
            continue;
        };
        let name = |field| {
            skin.get_child_string(field)
                .and_then(|label| label.strip_prefix("#crew_skins:"))
                .and_then(|label| names.get(label))
        };
        let name = name("firstName").zip(name("lastName")).map_or_else(
            || fallback.to_owned(),
            |(first, last)| format!("{first} {last}"),
        );
        result.push(listing(
            i32::try_from(cd.to_int())?,
            name,
            StoreCategory::CrewSkins,
            StorePrice::Gold(200),
        ));
    }
    Ok(result)
}

impl GarageCatalog {
    #[tracing::instrument(name = "accounts.crew_skins.check_purchase_limit", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = snapshot.id, input.item = item, input.count = count), err)]
    pub(super) fn check_crew_skin_limit(
        &self,
        snapshot: &AccountSnapshot,
        item: i32,
        count: u32,
    ) -> Result<(), ProgressionError> {
        let id = IntCompactDescr::from_int(i64::from(item)).id;
        let skin = self
            .crew_skins
            .get(&id)
            .ok_or(ProgressionError::InvalidRequest)?;
        if let Some(limit) = skin.max_owned {
            let depot = snapshot
                .progression
                .depot
                .get(&item)
                .copied()
                .unwrap_or_default();
            let roster = self.crew_roster(snapshot).map_err(Self::catalog_failure)?;
            let equipped = roster
                .members
                .values()
                .filter(|member| member.skin == Some(id))
                .count();
            let owned = u64::from(depot) + u64::from(count) + equipped as u64;
            if owned > u64::from(limit) {
                return Err(ProgressionError::InvalidRequest);
            }
        }
        Ok(())
    }
}
