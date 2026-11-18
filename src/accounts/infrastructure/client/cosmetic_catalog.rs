use super::{
    GarageCatalog, item_defs,
    item_prices::{ItemPrices, Price},
    items::{IntCompactDescr, item_type},
    module_defs::ExteriorSlots,
    vehicle_defs::VehicleOffer,
};
use crate::{
    accounts::{
        AccountSnapshot, CosmeticChange, CosmeticItem, CosmeticKind, CosmeticPlacement,
        CosmeticRule, InscriptionSlot, ProgressionError, ProjectionDecalSlot, VehicleCost,
        VehicleSurface, change_cosmetics,
    },
    platform::client_formats::pxml::Element,
};
use anyhow::{Context, Result, bail, ensure};
use std::collections::{BTreeMap, BTreeSet};
use wot_resources::ResourceBundle;

const CAMOUFLAGES: &str = "scripts/item_defs/customization/camouflages/list.xml";
const DECALS: &str = "scripts/item_defs/customization/decals/list.xml";
const NUMBERS: &str = "scripts/item_defs/customization/personal_numbers/list.xml";
const PROHIBITED_NUMBERS: &str =
    "scripts/item_defs/customization/personal_numbers/prohibitedNumbers.xml";
const PROJECTION_DECALS: &str = "scripts/item_defs/customization/projection_decals/list.xml";
const PROJECTION_DECAL_PROGRESSION: &str =
    "scripts/item_defs/customization/projection_decals/progression.xml";
const EFFECTS: &str = "scripts/item_defs/customization/modifications/list.xml";
const STYLES: &str = "scripts/item_defs/customization/styles/list.xml";
const STYLE_TYPE: u8 = 4;
const PRICE_GROUPS: &str = "scripts/item_defs/customization/priceGroups/list.xml";
pub(super) const CAMOUFLAGE_TYPE: u8 = 2;

#[derive(Default)]
pub(super) struct CosmeticCatalog {
    definitions: BTreeMap<CosmeticItem, CosmeticDefinition>,
    prohibited_numbers: BTreeSet<String>,
}

struct CosmeticDefinition {
    rule: CosmeticRule,
    available_for_custom_outfits: bool,
    include: Vec<VehicleFilter>,
    exclude: Vec<VehicleFilter>,
}

#[derive(Default)]
struct VehicleFilter {
    nations: BTreeSet<String>,
    levels: BTreeSet<i64>,
    vehicles: BTreeSet<String>,
    tags: BTreeSet<String>,
}

impl VehicleFilter {
    fn read(node: &Element) -> Result<Self> {
        let words = |key| {
            node.get_child_string(key)
                .unwrap_or_default()
                .split_whitespace()
        };
        Ok(Self {
            nations: words("nations").map(str::to_owned).collect(),
            levels: words("levels").map(str::parse).collect::<Result<_, _>>()?,
            vehicles: words("vehicles").map(str::to_owned).collect(),
            tags: words("tags").map(str::to_owned).collect(),
        })
    }

    fn matches(&self, key: &str, nation: &str, vehicle: &VehicleOffer) -> bool {
        (self.nations.is_empty() || self.nations.contains(nation))
            && (self.levels.is_empty()
                || vehicle
                    .level
                    .is_some_and(|level| self.levels.contains(&level)))
            && (self.vehicles.is_empty() || self.vehicles.contains(key))
            && (self.tags.is_empty()
                || (self.tags.len() < vehicle.tags.len() && self.tags.is_subset(&vehicle.tags)))
    }
}

