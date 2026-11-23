pub(crate) mod obstacles;
mod package;

use anyhow::{Context, Result, ensure};
use package::PackageReader;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{self, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};
use wot_resources::validate_resource_path;

pub struct ClientResources {
    files: BTreeMap<String, Source>,
    packages: Vec<PackageReader<File>>,
}

enum Source {
    Package {
        package: usize,
        entry: usize,
        size: u32,
    },
    Loose(PathBuf),
}

impl ClientResources {
    pub fn open(game_dir: &Path) -> Result<Self> {
        let root = game_dir.join("res");
        let mut resources = Self {
            files: BTreeMap::new(),
            packages: Vec::new(),
        };
        for path in package_paths(game_dir, &root.join("packages"))? {
            let reader = File::open(&path)
                .and_then(PackageReader::new)
                .with_context(|| format!("Index {}", path.display()))?;
            let package = resources.packages.len();
            let mut relevant = false;
            for (entry, info) in reader.infos().enumerate() {
                if is_candidate(info.name) {
                    resources
                        .files
                        .entry(info.name.to_owned())
                        .or_insert(Source::Package {
                            package,
                            entry,
                            size: info.size,
                        });
                    relevant = true;
                }
            }
            if relevant {
                resources.packages.push(reader);
            }
        }
        let mut loose_roots = BTreeSet::new();
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let name = entry.file_name();
                if let Some(name) = name.to_str()
                    && (matches!(name, "scripts" | "gui" | "text")
                        || entry.path().join("extension.xml").is_file())
                {
                    loose_roots.insert(name.to_owned());
                }
            }
        }
        loose_roots.extend(
            resources
                .files
                .keys()
                .filter_map(|path| path.strip_suffix("/extension.xml").map(str::to_owned)),
        );
        for name in loose_roots {
            validate_resource_path(&name)?;
            match name.as_str() {
                "scripts" => resources.index_loose(&root, "scripts")?,
                "gui" => resources.index_loose(&root, "gui/flash/techtree")?,
                "text" => resources.index_loose(&root, "text/lc_messages")?,
                _ => {
                    resources.index_loose(&root, &format!("{name}/extension.xml"))?;
                    resources.index_loose(&root, &format!("{name}/scripts"))?;
                }
            }
        }
        resources.index_loose(&root, "spaces")?;
        resources.index_loose(&root, "vehicles")?;
        resources.index_loose(&root, "objects/misc/bbox")?;
        resources.index_loose(&root, "system/data/material_kinds.xml")?;
        Ok(resources)
    }

    fn index_loose(&mut self, root: &Path, key: &str) -> Result<()> {
        let path = root.join(key);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        if metadata.is_dir() {
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                if let Some(name) = entry.file_name().to_str() {
                    self.index_loose(root, &format!("{key}/{name}"))?;
                }
            }
        } else if metadata.is_file() && is_candidate(key) {
            self.files
                .entry(key.to_owned())
                .or_insert(Source::Loose(path));
        }
        Ok(())
    }

    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }

    pub fn contains(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    pub fn copy_terrain(&mut self, path: &str, root: &Path) -> Result<Option<(usize, u64)>> {
        let Some((space, chunk)) = path.rsplit_once('/') else {
            return Ok(None);
        };
        let Some(stem) = chunk.strip_suffix("o.cdata_processed") else {
            let bytes = self.copy(path, &root.join(path))?;
            return Ok(Some((1, bytes)));
        };
        let mut archive = Vec::new();
        match self
            .files
            .get(path)
            .context("Missing terrain package entry")?
        {
            Source::Package {
                package,
                entry,
                size,
            } => {
                ensure!(
                    *size <= 16 * 1024 * 1024,
                    "Terrain chunk exceeds size limit"
                );
                self.packages
                    .get_mut(*package)
                    .context("Missing map package")?
                    .read_by_index(*entry)?
                    .read_to_end(&mut archive)?;
            }
            Source::Loose(path) => {
                let file = File::open(path)?;
                ensure!(
                    file.metadata()?.len() <= 16 * 1024 * 1024,
                    "Terrain chunk exceeds size limit"
                );
                file.take(16 * 1024 * 1024 + 1).read_to_end(&mut archive)?;
            }
        }
        let mut archive = PackageReader::new(io::Cursor::new(archive))?;
        let layers: Vec<_> = archive
            .infos()
            .enumerate()
            .filter_map(|(index, item)| {
                let extension = match item.name {
                    "terrain2/heights" => "hmp",
                    "terrain2/dominanttextures" => "mat",
                    _ => return None,
                };
                Some((index, item.size, extension))
            })
            .collect();
        let directory = root.join(space).join("terrain");
        let mut total_bytes = 0;
        for &(index, size, extension) in &layers {
            ensure!(size <= 512 * 1024, "Terrain layer exceeds size limit");
            fs::create_dir_all(&directory)?;
            let mut output = BufWriter::new(
                File::options()
                    .write(true)
                    .create_new(true)
                    .open(directory.join(format!("{stem}.{extension}")))?,
            );
            let bytes = io::copy(&mut archive.read_by_index(index)?, &mut output)?;
            ensure!(bytes == u64::from(size), "Truncated terrain layer");
            output.flush()?;
            total_bytes += bytes;
        }
        Ok(Some((layers.len(), total_bytes)))
    }

    fn read_bounded(&mut self, path: &str, limit: u64) -> Result<Vec<u8>> {
        let mut data = Vec::new();
        match self
            .files
            .get(path)
            .context("Resource missing from index")?
        {
            Source::Package {
                package,
                entry,
                size,
            } => {
                ensure!(
                    u64::from(*size) <= limit,
                    "Resource {path} exceeds size limit"
                );
                let package = self
                    .packages
                    .get_mut(*package)
                    .context("Missing resource package")?;
                package.read_by_index(*entry)?.read_to_end(&mut data)?;
                ensure!(data.len() == *size as usize, "Truncated resource {path}");
            }
            Source::Loose(path) => {
                File::open(path)?.take(limit + 1).read_to_end(&mut data)?;
                ensure!(
                    data.len() as u64 <= limit,
                    "Resource {path:?} exceeds size limit"
                );
            }
        }
        Ok(data)
    }

    pub fn copy(&mut self, path: &str, target: &Path) -> Result<u64> {
        (|| {
            fs::create_dir_all(target.parent().context("Output file has no parent")?)?;
            let mut output =
                BufWriter::new(File::options().write(true).create_new(true).open(target)?);
            let bytes = match self
                .files
                .get(path)
                .context("Resource missing from index")?
            {
                Source::Package {
                    package,
                    entry,
                    size,
                } => {
                    let package = self
                        .packages
                        .get_mut(*package)
                        .context("Package missing from index")?;
                    let mut reader = BufReader::new(package.read_by_index(*entry)?);
                    let bytes = io::copy(&mut reader, &mut output)?;
                    ensure!(bytes == u64::from(*size), "Truncated package entry");
                    bytes
                }
                Source::Loose(path) => io::copy(&mut File::open(path)?, &mut output)?,
            };
            output.flush()?;
            Ok(bytes)
        })()
        .with_context(|| format!("Extract {path}"))
    }
}

