use super::{
    GarageCatalog,
    item_defs::{NATIONS, ROLES, read_xml},
    localization::{humanize_item_name, localized_names},
};
use crate::{AccountSnapshot, CrewMember, CrewPassport, GrantError};
use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;
use wot_platform::client_formats::pxml::Element;
use wot_resources::ResourceBundle;

#[derive(Serialize)]
pub struct SpecialRecruitGrantEntry {
    pub id: i32,
    pub name: String,
    pub countries: Vec<SpecialRecruitCountry>,
}

#[derive(Serialize)]
pub struct SpecialRecruitCountry {
    pub id: i32,
    pub name: String,
}

pub(super) struct SpecialRecruit {
    key: String,
    name: String,
    country_name: String,
    nation: u8,
    group: u16,
    passport: CrewPassport,
    roles: Vec<u8>,
    passport_locked: bool,
    female: bool,
}

pub(super) fn recruit_id(nation: u8, group: u16) -> i32 {
    (i32::from(nation) << 16) | i32::from(group)
}

pub(super) fn load(fs: &ResourceBundle) -> Result<BTreeMap<i32, SpecialRecruit>> {
    let names: BTreeMap<_, _> = NATIONS
        .iter()
        .filter_map(|&nation| {
            let prefix = if nation == "uk" { "gb" } else { nation };
            let key = format!("{prefix}_crew");
            localized_names(fs, &format!("text/lc_messages/{key}.mo")).map(|names| (key, names))
        })
        .collect();
    let countries = localized_names(fs, "text/lc_messages/nations.mo").unwrap_or_default();
    let mut recruits = BTreeMap::new();
    for (nation, &country) in NATIONS.iter().enumerate() {
        let country_name = countries
            .get(country)
            .cloned()
            .unwrap_or_else(|| humanize_item_name(country));
        let config = read_xml(fs, &format!("scripts/item_defs/tankmen/{country}.xml"))?;
        let Some(groups) = config.get_child_element("premiumGroups") else {
            continue;
        };
        for (key, group) in groups.iter_child_elements() {
            let Some(id) = group
                .get_child_integer("groupID")
                .and_then(|id| u16::try_from(id).ok())
            else {
                continue;
            };
            let Some((first_name, first_label)) = passport_part(group, "firstNames") else {
                continue;
            };
            let Some((last_name, last_label)) = passport_part(group, "lastNames") else {
                continue;
            };
            let Some((portrait, _)) = passport_part(group, "icons") else {
                continue;
            };
            let localized = [first_label, last_label]
                .into_iter()
                .filter_map(|label| {
                    let (domain, key) = label.strip_prefix('#')?.split_once(':')?;
                    names.get(domain)?.get(key).map(String::as_str)
                })
                .map(str::trim)
                .filter(|name| !name.is_empty() && *name != "?empty?")
                .collect::<Vec<_>>()
                .join(" ");
            let name = match key {
                "men1" => "Male recruit".to_owned(),
                "women1" => "Female recruit".to_owned(),
                _ if localized.is_empty() => humanize_item_name(key),
                _ => localized,
            };
            let roles = group
                .get_child_string("roles")
                .unwrap_or_default()
                .split_whitespace()
                .filter_map(|role| {
                    ROLES
                        .iter()
                        .find_map(|(key, id)| (*key == role).then_some(*id))
                })
                .collect::<Vec<_>>();
            let passport_locked = group
                .get_child_string("tags")
                .unwrap_or_default()
                .split_whitespace()
                .any(|tag| tag == "passportReplacementForbidden");
            let nation = u8::try_from(nation)?;
            recruits.insert(
                recruit_id(nation, id),
                SpecialRecruit {
                    key: key.to_owned(),
                    name,
                    country_name: country_name.clone(),
                    nation,
                    group: id,
                    passport: CrewPassport {
                        first_name,
                        last_name,
                        portrait,
                    },
                    roles,
                    passport_locked,
                    female: group
                        .get_child_string("sex")
                        .is_some_and(|sex| sex.trim() == "female"),
                },
            );
        }
    }
    Ok(recruits)
}

fn passport_part<'a>(group: &'a Element, section: &str) -> Option<(u16, &'a str)> {
    group
        .get_child_element(section)?
        .iter_children_all()
        .find_map(|(key, value)| Some((key.strip_prefix('_')?.parse().ok()?, value.as_string()?)))
}