pub(super) fn index_exterior_shop(fs: &ResourceBundle, prices: &mut ItemPrices) -> Result<()> {
    let base_items: BTreeSet<_> = prices
        .prices
        .iter()
        .map(|(descriptor, _)| *descriptor)
        .filter(|descriptor| descriptor.item_type == item_type::CUSTOMIZATION)
        .map(|descriptor| (descriptor.nation, descriptor.id))
        .collect();
    let mut hidden = BTreeSet::new();
    let mut gate_styles = BTreeSet::new();
    if fs.has_file(STYLES) {
        let styles = item_defs::read_xml(fs, STYLES)?;
        for (id, style, group) in cosmetic_definitions(&styles, "style") {
            let id = id?;
            let tags = style
                .get_child_string("tags")
                .or_else(|| group.get_child_string("tags"))
                .unwrap_or_default();
            let tags: BTreeSet<_> = tags.split_whitespace().collect();
            hidden.insert((STYLE_TYPE, id));
            // These styles are intrinsically invisible to the carousel but satisfy the
            // hangar button's price-table membership check.
            if tags.contains("hiddenInUI") && !tags.contains("c11n3D") {
                gate_styles.insert(id);
            }
        }
    }
    for extension in item_defs::resource_extensions(fs) {
        for (path, item, subtype) in [
            (STYLES, "style", STYLE_TYPE),
            (CAMOUFLAGES, "camouflage", CAMOUFLAGE_TYPE),
            (DECALS, "decal", 3),
            (NUMBERS, "personal_number", 9),
            (EFFECTS, "modification", 5),
            (PROJECTION_DECALS, "projection_decal", 7),
        ] {
            let path = format!("{extension}/{path}");
            if !fs.has_file(&path) {
                continue;
            }
            let definitions = item_defs::read_xml(fs, &path)?;
            for (id, _, _) in cosmetic_definitions(&definitions, item) {
                let id = id.with_context(|| path.clone())?;
                if !base_items.contains(&(subtype, id)) {
                    hidden.insert((subtype, id));
                }
            }
        }
    }
    prices.catalog_only.extend(
        gate_styles
            .into_iter()
            .map(|id| cosmetic_descriptor(STYLE_TYPE, id)),
    );
    prices.not_in_shop.extend(
        hidden
            .into_iter()
            .map(|(subtype, id)| cosmetic_descriptor(subtype, id)),
    );
    Ok(())
}

fn cosmetic_definitions<'a>(
    list: &'a Element,
    item_name: &'a str,
) -> impl Iterator<Item = (Result<u16>, &'a Element, &'a Element)> + 'a {
    list.iter_child_elements()
        .filter(|(name, _)| *name == "itemGroup")
        .flat_map(move |(_, group)| {
            group
                .iter_child_elements()
                .filter(move |(name, _)| *name == item_name)
                .map(move |(_, item)| {
                    let id = item
                        .get_child_integer("id")
                        .context("Missing cosmetic id")
                        .and_then(|id| u16::try_from(id).context("Invalid cosmetic id"));
                    (id, item, group)
                })
        })
}

impl CosmeticCatalog {
    pub(super) fn load(fs: &ResourceBundle, prices: &mut ItemPrices) -> Result<Self> {
        if !fs.has_file(CAMOUFLAGES) && !fs.has_file(PRICE_GROUPS) {
            tracing::warn!("Resource bundle predates camouflage extraction");
            return Ok(Self::default());
        }
        let groups = item_defs::read_xml(fs, PRICE_GROUPS)?;
        let groups: BTreeMap<_, _> = groups
            .iter_child_elements()
            .filter(|(key, _)| *key == "priceGroup")
            .filter_map(|(_, group)| Some((group.get_child_string("name")?, group)))
            .collect();
        let mut catalog = Self::default();
        let mut progressive_decals = BTreeSet::new();
        if fs.has_file(PROJECTION_DECALS) {
            let progression = item_defs::read_xml(fs, PROJECTION_DECAL_PROGRESSION)?;
            progressive_decals.extend(
                progression
                    .iter_child_elements()
                    .filter(|(name, _)| *name == "progress")
                    .filter_map(|(_, item)| u16::try_from(item.get_child_integer("id")?).ok()),
            );
        }
        for (path, item_name, kind) in [
            (CAMOUFLAGES, "camouflage", CosmeticKind::Camouflage),
            (DECALS, "decal", CosmeticKind::Inscription),
            (NUMBERS, "personal_number", CosmeticKind::PersonalNumber),
            (EFFECTS, "modification", CosmeticKind::Effect),
            (
                PROJECTION_DECALS,
                "projection_decal",
                CosmeticKind::ProjectionDecal,
            ),
        ] {
            if path != CAMOUFLAGES && !fs.has_file(path) {
                continue;
            }
            let list = item_defs::read_xml(fs, path)?;
            for (id, item, group) in cosmetic_definitions(&list, item_name) {
                let id = id?;
                let descriptor = cosmetic_descriptor(cosmetic_subtype(kind), id);
                let is_static_inscription = item
                    .get_child_string("type")
                    .or_else(|| group.get_child_string("type"))
                    .is_some_and(|kind| kind.eq_ignore_ascii_case("inscription"));
                if (kind == CosmeticKind::Inscription && !is_static_inscription)
                    || (kind == CosmeticKind::Camouflage && id == 1)
                    || (kind == CosmeticKind::ProjectionDecal && progressive_decals.contains(&id))
                {
                    prices.not_in_shop.push(descriptor);
                    continue;
                }
                let (definition, display_price) = read_definition(item, group, &groups, kind)
                    .with_context(|| format!("Read cosmetic {kind:?} {id}"))?;
                prices.prices.push((descriptor, display_price));
                if definition.rule.price.is_none() {
                    prices.not_in_shop.push(descriptor);
                }
                ensure!(
                    catalog.definitions.insert((kind, id), definition).is_none(),
                    "Duplicate cosmetic {kind:?} {id}"
                );
            }
        }
        if fs.has_file(NUMBERS) {
            let prohibited = item_defs::read_xml(fs, PROHIBITED_NUMBERS)?;
            catalog.prohibited_numbers = prohibited
                .get_child_string("ProhibitedNumbers")
                .unwrap_or_default()
                .split_whitespace()
                .map(str::to_owned)
                .collect();
        }
        Ok(catalog)
    }

