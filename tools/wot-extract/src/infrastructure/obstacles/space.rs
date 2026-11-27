use anyhow::{Context, Result, ensure};
use byteorder::{LE, ReadBytesExt};
use glam::{Affine3A, Mat4};
use std::{collections::HashMap, io::Read};

pub(super) struct Placement {
    pub transform: Affine3A,
    pub visibility: u32,
    pub collision: u32,
    pub destructible: Option<u32>,
}

pub(super) const TREE_INSTANCE: u32 = 1 << 31;

pub(super) struct PlacedTree {
    pub native: Option<u32>,
    pub resource: u32,
    pub transform: Affine3A,
}

pub(super) struct Space<'a> {
    pub paths: HashMap<u32, &'a str>,
    pub placements: Vec<Placement>,
    pub trees: Vec<PlacedTree>,
}

impl<'a> Space<'a> {
    pub(super) fn decode(mut bytes: &'a [u8]) -> Result<Option<Self>> {
        let file = bytes;
        ensure!(
            &bytes.read_u32::<LE>()?.to_le_bytes() == b"BWTB",
            "Invalid compiled space"
        );
        bytes = bytes.get(16..).context("Truncated compiled space header")?;
        let sections = bytes.read_u32::<LE>()?;
        ensure!(sections <= 256, "Too many compiled space sections");
        let mut strings = None;
        let mut models = None;
        let mut instances = None;
        let mut destruction = None;
        let mut trees = None;
        for _ in 0..sections {
            let mut id = [0; 4];
            bytes.read_exact(&mut id)?;
            let version = bytes.read_u32::<LE>()?;
            let offset = usize::try_from(bytes.read_u64::<LE>()?)?;
            let size = usize::try_from(bytes.read_u64::<LE>()?)?;
            let target = match &id {
                b"BWST" => (&mut strings, 2),
                b"BSMO" => (&mut models, 3),
                b"BSMI" => (&mut instances, 3),
                b"WGDE" => (&mut destruction, 1),
                b"SpTr" => (&mut trees, 3),
                _ => continue,
            };
            ensure!(version == target.1, "Unsupported {id:?} format {version}");
            let end = offset
                .checked_add(size)
                .context("Compiled space section overflow")?;
            *target.0 = Some(
                file.get(offset..end)
                    .context("Truncated compiled space section")?,
            );
        }
        let (Some(strings), Some(mut models), Some(mut instances)) = (strings, models, instances)
        else {
            return Ok(None);
        };

        for _ in 0..15 {
            vector(&mut models, None)?;
        }
        let collisions = vector(&mut models, Some(8))?;
        let transforms = vector(&mut instances, Some(64))?;
        vector(&mut instances, None)?;
        let masks = vector(&mut instances, Some(4))?;
        let model_ids = vector(&mut instances, Some(8))?;
        ensure!(
            masks.len() * 2 == model_ids.len() && transforms.len() / 16 == masks.len(),
            "Inconsistent placement vectors"
        );

        let numbering = destruction
            .map(destructible_numbers)
            .transpose()?
            .unwrap_or_default();
        let mut placements = Vec::new();
        for (index, ((matrix, visibility), model)) in transforms
            .as_chunks::<64>()
            .0
            .iter()
            .zip(masks.as_chunks::<4>().0)
            .zip(model_ids.as_chunks::<8>().0)
            .enumerate()
        {
            let start = usize::try_from(model.as_slice().read_u32::<LE>()?)? * 8;
            let mut collision = collisions
                .get(start..start + 4)
                .context("Invalid placement model")?;
            let collision = collision.read_u32::<LE>()?;
            let visibility = u32::from_le_bytes(*visibility);
            if collision == 0 || visibility & 0x000f_ffff == 0 {
                continue;
            }
            let mut matrix = matrix.as_slice();
            let mut values = [0.0; 16];
            matrix.read_f32_into::<LE>(&mut values)?;
            let transform = Affine3A::from_mat4(Mat4::from_cols_array(&values));
            ensure!(transform.is_finite(), "Non-finite placement transform");
            placements.push(Placement {
                transform,
                visibility,
                collision,
                destructible: numbering.get(&(index as u32)).copied(),
            });
        }

        Ok(Some(Self {
            paths: paths(strings)?,
            placements,
            trees: trees
                .map(|section| placed_trees(section, &numbering))
                .transpose()?
                .unwrap_or_default(),
        }))
    }
}

