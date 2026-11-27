use super::{destructibles::Catalog, space::Space};
use anyhow::Result;
use byteorder::{LE, WriteBytesExt};
use std::{
    fs::File,
    io::{BufWriter, Write},
    path::Path,
};

pub(super) fn extract(space: &Space<'_>, catalog: &Catalog, path: &Path) -> Result<Option<u64>> {
    let mut trees = space
        .trees
        .iter()
        .filter_map(|tree| {
            let native = tree.native?;
            let definition = catalog.tree(space.paths.get(&tree.resource)?)?;
            let scale = tree.transform.matrix3.y_axis.length();
            let height = definition.height * scale;
            if !height.is_finite() || scale < 0.01 {
                return None;
            }
            let health = ((definition.health as f32 * scale * scale - 0.001).ceil() as u32).max(1);
            Some((native, tree.transform.translation, scale, height, health))
        })
        .peekable();
    if trees.peek().is_none() {
        return Ok(None);
    }
    let mut out = BufWriter::new(File::create(path)?);
    out.write_all(b"WCT1")?;
    for (native, position, scale, height, health) in trees {
        out.write_u32::<LE>(native)?;
        out.write_u32::<LE>(health)?;
        for value in [position.x, position.y, position.z, scale, height] {
            out.write_f32::<LE>(value)?;
        }
    }
    out.flush()?;
    Ok(Some(path.metadata()?.len()))
}
