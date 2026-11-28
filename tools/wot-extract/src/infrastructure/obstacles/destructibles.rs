use super::{ClientResources, foliage::BUSHES_PATH};
use anyhow::{Context, Result};
use std::collections::{BTreeMap, HashMap, HashSet};
use wot_platform::client_formats::pxml;

const FIRST_MODULE_KIND: u32 = 73;
const MAX_MODULES: usize = 13;

#[derive(Clone, Copy)]
pub(super) struct Fragment {
    pub part: u32,
    pub health: u32,
    pub cascade: u32,
}

pub(super) enum Model {
    Single(Fragment),
    Structure(HashMap<String, Fragment>),
}

impl Model {
    pub(super) fn fragment(&self, name: &str, material: u32, flags: u32) -> Option<Fragment> {
        match self {
            Self::Single(fragment) => Some(*fragment),
            Self::Structure(parts) => parts
                .get(material_name(name))
                .or_else(|| {
                    let kind = if flags & 8 != 0 {
                        material_name(name)
                            .strip_prefix("wall_")
                            .and_then(|index| index.parse::<u32>().ok())
                            .filter(|&index| index < MAX_MODULES as u32)
                            .map_or(material, |index| FIRST_MODULE_KIND + index)
                    } else {
                        material
                    };
                    parts.values().find(|fragment| fragment.part == kind)
                })
                .copied(),
        }
    }
}

#[derive(Default)]
pub(super) struct Catalog {
    models: HashMap<String, Model>,
    trees: HashMap<String, Tree>,
}

pub(super) struct Tree {
    pub health: u32,
    pub height: f32,
}

impl Catalog {
    pub(super) fn load(client: &mut ClientResources, bushes: &HashSet<String>) -> Result<Self> {
        const PATH: &str = "scripts/destructibles.xml";
        if !client.contains(PATH) {
            return Ok(Self::default());
        }
        let bytes = client.read_bounded(PATH, 16 * 1024 * 1024)?;
        let root = pxml::from_bytes(bytes).context("Read scenery destruction definitions")?;
        let mut result = Self {
            trees: trees(&root, bushes)?,
            ..Self::default()
        };
        for group in ["fragiles", "fallingAtoms", "structures"] {
            let Some(section) = root.get_child_element(group) else {
                continue;
            };
            for (_, item) in section.iter_child_elements() {
                let Some(model_path) = item
                    .get_child_string("filename")
                    .and_then(|name| name.trim().strip_suffix(".model"))
                else {
                    continue;
                };
                let model = if group == "structures" {
                    let Some(modules) = item.get_child_element("modules") else {
                        continue;
                    };
                    Model::Structure(structure(modules))
                } else {
                    let Some(health) = health(item) else { continue };
                    Model::Single(Fragment {
                        part: if group == "fallingAtoms" { 2 } else { 1 },
                        health,
                        cascade: 0,
                    })
                };
                result.models.insert(format!("{model_path}.havok"), model);
            }
        }
        Ok(result)
    }

    pub(super) fn model(&self, path: &str) -> Option<&Model> {
        self.models.get(path)
    }

    pub(super) fn tree(&self, path: &str) -> Option<&Tree> {
        self.trees.get(&path.to_ascii_lowercase())
    }
}

fn trees(root: &pxml::Element, bushes: &HashSet<String>) -> Result<HashMap<String, Tree>> {
    let Some(section) = root.get_child_element("trees") else {
        return Ok(HashMap::new());
    };
    anyhow::ensure!(
        !bushes.is_empty(),
        "Missing or empty {BUSHES_PATH}; cannot classify destructible trees"
    );
    Ok(section
        .iter_child_elements()
        .filter_map(|(_, entry)| {
            let path = entry
                .get_child_string("filename")?
                .trim()
                .to_ascii_lowercase();
            let name = path.rsplit('/').next()?.strip_suffix(".srt")?;
            if bushes.contains(name) {
                return None;
            }
            let height = entry
                .get_child_string("physicParams")?
                .split_whitespace()
                .nth(1)?
                .parse::<f32>()
                .ok()?;
            (height.is_finite() && height > 0.0).then_some((
                path,
                Tree {
                    height,
                    health: health(entry)?,
                },
            ))
        })
        .collect())
}

