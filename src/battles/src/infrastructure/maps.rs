use anyhow::{Context, Result};
use glam::Vec2;
use std::{collections::HashMap, io::BufReader};
use wot_platform::client_formats::pxml;
use wot_resources::ResourceBundle;
use wot_shared_kernel::ArenaSeason;

#[derive(Clone)]
pub(super) struct ArenaMap {
    pub(super) geometry: String,
    pub(super) season: ArenaSeason,
    pub(super) name: String,
    pub(super) spawns: [Vec2; 2],
    pub(super) bounds: [Vec2; 2],
}

impl ArenaMap {
    pub(super) fn from_definition(definition: &pxml::Element, gameplay: &str) -> Option<Self> {
        let geometry = definition.get_child_string("geometry")?;
        let variant = definition
            .get_child_element("gameplayTypes")?
            .get_child_element(gameplay)?;
        let spawn = |team| {
            variant
                .get_child_value_at(&["teamSpawnPoints", team, "position"])
                .or_else(|| variant.get_child_value_at(&["teamBasePositions", team, "position1"]))
                .and_then(pxml::Value::as_vec2)
                .filter(|point| point.is_finite())
        };
        let corner = |name| {
            definition
                .get_child_value_at(&["boundingBox", name])
                .and_then(pxml::Value::as_vec2)
                .filter(|point| point.is_finite())
        };
        let bounds = [corner("bottomLeft")?, corner("upperRight")?];
        if !bounds[0].cmplt(bounds[1]).all() {
            return None;
        }
        Some(Self {
            bounds,
            season: match definition.get_child_string("vehicleCamouflageKind") {
                Some("winter") => ArenaSeason::Winter,
                Some("desert") => ArenaSeason::Desert,
                _ => ArenaSeason::Summer,
            },
            geometry: geometry.to_owned(),
            name: String::new(),
            spawns: [spawn("team1")?, spawn("team2")?],
        })
    }
}

pub(super) fn load(resources: &ResourceBundle) -> Result<HashMap<i32, Option<ArenaMap>>> {
    const LIST: &str = "scripts/arena_defs/_list_.xml";
    if !resources.has_file(LIST) {
        return Ok(HashMap::new());
    }
    let list = pxml::from_reader(BufReader::new(resources.read(LIST)?))?;
    let defaults = pxml::from_reader(BufReader::new(
        resources.read("scripts/arena_defs/_default_.xml")?,
    ))?;
    let names = resources
        .read("text/lc_messages/arenas.mo")
        .ok()
        .and_then(wot_platform::client_formats::gettext::localized_names)
        .unwrap_or_default();
    let mut arenas = HashMap::new();
    for map in list
        .iter_children("map")
        .filter_map(pxml::Value::as_element)
    {
        if map.get_child_boolean("isHangar") == Some(true) {
            continue;
        }
        let Some(geometry) = map
            .get_child_integer("id")
            .and_then(|id| u16::try_from(id).ok())
        else {
            continue;
        };
        let name = map
            .get_child_string("name")
            .context("Arena entry has no name")?;
        let path = format!("scripts/arena_defs/{name}.xml");
        if !resources.has_file(&path) && map.get_child_boolean("isDevelopment") == Some(true) {
            continue;
        }
        let map = pxml::from_reader(BufReader::new(resources.read(&path)?))
            .with_context(|| path.clone())?;
        if let Some(gameplays) = map.get_child_element("gameplayTypes") {
            for (gameplay, id) in [
                ("ctf", 0),
                ("domination", 1),
                ("assault", 2),
                ("assault2", 6),
            ] {
                if let Some(variant) = gameplays.get_child(gameplay) {
                    let explicit = variant
                        .as_element()
                        .and_then(|data| data.get_child_boolean("explicitRequestOnly"))
                        .or_else(|| {
                            defaults
                                .get_child_element("gameplayTypes")?
                                .get_child_element(gameplay)?
                                .get_child_boolean("explicitRequestOnly")
                        });
                    if explicit != Some(true) {
                        arenas.insert(
                            (id << 16) | i32::from(geometry),
                            ArenaMap::from_definition(&map, gameplay).map(|mut arena| {
                                let label = map.get_child_string("name").unwrap_or(name);
                                let key = label.strip_prefix("#arenas:").unwrap_or(label);
                                let fallback = name.split_once('_').map_or(name, |(_, rest)| rest);
                                let label = names
                                    .get(key)
                                    .or_else(|| names.get(&format!("{name}/name")))
                                    .cloned()
                                    .unwrap_or_else(|| {
                                        fallback
                                            .split('_')
                                            .map(|word| {
                                                let mut letters = word.chars();
                                                letters.next().map_or_else(String::new, |first| {
                                                    first.to_uppercase().chain(letters).collect()
                                                })
                                            })
                                            .collect::<Vec<_>>()
                                            .join(" ")
                                    });
                                let mode = match gameplay {
                                    "domination" => "Encounter",
                                    "assault" => "Assault",
                                    "assault2" => "Assault (reversed)",
                                    _ => "Standard Battle",
                                };
                                arena.name = format!("{label} · {mode}");
                                arena
                            }),
                        );
                    }
                }
            }
        } else {
            arenas.insert(i32::from(geometry), None);
        }
    }
    Ok(arenas)
}