impl GarageCatalog {
    pub fn grantable_special_recruits(&self) -> Vec<SpecialRecruitGrantEntry> {
        let mut entries = BTreeMap::new();
        for (&id, recruit) in &self.special_recruits {
            entries
                .entry(recruit.key.as_str())
                .or_insert_with(|| SpecialRecruitGrantEntry {
                    id,
                    name: recruit.name.clone(),
                    countries: Vec::new(),
                })
                .countries
                .push(SpecialRecruitCountry {
                    id,
                    name: recruit.country_name.clone(),
                });
        }
        let mut entries: Vec<_> = entries.into_values().collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
        for entry in &mut entries {
            entry.countries.sort_by(|a, b| a.name.cmp(&b.name));
        }
        for group in entries
            .chunk_by_mut(|a, b| a.name == b.name)
            .filter(|group| group.len() > 1)
        {
            for (index, entry) in group.iter_mut().enumerate() {
                entry.name = format!("{} · Variant {}", entry.name, index + 1);
            }
        }
        entries
    }

    pub(crate) fn special_recruit_counts(&self, snapshot: &AccountSnapshot) -> BTreeMap<i32, u32> {
        snapshot
            .crew
            .iter()
            .flat_map(|crew| crew.members.values())
            .filter_map(|member| Some((recruit_id(member.nation, member.special_recruit?), 1)))
            .collect()
    }

    pub(super) fn recruit_allows_role(&self, member: &CrewMember, role: u8) -> bool {
        member
            .special_recruit
            .and_then(|group| self.special_recruits.get(&recruit_id(member.nation, group)))
            .is_none_or(|recruit| recruit.roles.is_empty() || recruit.roles.contains(&role))
    }

    pub(super) fn recruit_passport_locked(&self, member: &CrewMember) -> bool {
        member
            .special_recruit
            .and_then(|group| self.special_recruits.get(&recruit_id(member.nation, group)))
            .is_some_and(|recruit| recruit.passport_locked)
    }

    #[tracing::instrument(name = "accounts.grants.add_special_recruit", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = snapshot.id, input.item = id, output.member = tracing::field::Empty), err)]
    pub(super) fn add_special_recruit(
        &self,
        snapshot: &mut AccountSnapshot,
        id: i32,
    ) -> Result<i64, GrantError> {
        let recruit = self
            .special_recruits
            .get(&id)
            .ok_or(GrantError::UnknownSpecialRecruit)?;
        let nation = NATIONS
            .get(usize::from(recruit.nation))
            .ok_or(GrantError::UnknownSpecialRecruit)?;
        let mut crew = self.crew_roster(snapshot).map_err(Self::catalog_failure)?;
        if crew.members.values().any(|member| {
            member.nation == recruit.nation && member.special_recruit == Some(recruit.group)
        }) {
            return Err(GrantError::SpecialRecruitOwned);
        }
        let preferred_role = recruit.roles.first().copied().unwrap_or(1);
        let vehicles = snapshot
            .vehicles
            .iter()
            .filter_map(|vehicle| self.keys.get(&vehicle.catalog_key))
            .chain(self.tech_tree.iter())
            .chain(self.offers.keys());
        let vehicle = vehicles
            .filter_map(|id| self.offers.get(id).map(|offer| (id, offer)))
            .filter(|(_, offer)| {
                offer
                    .key
                    .split_once(':')
                    .is_some_and(|(country, _)| country == *nation)
                    && offer.grantable()
                    && !offer.tags.contains("lockCrew")
            })
            .filter_map(|(id, _)| self.crew_vehicle(*id).ok())
            .find(|vehicle| {
                vehicle
                    .roles
                    .iter()
                    .any(|roles| roles.first() == Some(&preferred_role))
            })
            .ok_or(GrantError::SpecialRecruitVehicle)?;
        let mut member = self.new_crew_member(&vehicle, preferred_role, 0)?;
        member.passport = recruit.passport;
        member.special_recruit = Some(recruit.group);
        member.female = recruit.female;
        let member = crew.insert(member)?;
        crew.validate_berths(snapshot.balances.berths)?;
        snapshot.crew = Some(crew);
        tracing::Span::current().record("output.member", member);
        Ok(member)
    }
}
