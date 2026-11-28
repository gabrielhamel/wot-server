use super::{ClientResources, space::Space};
use anyhow::{Context, Result};
use byteorder::{LE, ReadBytesExt, WriteBytesExt};
use glam::Vec3;
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufWriter, Write},
    path::Path,
};
use wot_platform::client_formats::pxml;

pub(super) const BUSHES_PATH: &str = "vegetation/bushes.xml";

#[derive(Clone, Copy)]
struct Plant {
    bounds: [Vec3; 2],
    flags: u32,
}

pub(super) struct Catalog {
    plants: HashMap<String, Option<Plant>>,
    bushes: HashSet<String>,
    grass: HashSet<String>,
}

impl Catalog {
    pub(super) fn load(client: &mut ClientResources) -> Result<Self> {
        let mut list = |path| -> Result<_> {
            if !client.contains(path) {
                return Ok(HashSet::new());
            }
            let bytes = client
                .read_bounded(path, 4 * 1024 * 1024)
                .with_context(|| format!("Read {path}"))?;
            let xml = pxml::from_bytes(bytes).with_context(|| format!("Decode {path}"))?;
            Ok(xml
                .iter_children_all()
                .map(|(name, _)| name.to_ascii_lowercase())
                .collect())
        };
        Ok(Self {
            plants: HashMap::new(),
            bushes: list(BUSHES_PATH)?,
            grass: list("vegetation/grassBushes.xml")?,
        })
    }

    pub(super) fn bushes(&self) -> &HashSet<String> {
        &self.bushes
    }

    pub(super) fn extract(
        &mut self,
        client: &mut ClientResources,
        space: &Space<'_>,
        path: &Path,
    ) -> Result<u64> {
        let mut output = BufWriter::new(File::create(path)?);
        output.write_all(b"WCF1")?;
        for placement in &space.trees {
            let Some(&model) = space.paths.get(&placement.resource) else {
                continue;
            };
            let lookup = model.to_ascii_lowercase();
            if !self.plants.contains_key(&lookup) {
                let asset = self.read_plant(client, model, &lookup)?;
                self.plants.insert(lookup.clone(), asset);
            }
            let Some(Some(plant)) = self.plants.get(&lookup) else {
                continue;
            };
            output.write_u32::<LE>(placement.native.unwrap_or(u32::MAX))?;
            output.write_u32::<LE>(plant.flags)?;
            for value in placement.transform.to_cols_array().into_iter().chain(
                plant
                    .bounds
                    .into_iter()
                    .flat_map(|bounds| bounds.to_array()),
            ) {
                output.write_f32::<LE>(value)?;
            }
        }
        output.flush()?;
        Ok(path.metadata()?.len())
    }

    fn read_plant(
        &self,
        client: &mut ClientResources,
        model: &str,
        lookup: &str,
    ) -> Result<Option<Plant>> {
        let Some(name) = lookup
            .rsplit('/')
            .next()
            .and_then(|name| name.strip_suffix(".srt"))
        else {
            return Ok(None);
        };
        if self.grass.contains(name) || name.contains("grass") || !client.contains(model) {
            return Ok(None);
        }
        let data = client.read_bounded(model, 16 * 1024 * 1024)?;
        if !data.starts_with(b"SRT 06.0.0") {
            return Ok(None);
        }
        let Some(mut bounds) = data.get(20..44) else {
            return Ok(None);
        };
        let mut values = [0.0; 6];
        bounds.read_f32_into::<LE>(&mut values)?;
        let [min_x, min_y, min_z, max_x, max_y, max_z] = values;
        let min = Vec3::new(min_x, min_y, min_z);
        let max = Vec3::new(max_x, max_y, max_z);
        if !(min.is_finite() && max.is_finite() && min.cmplt(max).all()) {
            return Ok(None);
        }
        let bush =
            self.bushes.contains(name) || name.contains("bush") || lookup.contains("/shrubs/");
        let bare = name.contains("dry") || name.contains("leafless");
        let low_crown = ["conifer", "pine", "spruce", "fir", "juniper"]
            .iter()
            .any(|kind| lookup.contains(kind));
        let flags = u32::from(bare) | (u32::from(bush || low_crown) << 1);
        Ok(Some(Plant {
            bounds: [min, max],
            flags,
        }))
    }
}
