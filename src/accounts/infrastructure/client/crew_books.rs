use super::{
    GarageCatalog,
    item_defs::{NATIONS, read_xml},
    item_prices::ItemPrices,
    items::{IntCompactDescr, item_type},
    localization::localized_names,
};
use crate::accounts::{AccountSnapshot, CrewRoster, ProgressionError};
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use wot_resources::ResourceBundle;

pub(super) struct CrewBook {
    pub(super) name: String,
    xp: u32,
    nation: Option<u8>,
    personal: bool,
}

pub(super) fn load(
    fs: &ResourceBundle,
    prices: &mut ItemPrices,
) -> Result<BTreeMap<i32, CrewBook>> {
    let types = read_xml(fs, "scripts/item_defs/crewBooks/crewBookTypes.xml")?;
    let groups = read_xml(fs, "scripts/item_defs/crewBooks/priceGroups.xml")?;
    let books = read_xml(fs, "scripts/item_defs/crewBooks/crewBooks.xml")?;
    let types: BTreeMap<_, _> = types
        .iter_child_elements()
        .filter_map(|(_, item)| {
            Some((
                item.get_child_string("type")?.trim(),
                item.get_child_integer("exp")?,
            ))
        })
        .collect();
    let groups: BTreeMap<_, _> = groups
        .iter_child_elements()
        .filter_map(|(_, group)| Some((group.get_child_string("name")?.trim(), group)))
        .collect();
    let names = localized_names(fs, "text/lc_messages/crew_books.mo").unwrap_or_default();
    let nations = localized_names(fs, "text/lc_messages/nations.mo").unwrap_or_default();
    let mut result = BTreeMap::new();
    for (_, book) in books.iter_child_elements() {
        let cd = IntCompactDescr {
            item_type: item_type::CREW_BOOK,
            nation: 1,
            id: u16::try_from(
                book.get_child_integer("id")
                    .context("Missing crew book id")?,
            )?,
        };
        let kind = book
            .get_child_string("type")
            .context("Missing crew book type")?
            .trim();
        let xp = u32::try_from(*types.get(kind).context("Unknown crew book type")?)?;
        let group = book
            .get_child_string("priceGroup")
            .context("Missing crew book price group")?;
        prices.add(
            cd,
            groups
                .get(group.trim())
                .context("Unknown crew book price group")?,
        );
        if book.get_child_boolean("notInShop") == Some(true) {
            prices.not_in_shop.push(cd);
        }
        let nation = book
            .get_child_element("filters")
            .and_then(|filters| filters.get_child_string("nation"))
            .map(|nation| {
                NATIONS
                    .iter()
                    .position(|&name| name == nation.trim())
                    .map(|id| id as u8)
                    .context("Unknown crew book nation")
            })
            .transpose()?;
        let name = book
            .get_child_string("name")
            .and_then(|name| name.trim().strip_prefix("#crew_books:"))
            .and_then(|key| names.get(key))
            .cloned()
            .unwrap_or_else(|| match kind {
                "brochure" => "Training Booklet".to_owned(),
                "guide" => "Training Guide".to_owned(),
                "crewBook" => "Training Manual".to_owned(),
                "personalBook" => "Personal Training Manual".to_owned(),
                "universalBook" => "Universal Manual".to_owned(),
                "universalGuide" => "Universal Guide".to_owned(),
                "universalBrochure" => "Universal Booklet".to_owned(),
                _ => format!("Crew book · {xp} XP"),
            });
        let name = if let Some(key) = nation.and_then(|id| NATIONS.get(usize::from(id))) {
            let nation = nations.get(*key).map(String::as_str).unwrap_or(match *key {
                "ussr" => "U.S.S.R.",
                "usa" => "U.S.A.",
                "uk" => "U.K.",
                "czech" => "Czechoslovakia",
                "germany" => "Germany",
                "china" => "China",
                "france" => "France",
                "japan" => "Japan",
                "sweden" => "Sweden",
                "poland" => "Poland",
                "italy" => "Italy",
                _ => "Unknown nation",
            });
            if name.contains("%(nation)s") {
                name.replace("%(nation)s", nation)
            } else {
                format!("{name} ({nation})")
            }
        } else {
            name
        };
        result.insert(
            cd.to_int() as i32,
            CrewBook {
                name: name.replace('\u{a0}', " "),
                xp,
                nation,
                personal: kind == "personalBook",
            },
        );
    }
    Ok(result)
}