    fn rules_for(
        &self,
        key: &str,
        nation: &str,
        vehicle: &VehicleOffer,
        exterior: &ExteriorSlots,
        supports_effect: bool,
        projection_slots: &BTreeMap<u16, ProjectionDecalSlot>,
    ) -> BTreeMap<CosmeticItem, CosmeticRule> {
        if ["lockOutfit", "lockExceptProgression", "event_battles"]
            .iter()
            .any(|tag| vehicle.tags.contains(*tag))
        {
            return BTreeMap::new();
        }
        self.definitions
            .iter()
            .filter_map(|(&id, definition)| {
                let matches = |filter: &VehicleFilter| filter.matches(key, nation, vehicle);
                if !definition.available_for_custom_outfits
                    || (!definition.include.is_empty() && !definition.include.iter().any(matches))
                    || definition.exclude.iter().any(matches)
                {
                    return None;
                }
                let mut rule = definition.rule.clone();
                let can_fit = match &mut rule.placement {
                    CosmeticPlacement::Camouflage { surfaces, .. } => {
                        surfaces.retain(|surface| exterior.camouflage.contains(surface));
                        !surfaces.is_empty()
                    }
                    CosmeticPlacement::Inscription { slots, .. } => {
                        slots.retain(|slot| exterior.inscriptions.contains(slot));
                        !slots.is_empty()
                    }
                    CosmeticPlacement::Effect => supports_effect,
                    CosmeticPlacement::ProjectionDecal { form, .. } => projection_slots
                        .values()
                        .any(|slot| slot.supports_form(form.as_deref())),
                };
                can_fit.then_some((id, rule))
            })
            .collect()
    }
}

pub(super) fn has_camouflage_area(part: &Element, area: &str) -> bool {
    part.get_child_value_at(&["customization", "customizableVehicleAreas", "camouflage"])
        .and_then(|value| value.as_string())
        .is_some_and(|areas| {
            areas
                .split_whitespace()
                .any(|name| name.eq_ignore_ascii_case(area))
        })
}

