use anyhow::{Context, Result, ensure};
use byteorder::{LE, ReadBytesExt};
use glam::{Affine3A, Mat4};
use std::{collections::HashMap, io::Read};

pub(super) struct Placement {
    pub transform: Affine3A,
    pub visibility: u32,
    pub collision: u32,
}

pub(super) struct Space<'a> {
    pub paths: HashMap<u32, &'a str>,
    pub placements: Vec<Placement>,
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

        let mut placements = Vec::new();
        for ((matrix, visibility), model) in transforms
            .as_chunks::<64>()
            .0
            .iter()
            .zip(masks.as_chunks::<4>().0)
            .zip(model_ids.as_chunks::<8>().0)
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
            });
        }

        Ok(Some(Self {
            paths: paths(strings)?,
            placements,
        }))
    }
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
        if path.ends_with(b".havok") {
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
