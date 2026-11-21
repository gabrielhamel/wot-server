mod selection;

use anyhow::{Context, Result, ensure};
use std::{collections::BTreeSet, fs, path::Path};

use crate::infrastructure::{ClientResources, obstacles};
use wot_resources::{ResourceBundle, validate_resource_path};

pub struct Summary {
    pub files: usize,
    pub bytes: u64,
}

pub fn extract(game_dir: &Path, output: &Path) -> Result<Summary> {
    let game_version = crate::infrastructure::game_version(game_dir)?;
    let mut client = ClientResources::open(game_dir)?;
    let extensions: BTreeSet<_> = client
        .paths()
        .filter_map(|path| path.strip_suffix("/extension.xml"))
        .map(str::to_owned)
        .collect();
    let mut files = Vec::new();
    let mut classes = BTreeSet::new();
    let mut terrain = Vec::new();
    let mut placements = Vec::new();
    for path in client.paths() {
        if obstacles::is_placement(path) {
            validate_resource_path(path)?;
            placements.push(path.to_owned());
            continue;
        }
        if crate::infrastructure::is_terrain(path) {
            validate_resource_path(path)?;
            terrain.push(path.to_owned());
            continue;
        }
        let Some((root, relative)) = selection::resource_root(path, &extensions) else {
            continue;
        };
        if selection::is_definition(root, relative) {
            validate_resource_path(path)?;
            files.push(path.to_owned());
        }
        if let Some(name) = selection::client_class(relative) {
            classes.insert(name.to_owned());
        }
    }
    // Player Sessions uses client class presence only for components. Discard other script names.
    let components: BTreeSet<_> = files
        .iter()
        .filter_map(|path| selection::component_name(path))
        .collect();
    classes.retain(|name| components.contains(name.as_str()));

    for path in selection::required_files() {
        ensure!(client.contains(&path), "Missing client resource {path}");
    }

    fs::create_dir(output)
        .with_context(|| format!("Create new output directory {}", output.display()))?;
    let result = (|| {
        let mut bytes = 0;
        for path in &files {
            bytes += client.copy(path, &output.join("res").join(path))?;
        }
        let mut count = files.len();
        let root = output.join("res");
        for path in &terrain {
            if let Some((files, size)) = client
                .copy_terrain(path, &root)
                .with_context(|| format!("Extract terrain {path}"))?
            {
                count += files;
                bytes += size;
            }
        }
        let (collision_files, collision_bytes) =
            obstacles::extract(&mut client, &placements, &root)?;
        count += collision_files;
        bytes += collision_bytes;
        ResourceBundle::write_manifest_with_version(output, classes, game_version)
            .context("Write bundle manifest")?;
        Ok(Summary {
            files: count,
            bytes,
        })
    })();
    if result.is_err()
        && let Err(error) = fs::remove_dir_all(output)
    {
        eprintln!("Could not clean up {}: {error}", output.display());
    }
    result
}