fn read_definition(
    item: &Element,
    group: &Element,
    groups: &BTreeMap<&str, &Element>,
    kind: CosmeticKind,
) -> Result<(CosmeticDefinition, Price)> {
    let text = |key| {
        item.get_child_string(key)
            .or_else(|| group.get_child_string(key))
    };
    let section = |key| {
        item.get_child_element(key)
            .or_else(|| group.get_child_element(key))
    };

    let price_definition = if item.get_child("price").is_some() {
        item
    } else {
        let group_name = text("priceGroup").context("Missing price group")?;
        groups
            .get(group_name)
            .copied()
            .with_context(|| format!("Unknown price group {group_name}"))?
    };
    let display_price = ItemPrices::definition_price(price_definition);
    let excluded = |node: &Element| item_defs::xml_flag(node, "notInShop");
    let restricted = text("requiredToken").is_some_and(|token| !token.is_empty())
        || text("tags").is_some_and(|tags| {
            tags.split_whitespace()
                .any(|tag| matches!(tag, "styleOnly" | "hiddenInUI" | "vehicleBound"))
        });
    let price = Some(display_price)
        .filter(|price| {
            price.amount > 0 && !restricted && !excluded(item) && !excluded(price_definition)
        })
        .and_then(|price| match price.currency {
            "gold" => Some(VehicleCost {
                gold: price.amount,
                credits: 0,
            }),
            "credits" => Some(VehicleCost {
                gold: 0,
                credits: price.amount,
            }),
            _ => None,
        });
    let seasons = text("season")
        .unwrap_or("ALL")
        .split_whitespace()
        .try_fold(BTreeSet::new(), |mut seasons, flag| {
            use crate::accounts::OutfitSeason::*;
            match flag.to_ascii_uppercase().as_str() {
                "ALL" => seasons.extend([Winter, Summer, Desert]),
                "WINTER" => {
                    seasons.insert(Winter);
                }
                "SUMMER" => {
                    seasons.insert(Summer);
                }
                "DESERT" => {
                    seasons.insert(Desert);
                }
                "EVENT" | "UNDEFINED" => {}
                _ => bail!("Unknown season flag {flag}"),
            }
            Ok(seasons)
        })?;
    let surfaces = text("compatibleParts")
        .unwrap_or("HULL TURRET GUN")
        .split_whitespace()
        .filter_map(|part| match part.to_ascii_uppercase().as_str() {
            "HULL" => Some(VehicleSurface::Hull),
            "TURRET" => Some(VehicleSurface::Turret),
            "GUN" => Some(VehicleSurface::Gun),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let palettes = section("palettes").map_or(0, |palettes| palettes.iter_child_elements().count());
    let pattern_sizes = text("scales").map_or(3, |scales| scales.split_whitespace().count());
    let max_owned = item
        .get_child_integer("maxNumber")
        .or_else(|| group.get_child_integer("maxNumber"))
        .map(u32::try_from)
        .transpose()?;
    let mut include = Vec::new();
    let mut exclude = Vec::new();
    if let Some(filter) = section("vehicleFilter") {
        for (kind, node) in filter.iter_child_elements() {
            match kind {
                "include" => include.push(VehicleFilter::read(node)?),
                "exclude" => exclude.push(VehicleFilter::read(node)?),
                _ => bail!("Unknown vehicle filter rule {kind}"),
            }
        }
    }
    let (placement, usable) = match kind {
        CosmeticKind::Camouflage => {
            let usable = palettes > 0 && pattern_sizes > 0 && !surfaces.is_empty();
            (
                CosmeticPlacement::Camouflage {
                    surfaces,
                    palettes,
                    pattern_sizes,
                },
                usable,
            )
        }
        CosmeticKind::Effect => (CosmeticPlacement::Effect, true),
        CosmeticKind::ProjectionDecal => {
            let tags = text("tags").unwrap_or_default();
            let has_tag = |wanted| tags.split_whitespace().any(|tag| tag == wanted);
            let form = tags
                .split_whitespace()
                .find(|tag| tag.starts_with("formfactor_"))
                .map(str::to_owned);
            let horizontal_mirror = if item.get_child("mirror").is_some() {
                item_defs::xml_flag(item, "mirror")
            } else {
                item_defs::xml_flag(group, "mirror")
            };
            (
                CosmeticPlacement::ProjectionDecal {
                    form,
                    horizontal_mirror,
                    vertical_mirror: !has_tag("disableVerticalMirror"),
                    only_vertical_mirror: has_tag("onlyVerticalMirror"),
                },
                true,
            )
        }
        CosmeticKind::Inscription | CosmeticKind::PersonalNumber => {
            use InscriptionSlot::*;
            let digits = (kind == CosmeticKind::PersonalNumber)
                .then(|| {
                    item.get_child_integer("digitsCount")
                        .or_else(|| group.get_child_integer("digitsCount"))
                        .unwrap_or(3)
                })
                .map(usize::try_from)
                .transpose()?;
            (
                CosmeticPlacement::Inscription {
                    slots: BTreeSet::from([
                        HullFirst,
                        HullSecond,
                        TurretFirst,
                        TurretSecond,
                        GunFirst,
                        GunSecond,
                    ]),
                    digits,
                },
                digits.is_none_or(|digits| (1..=8).contains(&digits)),
            )
        }
    };
    let rule = CosmeticRule {
        price: price.filter(|_| usable && !seasons.is_empty()),
        seasons,
        placement,
        max_owned,
    };
    Ok((
        CosmeticDefinition {
            rule,
            available_for_custom_outfits: !restricted && usable,
            include,
            exclude,
        },
        display_price,
    ))
}

fn cosmetic_descriptor(subtype: u8, id: u16) -> IntCompactDescr {
    IntCompactDescr {
        id,
        nation: subtype,
        item_type: item_type::CUSTOMIZATION,
    }
}

pub(super) fn cosmetic_subtype(kind: CosmeticKind) -> u8 {
    match kind {
        CosmeticKind::Camouflage => CAMOUFLAGE_TYPE,
        CosmeticKind::Inscription => 3,
        CosmeticKind::PersonalNumber => 9,
        CosmeticKind::Effect => 5,
        CosmeticKind::ProjectionDecal => 7,
    }
}

pub fn cosmetic_item(descriptor: i32) -> Option<CosmeticItem> {
    let decoded = IntCompactDescr::from_int(descriptor.into());
    let kind = match decoded.nation {
        CAMOUFLAGE_TYPE => CosmeticKind::Camouflage,
        3 => CosmeticKind::Inscription,
        9 => CosmeticKind::PersonalNumber,
        5 => CosmeticKind::Effect,
        7 => CosmeticKind::ProjectionDecal,
        _ => return None,
    };
    (cosmetic_descriptor(cosmetic_subtype(kind), decoded.id).to_int() == i64::from(descriptor))
        .then_some((kind, decoded.id))
}

impl GarageCatalog {
    pub fn default_vehicle_emblem(
        &self,
        snapshot: &AccountSnapshot,
        inventory_id: i64,
    ) -> Option<u16> {
        self.owned_vehicle_template(snapshot, inventory_id)
            .ok()?
            .default_emblem
    }

    pub(super) fn change_cosmetics(
        &self,
        snapshot: &mut AccountSnapshot,
        inventory_id: i64,
        change: CosmeticChange,
    ) -> Result<bool, ProgressionError> {
        if inventory_id == 0 && matches!(change, CosmeticChange::Buy(_)) {
            let rules = self
                .cosmetics
                .definitions
                .iter()
                .map(|(&id, def)| (id, def.rule.clone()))
                .collect();
            return change_cosmetics(
                snapshot,
                inventory_id,
                change,
                &rules,
                &self.cosmetics.prohibited_numbers,
                &BTreeMap::new(),
            );
        }
        let owned = snapshot
            .vehicles
            .iter()
            .find(|vehicle| vehicle.inventory_id == inventory_id)
            .ok_or(ProgressionError::VehicleNotFound)?;
        let vehicle = self
            .vehicle(&owned.catalog_key)
            .map_err(Self::catalog_failure)?;
        let modules = vehicle.modules.rules.loadout(snapshot, inventory_id);
        let exterior = vehicle
            .modules
            .exterior_slots(&modules)
            .ok_or(ProgressionError::InvalidCosmetic)?;
        let offer = self
            .offers
            .get(&vehicle.modules.rules.vehicle)
            .ok_or(ProgressionError::VehicleNotFound)?;
        let projection_slots = vehicle.modules.projection_slots(&modules, exterior);
        let rules = self.cosmetics.rules_for(
            &owned.catalog_key,
            &vehicle.customization_nation,
            offer,
            exterior,
            vehicle.modules.supports_effect(&modules, exterior),
            &projection_slots,
        );
        change_cosmetics(
            snapshot,
            inventory_id,
            change,
            &rules,
            &self.cosmetics.prohibited_numbers,
            &projection_slots,
        )
    }
}
