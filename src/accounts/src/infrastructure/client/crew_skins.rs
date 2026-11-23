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
    pub(super) name: String,
    pub(super) eu: bool,
    nation: Option<u8>,
    female: Option<bool>,
    pub(super) max_owned: Option<u32>,
}

pub(super) fn descriptor(skin: u16) -> IntCompactDescr {
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
    let names = super::localization::localized_names(fs, "text/lc_messages/crew_skins.mo")
        .unwrap_or_default();
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
        let eu = skin.get_child_string("realms").is_none_or(|realms| {
            realms.trim().is_empty()
                || realms
                    .split_whitespace()
                    .any(|realm| realm.eq_ignore_ascii_case("eu"))
        });
        if super::item_defs::xml_flag(skin, "notInShop") || !eu {
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
        let female = match filters
            .and_then(|filters| filters.get_child_string("sex"))
            .map(str::trim)
        {
            Some("female") => Some(true),
            Some("male") => Some(false),
            _ => None,
        };
        result.insert(
            id,
            CrewSkin {
                name: display_name(skin, id, &names),
                eu,
                nation,
                female,
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
            if self.recruit_passport_locked(tankman) {
                return Err(ProgressionError::InvalidRequest);
            }
            let def = self
                .crew_skins
                .get(&skin)
                .ok_or(ProgressionError::InvalidRequest)?;
            if !def.eu
                || def.female.is_some_and(|female| female != tankman.female)
                || def.nation.is_some_and(|nation| nation != tankman.nation)
            {
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

fn display_name(
    skin: &wot_platform::client_formats::pxml::Element,
    id: u16,
    names: &BTreeMap<String, String>,
) -> String {
    let localized = ["firstName", "lastName"]
        .into_iter()
        .filter_map(|field| skin.get_child_string(field)?.strip_prefix("#crew_skins:"))
        .filter_map(|key| names.get(key).map(String::as_str))
        .map(str::trim)
        .filter(|name| !name.is_empty() && *name != "?empty?")
        .collect::<Vec<_>>()
        .join(" ");
    if !localized.is_empty() {
        return localized;
    }
    if let Some((_, name)) = STORE_SKINS.iter().find(|(key, _)| *key == id) {
        return (*name).to_owned();
    }
    skin.get_child_string("icon")
        .map(super::localization::humanize_item_name)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Unnamed crew skin".to_owned())
}

pub(super) fn store_offers(
    skins: &BTreeMap<u16, CrewSkin>,
    prices: &ItemPrices,
) -> Vec<super::StoreOffer> {
    use super::store::{StoreCategory, StorePrice, listing};
    prices
        .prices
        .iter()
        .filter_map(|(cd, price)| {
            if cd.item_type != item_type::CREW_SKIN
                || prices.not_in_shop.contains(cd)
                || prices.not_for_sale.contains(cd)
            {
                return None;
            }
            let skin = skins.get(&cd.id)?;
            let price = if STORE_SKINS.iter().any(|(id, _)| *id == cd.id) {
                StorePrice::Gold(200)
            } else {
                if price.amount <= 0 {
                    return None;
                }
                match price.currency {
                    "credits" => StorePrice::Credits(price.amount),
                    "gold" => StorePrice::Gold(price.amount),
                    "crystal" => StorePrice::Bonds(price.amount),
                    _ => return None,
                }
            };
            Some(listing(
                cd.to_int() as i32,
                skin.name.clone(),
                StoreCategory::CrewSkins,
                price,
            ))
        })
        .collect()
}

impl GarageCatalog {
    pub(crate) fn crew_skin_counts(&self, snapshot: &crate::AccountSnapshot) -> BTreeMap<u16, u32> {
        let mut counts = BTreeMap::new();
        for (&item, &count) in &snapshot.progression.depot {
            let cd = IntCompactDescr::from_int(i64::from(item));
            if cd.item_type == item_type::CREW_SKIN && descriptor(cd.id) == cd {
                counts.insert(cd.id, count);
            }
        }
        if let Some(crew) = &snapshot.crew {
            for member in crew.members.values() {
                if let Some(id) = member.skin {
                    let count = counts.entry(id).or_insert(0);
                    *count = count.saturating_add(1);
                }
            }
        }
        counts
    }

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