impl GarageCatalog {
    pub(super) fn conversion_books(
        &self,
        nation: u8,
        mut xp: u32,
    ) -> Result<Vec<(i32, u32)>, ProgressionError> {
        if xp == 0 {
            return Ok(Vec::new());
        }
        [250_000, 100_000, 20_000]
            .into_iter()
            .map(|amount| {
                let (&book, _) = self
                    .crew_books
                    .iter()
                    .find(|(_, book)| {
                        book.nation == Some(nation) && !book.personal && book.xp == amount
                    })
                    .ok_or(ProgressionError::CatalogUnavailable)?;
                let count = xp / (amount + 1) + u32::from(amount == 20_000);
                xp %= amount + 1;
                Ok((book, count))
            })
            .collect()
    }

    pub(super) fn use_crew_book(
        &self,
        snapshot: &mut AccountSnapshot,
        crew: &mut CrewRoster,
        book: i32,
        count: u32,
        vehicle: Option<i64>,
        member: Option<i64>,
    ) -> Result<(), ProgressionError> {
        let definition = self
            .crew_books
            .get(&book)
            .ok_or(ProgressionError::InvalidRequest)?;
        let stock = snapshot
            .progression
            .depot
            .get(&book)
            .copied()
            .unwrap_or_default();
        let remaining = stock
            .checked_sub(count)
            .filter(|_| (1..=100).contains(&count))
            .ok_or(ProgressionError::InsufficientDepotStock)?;
        let xp = definition
            .xp
            .checked_mul(count)
            .ok_or(ProgressionError::InvalidRequest)?;
        if definition.personal {
            let member = crew.active_mut(member.ok_or(ProgressionError::InvalidRequest)?)?;
            self.check_crew_seat(snapshot, member, true)?;
            if member.training.xp_capacity() == 0 {
                return Err(ProgressionError::InvalidRequest);
            }
            member.training.add_xp(xp);
        } else {
            let id = vehicle.ok_or(ProgressionError::InvalidRequest)?;
            let vehicle = self.owned_crew_vehicle(snapshot, id)?;
            if vehicle.perks_locked
                || definition
                    .nation
                    .is_some_and(|nation| vehicle.nation != nation)
            {
                return Err(ProgressionError::InvalidRequest);
            }
            let seats = crew.seats(id, vehicle.roles.len());
            if seats.is_empty() {
                return Err(ProgressionError::InvalidRequest);
            }
            let mut applied = false;
            for member in seats {
                let member = crew.active_mut(member.ok_or(ProgressionError::InvalidRequest)?)?;
                let trained = member.specialization == vehicle.specialization
                    || vehicle.no_transfer_penalty
                    || (vehicle.premium
                        && vehicle.class_tag.is_some()
                        && vehicle.class_tag == self.specialization_vehicle(member)?.class_tag);
                if !trained || member.training.efficiency_xp < crate::accounts::MAX_EFFICIENCY_XP {
                    return Err(ProgressionError::InvalidRequest);
                }
                applied |= member.training.xp_capacity() != 0;
                member.training.add_xp(xp);
            }
            if !applied {
                return Err(ProgressionError::InvalidRequest);
            }
        }
        if remaining == 0 {
            snapshot.progression.depot.remove(&book);
        } else {
            snapshot.progression.depot.insert(book, remaining);
        }
        Ok(())
    }
}