fn placed_trees(mut section: &[u8], numbers: &HashMap<u32, u32>) -> Result<Vec<PlacedTree>> {
    let placements = vector(&mut section, Some(80))?.as_chunks::<80>().0;
    let mut trees = Vec::new();
    for (index, row) in placements.iter().enumerate() {
        let native = numbers
            .get(&(TREE_INSTANCE | u32::try_from(index)?))
            .copied();
        let mut row = row.as_slice();
        let mut values = [0.0; 16];
        row.read_f32_into::<LE>(&mut values)?;
        let transform = Affine3A::from_mat4(Mat4::from_cols_array(&values));
        let resource = row.read_u32::<LE>()?;
        if transform.is_finite() {
            trees.push(PlacedTree {
                native,
                resource,
                transform,
            });
        }
    }
    Ok(trees)
}

fn destructible_numbers(mut section: &[u8]) -> Result<HashMap<u32, u32>> {
    let chunks = vector(&mut section, Some(12))?;
    let ranges = vector(&mut section, Some(8))?.as_chunks::<8>().0;
    let instances = vector(&mut section, Some(4))?.as_chunks::<4>().0;
    let mut by_instance = HashMap::new();
    let mut ambiguous = std::collections::HashSet::new();
    for chunk in chunks.as_chunks::<12>().0 {
        let mut chunk = chunk.as_slice();
        let id = chunk.read_u32::<LE>()?;
        let first = usize::try_from(chunk.read_u32::<LE>()?)?;
        let count = usize::try_from(chunk.read_u32::<LE>()?)?;
        ensure!(
            id <= u32::from(u16::MAX) && count <= 1024,
            "Invalid destructible chunk"
        );
        let entries = ranges
            .get(
                first
                    ..first
                        .checked_add(count)
                        .context("Destructible chunk overflow")?,
            )
            .context("Destructible chunk exceeds number of ranges")?;
        for (index, entry) in entries.iter().enumerate() {
            let mut entry = entry.as_slice();
            let start = entry.read_u32::<LE>()?;
            let last = entry.read_u32::<LE>()?;
            if last == u32::MAX || last < start {
                continue;
            }
            let placed = instances
                .get(start as usize..=last as usize)
                .context("Destructible placement range exceeds instance table")?;
            let native = id << 16 | index as u32;
            for &instance in placed {
                let instance = u32::from_le_bytes(instance);
                if by_instance
                    .insert(instance, native)
                    .is_some_and(|previous| previous != native)
                {
                    ambiguous.insert(instance);
                }
            }
        }
    }
    by_instance.retain(|instance, _| !ambiguous.contains(instance));
    Ok(by_instance)
}

fn paths(mut section: &[u8]) -> Result<HashMap<u32, &str>> {
    let mut entries = vector(&mut section, Some(12))?;
    let size = usize::try_from(section.read_u32::<LE>()?)?;
    let data = section
        .get(..size)
        .context("Truncated compiled space strings")?;
    let mut paths = HashMap::new();
    while !entries.is_empty() {
        let _stored_hash = entries.read_u32::<LE>()?;
        let start = usize::try_from(entries.read_u32::<LE>()?)?;
        let size = usize::try_from(entries.read_u32::<LE>()?)?;
        let path = data
            .get(start..start + size)
            .context("Invalid compiled space string")?;
        if path.ends_with(b".havok") || path.ends_with(b".srt") {
            let hash = path.iter().fold(0xcbf29ce484222325u64, |hash, &byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
            }) as u32;
            paths.insert(hash, std::str::from_utf8(path)?);
        }
    }
    Ok(paths)
}

fn vector<'a>(input: &mut &'a [u8], width: Option<usize>) -> Result<&'a [u8]> {
    let stride = usize::try_from(input.read_u32::<LE>()?)?;
    let count = usize::try_from(input.read_u32::<LE>()?)?;
    ensure!(
        width.is_none_or(|width| stride == width || count == 0),
        "Unexpected compiled space element width"
    );
    let size = stride
        .checked_mul(count)
        .context("Compiled space vector overflow")?;
    let (data, rest) = input
        .split_at_checked(size)
        .context("Truncated compiled space vector")?;
    *input = rest;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_chunk_local_indices_for_grouped_instances_and_trees() {
        let words = [
            12,
            2,
            0x8083,
            0,
            3,
            0x8084,
            3,
            1, // chunks
            8,
            4,
            0,
            1,
            2,
            2,
            3,
            3,
            4,
            4, // ranges
            4,
            5,
            5390,
            5391,
            1 << 31 | 12,
            5387,
            5392, // instances
        ];
        let data: Vec<u8> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        let mapping = destructible_numbers(&data).unwrap();
        assert_eq!(
            mapping,
            HashMap::from([
                (5390, 0x8083_0000),
                (5391, 0x8083_0000),
                (TREE_INSTANCE | 12, 0x8083_0001),
                (5387, 0x8083_0002),
                (5392, 0x8084_0000),
            ])
        );
    }
}
