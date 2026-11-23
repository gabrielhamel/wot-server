mod collision;
mod destructibles;
mod mesh;
mod space;
mod tagfile;
mod trees;

use super::ClientResources;
use anyhow::{Context, Result};
use byteorder::{LE, WriteBytesExt};
use std::{
    collections::HashMap,
    fs::{File, create_dir_all},
    io::{BufWriter, Write},
    path::Path,
};

pub(crate) fn is_placement(path: &str) -> bool {
    path.strip_prefix("spaces/")
        .and_then(|path| path.split_once('/'))
        .is_some_and(|(_, file)| file == "space.bin")
}

pub(crate) fn extract(
    client: &mut ClientResources,
    layouts: &[String],
    root: &Path,
) -> Result<(usize, u64)> {
    let mesh_root = root.join("collision-models");
    create_dir_all(&mesh_root)?;
    let mut models = HashMap::new();
    let mut files = 0;
    let mut bytes = 0;
    let mut unsupported = 0;
    let destructibles = destructibles::Catalog::load(client)?;
    for path in layouts {
        let data = client.read_bounded(path, 384 * 1024 * 1024)?;
        let Some(space) = space::Space::decode(&data).with_context(|| format!("Decode {path}"))?
        else {
            continue;
        };
        let (directory, _) = path.rsplit_once('/').context("Missing map directory")?;
        let map_root = root.join(directory);
        create_dir_all(&map_root)?;
        if let Some(size) = trees::extract(&space, &destructibles, &map_root.join("trees.bin"))? {
            files += 1;
            bytes += size;
        }
        let mesh_path = map_root.join("collision.bin");
        let mut placements = BufWriter::new(File::create(&mesh_path)?);
        placements.write_all(b"WCP2")?;
        for item in space.placements {
            let Some(&model_path) = space.paths.get(&item.collision) else {
                continue;
            };
            let id = if let Some(&id) = models.get(model_path) {
                id
            } else {
                let id = u32::try_from(models.len())?;
                let model = read_collision(client, model_path, destructibles.model(model_path))?;
                let id = match model {
                    Some(triangles) if !triangles.is_empty() => {
                        let path = mesh_root.join(format!("{id:08x}.mesh"));
                        let mut output = BufWriter::new(File::create(&path)?);
                        output.write_all(b"WCM3")?;
                        for triangle in triangles {
                            output.write_u32::<LE>(triangle.material)?;
                            let (part, health, cascade) =
                                triangle.fragment.map_or((0, 0, 0), |fragment| {
                                    (fragment.part, fragment.health, fragment.cascade)
                                });
                            output.write_u32::<LE>(part)?;
                            output.write_u32::<LE>(health)?;
                            output.write_u32::<LE>(cascade)?;
                            for value in triangle.vertices.into_iter().flat_map(|v| v.to_array()) {
                                output.write_f32::<LE>(value)?;
                            }
                        }
                        output.flush()?;
                        files += 1;
                        bytes += path.metadata()?.len();
                        Some(id)
                    }
                    Some(_) => None,
                    None => {
                        unsupported += 1;
                        None
                    }
                };
                models.insert(model_path.to_owned(), id);
                id
            };
            let Some(id) = id else {
                continue;
            };
            placements.write_u32::<LE>(id)?;
            placements.write_u32::<LE>(item.visibility)?;
            placements.write_u32::<LE>(item.destructible.unwrap_or(u32::MAX))?;
            for value in item.transform.to_cols_array() {
                placements.write_f32::<LE>(value)?;
            }
        }
        placements.flush()?;
        files += 1;
        bytes += mesh_path.metadata()?.len();
    }
    if unsupported != 0 {
        eprintln!("Skipped {unsupported} unavailable or unsupported obstacle collision models");
    }
    Ok((files, bytes))
}

fn read_collision(
    client: &mut ClientResources,
    path: &str,
    model: Option<&destructibles::Model>,
) -> Result<Option<Vec<collision::Triangle>>> {
    if !client.contains(path) {
        return Ok(None);
    }
    let data = client.read_bounded(path, 16 * 1024 * 1024)?;
    Ok(tagfile::Tagfile::from_bytes(&data)
        .ok()
        .and_then(|tagfile| collision::triangles(&tagfile, model)))
}