pub fn is_terrain(path: &str) -> bool {
    let Some((space, filename)) = path.strip_prefix("spaces/").and_then(|p| p.split_once('/'))
    else {
        return false;
    };
    !space.is_empty()
        && (filename == "space.settings"
            || filename
                .strip_suffix("o.cdata_processed")
                .is_some_and(|coordinates| {
                    coordinates.len() == 8
                        && coordinates.bytes().all(|digit| digit.is_ascii_hexdigit())
                }))
}

fn is_candidate(path: &str) -> bool {
    if is_terrain(path)
        || obstacles::is_placement(path)
        || obstacles::is_vehicle_collision_model(path)
        || ((path.starts_with("content/") || path.starts_with("objects/misc/bbox/"))
            && path.ends_with(".havok"))
        || path == "system/data/material_kinds.xml"
    {
        return true;
    }
    let Some((first, rest)) = path.split_once('/') else {
        return false;
    };
    if path.starts_with("gui/flash/techtree/") || path.starts_with("text/lc_messages/") {
        return true;
    }
    let relative = if first == "scripts" {
        rest
    } else if rest == "extension.xml" {
        return true;
    } else if let Some(relative) = rest.strip_prefix("scripts/") {
        relative
    } else {
        return false;
    };
    match relative.split_once('/') {
        Some(("client", file)) => file.ends_with(".pyc") && !file.contains('/'),
        Some(("entity_defs" | "component_defs" | "item_defs" | "arena_defs", _)) => true,
        _ => matches!(
            relative,
            "entities.xml" | "components.xml" | "destructibles.xml"
        ),
    }
}

fn package_paths(game_dir: &Path, packages_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut installed = BTreeSet::new();
    for entry in fs::read_dir(packages_dir)
        .with_context(|| format!("Read client packages in {}", packages_dir.display()))?
    {
        let entry = entry?;
        if entry
            .path()
            .extension()
            .is_some_and(|extension| extension == "pkg")
            && entry.file_type()?.is_file()
        {
            installed.insert(entry.path());
        }
    }
    let paths_file = game_dir.join("paths.xml");
    let order = match fs::read_to_string(&paths_file) {
        Ok(xml) => xml,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(installed.into_iter().collect());
        }
        Err(error) => return Err(error).with_context(|| format!("Read {}", paths_file.display())),
    };
    let order = roxmltree::Document::parse(&order)
        .with_context(|| format!("Parse {}", paths_file.display()))?;
    let mut packages = Vec::new();
    for node in order
        .descendants()
        .filter(|node| node.has_tag_name("Package"))
    {
        if let Some(name) = node
            .text()
            .and_then(|text| text.trim().strip_prefix("./res/packages/"))
            && let Some(path) = installed.take(&packages_dir.join(name))
        {
            packages.push(path);
        }
    }
    packages.extend(installed);
    Ok(packages)
}

pub fn game_version(game_dir: &Path) -> Result<Option<String>> {
    let xml = match fs::read_to_string(game_dir.join("version.xml")) {
        Ok(xml) => xml,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("Read source client version.xml"),
    };
    let document = roxmltree::Document::parse(&xml).context("Parse source client version.xml")?;
    Ok(document
        .root_element()
        .children()
        .find(|node| node.has_tag_name("version"))
        .and_then(|node| node.text())
        .map(str::trim)
        .filter(|version| !version.is_empty())
        .map(str::to_owned))
}