fn structure(modules: &pxml::Element) -> HashMap<String, Fragment> {
    let definitions: BTreeMap<_, _> = modules
        .iter_child_elements()
        .filter_map(|(_, module)| {
            let name = material_name(module.get_child_string("matName")?.trim());
            let mut tokens = name.split('_');
            tokens.next()?;
            let id = tokens.next()?.parse::<u32>().ok()?;
            let dependencies = tokens
                .map(str::parse::<u32>)
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            Some((id, (name, health(module).unwrap_or(0), dependencies)))
        })
        .take(MAX_MODULES)
        .collect();
    let indexed: BTreeMap<_, _> = definitions
        .into_iter()
        .enumerate()
        .map(|(position, (id, (name, health, dependencies)))| {
            (id, (position, name, health, dependencies))
        })
        .collect();
    indexed
        .values()
        .filter(|(_, _, health, _)| *health != 0)
        .map(|&(position, name, health, ref dependencies)| {
            let mut cascade = 0;
            let mut visited = 1 << position;
            let mut pending = dependencies.clone();
            while let Some(next) = pending.pop() {
                let Some(&(index, _, _, ref dependencies)) = indexed.get(&next) else {
                    continue;
                };
                let bit = 1 << index;
                if visited & bit == 0 {
                    visited |= bit;
                    pending.extend(dependencies);
                    cascade |= bit;
                }
            }
            (
                name.to_owned(),
                Fragment {
                    part: FIRST_MODULE_KIND + position as u32,
                    health,
                    cascade,
                },
            )
        })
        .collect()
}

fn material_name(name: &str) -> &str {
    name.strip_prefix("s_")
        .or_else(|| name.strip_prefix("n_"))
        .unwrap_or(name)
}

fn health(item: &pxml::Element) -> Option<u32> {
    u32::try_from(item.get_child_integer("health")?)
        .ok()
        .filter(|&health| health > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbered_tree_definitions_require_the_bush_catalog_and_exclude_its_entries() {
        let mut section = pxml::Element::new();
        for path in ["vegetation/Hawthorn_01.srt", "vegetation/Hawthorn_03.srt"] {
            let mut entry = pxml::Element::new();
            entry.add_children("filename", pxml::Value::String(path.into()));
            entry.add_children("physicParams", pxml::Value::String("10 3".into()));
            entry.add_children("health", pxml::Value::Integer(3));
            section.add_children("entry", pxml::Value::Element(Box::new(entry)));
        }
        let mut root = pxml::Element::new();
        root.add_children("trees", pxml::Value::Element(Box::new(section)));

        let error = trees(&root, &HashSet::new()).err().unwrap();
        assert!(error.to_string().contains("vegetation/bushes.xml"));

        let definitions = trees(&root, &HashSet::from(["hawthorn_01".into()])).unwrap();
        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions["vegetation/hawthorn_03.srt"].health, 3);
    }

    #[test]
    fn sorts_native_module_kinds_and_resolves_transitive_and_cyclic_dependencies() {
        let mut modules = pxml::Element::new();
        for name in ["s_wood_20_30", "s_wood_40", "s_wood_30_10", "s_wood_10_20"] {
            let mut module = pxml::Element::new();
            module.add_children("matName", pxml::Value::String(name.into()));
            module.add_children("health", pxml::Value::Integer(100));
            modules.add_children("module", pxml::Value::Element(Box::new(module)));
        }
        let fragments = structure(&modules);
        for (name, kind, cascade) in [
            ("wood_10_20", 73, 0b0110),
            ("wood_20_30", 74, 0b0101),
            ("wood_30_10", 75, 0b0011),
            ("wood_40", 76, 0),
        ] {
            let fragment = fragments[name];
            assert_eq!(
                (fragment.part, fragment.health, fragment.cascade),
                (kind, 100, cascade)
            );
        }
    }
}
