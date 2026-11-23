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
                        output.write_all(b"WCM5")?;
                        for triangle in triangles {
                            output.write_u32::<LE>(triangle.material)?;
                            output.write_u32::<LE>(triangle.pass_flags)?;
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
                        let reason = if client.contains(model_path) {
                            "unsupported Havok mesh"
                        } else {
                            "asset unavailable in the client resource index"
                        };
                        eprintln!("Skipped obstacle collision model {model_path}: {reason}");
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
    let railway_stock = path.strip_prefix("content/Railway/").is_some_and(|path| {
        path.starts_with("hd_rw_UNI_004_Carriage/") || path.starts_with("hd_rw_UNI_005_Locomotive/")
    });
    Ok(tagfile::Tagfile::from_bytes(&data)
        .ok()
        .and_then(|tagfile| collision::triangles(&tagfile, model, railway_stock)))
}

pub(crate) fn is_vehicle_collision_model(path: &str) -> bool {
    path.starts_with("vehicles/")
        && path
            .rsplit_once("/collision_client/")
            .is_some_and(|(_, file)| {
                file.strip_suffix(".havok").is_some_and(|name| {
                    !name.contains('/')
                        && ["Chassis", "Hull", "Turret", "Gun"]
                            .iter()
                            .any(|part| name.starts_with(part))
                })
            })
}

pub(crate) fn extract_vehicle_collision(
    client: &mut ClientResources,
    paths: &[String],
    root: &Path,
) -> Result<(usize, u64)> {
    let mut count = 0;
    let mut bytes = 0;
    for path in paths {
        let data = client.read_bounded(path, 16 * 1024 * 1024)?;
        let Ok(tag) = tagfile::Tagfile::from_bytes(&data) else {
            continue;
        };
        let Some(bounds) = collision::bounds(&tag) else {
            continue;
        };
        let dest = root.join(path).with_extension("bounds");
        create_dir_all(
            dest.parent()
                .context("Missing vehicle collision directory")?,
        )?;
        let mut file = BufWriter::new(File::create(&dest)?);
        file.write_all(b"WVB1")?;
        for value in bounds.into_iter().flat_map(|point| point.to_array()) {
            file.write_f32::<LE>(value)?;
        }
        file.flush()?;
        count += 1;
        bytes += dest.metadata()?.len();
        let Some(triangles) =
            collision::triangles(&tag, None, false).filter(|triangles| !triangles.is_empty())
        else {
            continue;
        };
        let dest = dest.with_extension("armor");
        let mut file = BufWriter::new(File::create(&dest)?);
        file.write_all(b"WVA1")?;
        file.write_u32::<LE>(triangles.len().try_into()?)?;
        for triangle in triangles {
            file.write_u32::<LE>(triangle.material)?;
            for value in triangle
                .vertices
                .into_iter()
                .flat_map(|point| point.to_array())
            {
                file.write_f32::<LE>(value)?;
            }
        }
        file.flush()?;
        count += 1;
        bytes += dest.metadata()?.len();
    }
    Ok((count, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Requires WOT_GAME_DIR with the installed EU client"]
    fn eu_bbox_assets_and_buildings_decode_without_skipping_collision() {
        let root = std::env::var_os("WOT_GAME_DIR").unwrap();
        let mut client = ClientResources::open(Path::new(&root)).unwrap();
        for path in [
            "objects/misc/bbox/building_cylinder.havok",
            "objects/misc/bbox/building_wall1.havok",
            "objects/misc/bbox/building_wall2.havok",
            "objects/misc/bbox/building_wall3.havok",
            "content/Buildings/hd_bld_EU_011_TownHall/normal/lod0/hd_bld_EU_011_TownHall_01.havok",
            "content/Buildings/hd_bld_EU_011_TownHall/normal/lod0/hd_bld_EU_011_TownHall_02.havok",
            "content/Buildings/hd_bld_EU_220_VHouse/normal/lod0/hd_bld_EU_220_VHouse_01.havok",
            "content/Buildings/hd_bld_SU_013_Thouse_01/normal/lod0/hd_bld_SU_013_Thouse_01.havok",
            "content/Buildings/hd_bld_SU_029_MinskConstructor/normal/lod0/hd_bld_SU_029_MinskConstructor_Wall_01.havok",
        ] {
            let triangles = read_collision(&mut client, path, None)
                .unwrap()
                .expect(path);
            assert!(!triangles.is_empty(), "{path} has no active triangles");
        }
        let camera = read_collision(&mut client, "objects/misc/bbox/no_camera_box.havok", None)
            .unwrap()
            .unwrap();
        assert!(camera.is_empty());
    }
}
