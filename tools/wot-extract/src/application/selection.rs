use std::collections::BTreeSet;
use wot_resources::NATIONS;

const ABI_FILES: &[&str] = &[
    "scripts/entities.xml",
    "scripts/components.xml",
    "scripts/entity_defs/alias.xml",
    "scripts/entity_defs/interfaces/AccountVersion.def",
];

const GARAGE_FILES: &[&str] = &[
    "scripts/item_defs/customization/camouflages/list.xml",
    "scripts/item_defs/customization/paints/list.xml",
    "scripts/item_defs/customization/attachments/list.xml",
    "scripts/item_defs/customization/stat_trackers/list.xml",
    "scripts/item_defs/customization/styles/list.xml",
    "scripts/item_defs/customization/decals/list.xml",
    "scripts/item_defs/customization/modifications/list.xml",
    "scripts/item_defs/customization/projection_decals/list.xml",
    "scripts/item_defs/customization/projection_decals/progression.xml",
    "scripts/item_defs/customization/personal_numbers/list.xml",
    "scripts/item_defs/customization/personal_numbers/prohibitedNumbers.xml",
    "scripts/item_defs/customization/priceGroups/list.xml",
    "scripts/item_defs/crewBooks/crewBookTypes.xml",
    "scripts/item_defs/crewBooks/priceGroups.xml",
    "scripts/item_defs/crewBooks/crewBooks.xml",
    "scripts/item_defs/crewSkins/priceGroups.xml",
    "scripts/item_defs/crewSkins/crewSkins.xml",
    "scripts/item_defs/vehicles/common/equipments.xml",
    "scripts/item_defs/vehicles/common/optional_devices.xml",
    "scripts/item_defs/vehicles/common/supply_slot_types.xml",
    "scripts/item_defs/vehicles/common/post_progression/trees.xml",
];

const BATTLE_EFFECT_FILES: &[&str] = &[
    "scripts/item_defs/vehicles/common/shot_effects.xml",
    "scripts/item_defs/vehicles/common/damage_stickers.xml",
    "scripts/item_defs/vehicles/common/prefab_effects/shot_effects.xml",
    "scripts/item_defs/vehicles/common/vehicle.xml",
    "system/data/material_kinds.xml",
];

const VEHICLE_COMPONENTS: &[&str] = &[
    "chassis.xml",
    "engines.xml",
    "fuelTanks.xml",
    "guns.xml",
    "radios.xml",
    "shells.xml",
    "turrets.xml",
];

pub(super) fn resource_root<'a>(
    path: &'a str,
    extensions: &BTreeSet<String>,
) -> Option<(&'a str, &'a str)> {
    let (first, rest) = path.split_once('/')?;
    if matches!(first, "scripts" | "gui" | "text" | "system") {
        Some(("", path))
    } else if extensions.contains(first) {
        Some((first, rest))
    } else {
        None
    }
}

pub(super) fn is_definition(root: &str, path: &str) -> bool {
    if !root.is_empty()
        && matches!(
            path,
            "scripts/item_defs/customization/styles/list.xml"
                | "scripts/item_defs/customization/camouflages/list.xml"
                | "scripts/item_defs/customization/paints/list.xml"
                | "scripts/item_defs/customization/attachments/list.xml"
                | "scripts/item_defs/customization/stat_trackers/list.xml"
                | "scripts/item_defs/customization/decals/list.xml"
                | "scripts/item_defs/customization/modifications/list.xml"
                | "scripts/item_defs/customization/projection_decals/list.xml"
                | "scripts/item_defs/customization/personal_numbers/list.xml"
        )
    {
        return true;
    }
    if path == "extension.xml" {
        return !root.is_empty();
    }
    if root.is_empty()
        && path
            .strip_prefix("scripts/arena_defs/")
            .and_then(|file| leaf(file, ".xml"))
            .is_some()
    {
        return true;
    }
    if ["scripts/entity_defs/", "scripts/component_defs/"]
        .into_iter()
        .filter_map(|prefix| path.strip_prefix(prefix))
        .any(|relative| {
            leaf(
                relative.strip_prefix("interfaces/").unwrap_or(relative),
                ".def",
            )
            .is_some()
        })
    {
        return true;
    }
    if let Some((nation, relative)) = path
        .strip_prefix("scripts/item_defs/vehicles/")
        .and_then(|path| path.split_once('/'))
        && NATIONS.contains(&nation)
    {
        return leaf(relative, ".xml").is_some()
            || relative
                .strip_prefix("components/")
                .is_some_and(|file| VEHICLE_COMPONENTS.contains(&file));
    }
    root.is_empty()
        && (ABI_FILES.contains(&path)
            || GARAGE_FILES.contains(&path)
            || BATTLE_EFFECT_FILES.contains(&path)
            || path == "scripts/item_defs/vehicles/common/forbidden_vehicles_to_battle_config.xml"
            || path == "scripts/item_defs/vehicles/common/vehicle_common.xml"
            || matches!(
                path,
                "text/lc_messages/vehicle_customization.mo"
                    | "text/lc_messages/artefacts.mo"
                    | "text/lc_messages/crew_skins.mo"
                    | "text/lc_messages/crew_books.mo"
                    | "text/lc_messages/nations.mo"
                    | "text/lc_messages/gb_vehicles.mo"
                    | "text/lc_messages/gb_crew.mo"
            )
            || NATIONS.iter().any(|nation| {
                path == format!("scripts/item_defs/tankmen/{nation}.xml")
                    || path == format!("gui/flash/techtree/{nation}-tree.xml")
                    || path == format!("text/lc_messages/{nation}_vehicles.mo")
                    || path == format!("text/lc_messages/{nation}_crew.mo")
            }))
}

pub(super) fn client_class(path: &str) -> Option<&str> {
    leaf(path.strip_prefix("scripts/client/")?, ".pyc")
}

pub(super) fn component_name(path: &str) -> Option<&str> {
    let path = if path.starts_with("scripts/") {
        path
    } else {
        path.split_once('/')?.1
    };
    leaf(path.strip_prefix("scripts/component_defs/")?, ".def")
}

fn leaf<'a>(path: &'a str, suffix: &str) -> Option<&'a str> {
    let name = path.strip_suffix(suffix)?;
    (!name.is_empty() && !name.contains('/')).then_some(name)
}

pub(super) fn required_files() -> impl Iterator<Item = String> {
    ABI_FILES
        .iter()
        .chain(GARAGE_FILES)
        .chain(BATTLE_EFFECT_FILES)
        .map(|path| (*path).to_owned())
        .chain(NATIONS.iter().flat_map(|nation| {
            [
                format!("scripts/item_defs/tankmen/{nation}.xml"),
                format!("gui/flash/techtree/{nation}-tree.xml"),
                format!("scripts/item_defs/vehicles/{nation}/list.xml"),
            ]
            .into_iter()
            .chain(
                VEHICLE_COMPONENTS.iter().map(move |file| {
                    format!("scripts/item_defs/vehicles/{nation}/components/{file}")
                }),
            )
        }))
}
