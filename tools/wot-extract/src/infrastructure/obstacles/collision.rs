//! Collision models, as the game ships them for its vehicles' parts and its maps' static
//! models: a Havok tagfile whose named variants each hold a "Collision Physics Data"
//! system of mesh bodies, and an `HKBodyFlagsData` giving their bounds and each body's
//! material kind and collision flags, by its name without its `s_` (solid) or `n_` prefix. A vehicle's part
//! has one variant, a destructible building one per fragment, `n_wood0_1_5` say.

use glam::Vec3;

use super::mesh::compressed_mesh_triangles;
use super::tagfile::{Node, Tagfile};

pub(super) struct Triangle {
    pub vertices: [Vec3; 3],
    pub material: u32,
}

pub(super) fn triangles(tagfile: &Tagfile) -> Option<Vec<Triangle>> {
    let mut triangles = Vec::new();
    for named in tagfile.root()?.get("namedVariants")?.elements()? {
        let handles: Vec<_> = named
            .get("variant")?
            .deref()?
            .get("resourceHandles")?
            .elements()?
            .into_iter()
            .filter_map(|handle| handle.deref())
            .collect();
        let infos: Vec<_> = variant(&handles, "HKBodyFlagsData")?
            .get("flags_")?
            .elements()?
            .into_iter()
            .filter_map(|info| {
                let name = info.get("name")?.as_str()?;
                let material = info.get("normalMatKind")?.as_int()? as u32;
                let flags = info
                    .get("collisionFlags")
                    .and_then(|flags| flags.as_int())
                    .unwrap_or(0);
                Some((name, material, flags))
            })
            .collect();

        for body in variant(&handles, "Collision Physics Data")?
            .get("bodyCinfos")?
            .elements()?
        {
            let name = body
                .get("name")
                .and_then(|name| name.as_str())
                .unwrap_or_default();
            let name = name
                .strip_prefix("s_")
                .or_else(|| name.strip_prefix("n_"))
                .unwrap_or(name);
            let (material, flags) = infos
                .iter()
                .find(|(part, _, _)| *part == name)
                .map_or((0, 0), |&(_, material, flags)| (material, flags));
            if name.starts_with("c_") || flags & 2 != 0 {
                continue;
            }
            let shape = body.get("shape")?.deref()?;
            if shape.type_name() == "hknpCompressedMeshShape" {
                let mesh = compressed_mesh_triangles(shape)?;
                if triangles.len() + mesh.len() > 1_000_000 {
                    return None;
                }
                triangles.extend(
                    mesh.into_iter()
                        .filter(|vertices| vertices.iter().all(|vertex| vertex.is_finite()))
                        .map(|vertices| Triangle { vertices, material }),
                );
            }
        }
    }
    Some(triangles)
}

/// The object of a named resource handle.
fn variant<'a>(handles: &[Node<'a>], name: &str) -> Option<Node<'a>> {
    handles
        .iter()
        .find(|handle| handle.get("name").and_then(|n| n.as_str()) == Some(name))
        .and_then(|handle| handle.get("variant")?.deref())
}
