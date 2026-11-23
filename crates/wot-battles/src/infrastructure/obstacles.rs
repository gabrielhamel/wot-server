use super::*;
use byteorder::{LE, ReadBytesExt};
use glam::Affine3A;
use std::io::{BufReader, Read};
use wot_platform::client_formats::pxml;

#[derive(Default)]
pub(super) struct Obstacles {
    meshes: Vec<Mesh>,
    instances: Vec<Instance>,
    index: Option<Box<CollisionIndex>>,
}

#[derive(Clone, Copy)]
pub(super) struct ObstacleHit {
    pub distance: f32,
    pub material: u32,
}

struct Mesh {
    bounds: Bounds,
    triangles: Vec<Triangle>,
}

struct Triangle {
    vertices: [Vec3; 3],
    material: u32,
}

struct Instance {
    world_to_model: Affine3A,
    bounds: Bounds,
    mesh: usize,
}

struct CollisionIndex {
    bounds: Bounds,
    children: Option<[Box<Self>; 2]>,
    count: usize,
}

#[derive(Clone, Copy)]
struct Bounds {
    min: Vec3,
    max: Vec3,
}

impl Obstacles {
    #[tracing::instrument(name = "battles.load_obstacles", skip_all, fields(
        ddd.bounded_context.name = "battles", input.geometry = geometry,
        input.arena_type_id = arena_type, output.obstacles.instances = tracing::field::Empty,
        otel.status_code = tracing::field::Empty,
    ), err)]
    pub(super) fn load(
        resources: &ResourceBundle,
        geometry: &str,
        arena_type: i32,
    ) -> Result<Self> {
        let path = format!("{geometry}/collision.bin");
        if !resources.has_file(&path) {
            tracing::Span::current().record("output.obstacles.instances", 0);
            return Ok(Self::default());
        }
        let mask = visibility_mask(resources, geometry, arena_type)?;
        let placements = records(resources, &path, b"WCP1", 56, 200_000)?;
        let mut meshes = Vec::new();
        let mut ids = HashMap::new();
        let mut instances = Vec::new();
        let mut triangle_count = 0;
        for bytes in placements.as_chunks::<56>().0 {
            let mut placement = bytes.as_slice();
            let id = placement.read_u32::<LE>()?;
            if placement.read_u32::<LE>()? & mask == 0 {
                continue;
            }
            let mut transform = [0.0; 12];
            placement.read_f32_into::<LE>(&mut transform)?;
            let transform = Affine3A::from_cols_array(&transform);
            let determinant = transform.matrix3.determinant();
            if !transform.is_finite() || !determinant.is_finite() || determinant.abs() < 1e-8 {
                continue;
            }
            let mesh = match ids.get(&id) {
                Some(&mesh) => mesh,
                None => {
                    let mesh = Mesh::load(resources, id)?;
                    triangle_count += mesh.triangles.len();
                    anyhow::ensure!(
                        triangle_count <= 4_000_000,
                        "Map collision exceeds triangle limit"
                    );
                    let index = meshes.len();
                    meshes.push(mesh);
                    ids.insert(id, index);
                    index
                }
            };
            let world_to_model = transform.inverse();
            let bounds = meshes[mesh].bounds.transform(transform);
            if world_to_model.is_finite() && bounds.min.is_finite() && bounds.max.is_finite() {
                instances.push(Instance {
                    world_to_model,
                    bounds,
                    mesh,
                });
            }
        }
        let index =
            (!instances.is_empty()).then(|| Box::new(CollisionIndex::build(&mut instances)));
        tracing::Span::current().record("output.obstacles.instances", instances.len());
        Ok(Self {
            meshes,
            instances,
            index,
        })
    }

    pub(super) fn intersect(
        &self,
        origin: Vec3,
        direction: Vec3,
        range: f32,
    ) -> Option<ObstacleHit> {
        let mut nearest = None;
        self.index.as_ref()?.intersect(
            &self.instances,
            &self.meshes,
            origin,
            direction,
            range,
            &mut nearest,
        );
        nearest
    }
}

impl Mesh {
    fn load(resources: &ResourceBundle, id: u32) -> Result<Self> {
        let path = format!("collision-models/{id:08x}.mesh");
        let data = records(resources, &path, b"WCM1", 40, 1_000_000)?;
        let mut triangles = Vec::new();
        let mut bounds = Bounds::empty();
        for bytes in data.as_chunks::<40>().0 {
            let mut record = bytes.as_slice();
            let material = record.read_u32::<LE>()?;
            let mut coordinates = [0.0; 9];
            record.read_f32_into::<LE>(&mut coordinates)?;
            let (vertices, _) = coordinates.as_chunks::<3>();
            let [a, b, c] = vertices else {
                anyhow::bail!("Invalid collision triangle")
            };
            let vertices = [*a, *b, *c].map(Vec3::from_array);
            anyhow::ensure!(
                vertices.iter().all(|point| point.is_finite()),
                "Invalid collision point"
            );
            for point in vertices {
                bounds.min = bounds.min.min(point);
                bounds.max = bounds.max.max(point);
            }
            triangles.push(Triangle { vertices, material });
        }
        anyhow::ensure!(!triangles.is_empty(), "Missing collision triangles");
        Ok(Self { bounds, triangles })
    }
}

impl CollisionIndex {
    fn build(instances: &mut [Instance]) -> Self {
        let bounds = instances
            .iter()
            .fold(Bounds::empty(), |bounds, item| bounds.union(item.bounds));
        let count = instances.len();
        let children = (count > 8).then(|| {
            let extent = bounds.max - bounds.min;
            let axis = if extent.x >= extent.y && extent.x >= extent.z {
                0
            } else if extent.y >= extent.z {
                1
            } else {
                2
            };
            let middle = count / 2;
            instances.select_nth_unstable_by(middle, |a, b| {
                let center = |bounds: Bounds| (bounds.min * 0.5 + bounds.max * 0.5)[axis];
                center(a.bounds).total_cmp(&center(b.bounds))
            });
            let (left, right) = instances.split_at_mut(middle);
            [Box::new(Self::build(left)), Box::new(Self::build(right))]
        });
        Self {
            bounds,
            count,
            children,
        }
    }

    fn intersect(
        &self,
        instances: &[Instance],
        meshes: &[Mesh],
        origin: Vec3,
        direction: Vec3,
        range: f32,
        nearest: &mut Option<ObstacleHit>,
    ) {
        let limit = nearest.map_or(range, |hit| hit.distance);
        if !self.bounds.intersects(origin, direction, limit) {
            return;
        }
        if let Some([left, right]) = &self.children {
            let (left_instances, right_instances) = instances.split_at(left.count);
            left.intersect(left_instances, meshes, origin, direction, range, nearest);
            right.intersect(right_instances, meshes, origin, direction, range, nearest);
            return;
        }

        for instance in instances {
            let limit = nearest.map_or(range, |hit| hit.distance);
            if !instance.bounds.intersects(origin, direction, limit) {
                continue;
            }
            let local_origin = instance.world_to_model.transform_point3(origin);
            let local_direction = instance.world_to_model.transform_vector3(direction);
            for triangle in &meshes[instance.mesh].triangles {
                if let Some(distance) = triangle.intersect(local_origin, local_direction)
                    && distance < nearest.map_or(range, |hit| hit.distance)
                {
                    *nearest = Some(ObstacleHit {
                        distance,
                        material: triangle.material,
                    });
                }
            }
        }
    }
}

impl Triangle {
    fn intersect(&self, origin: Vec3, direction: Vec3) -> Option<f32> {
        let [a, b, c] = self.vertices;
        let ab = b - a;
        let ac = c - a;
        let cross = direction.cross(ac);
        let determinant = ab.dot(cross);
        if determinant.abs() < 1e-8 {
            return None;
        }
        let offset = origin - a;
        let u = offset.dot(cross) / determinant;
        let cross = offset.cross(ab);
        let v = direction.dot(cross) / determinant;
        let distance = ac.dot(cross) / determinant;
        (u >= 0.0 && v >= 0.0 && u + v <= 1.0 && distance > 1e-4).then_some(distance)
    }
}

impl Bounds {
    fn empty() -> Self {
        Self {
            min: Vec3::splat(f32::INFINITY),
            max: Vec3::splat(f32::NEG_INFINITY),
        }
    }

    fn union(self, other: Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    fn transform(self, transform: Affine3A) -> Self {
        let center = transform.transform_point3(self.min + (self.max - self.min) * 0.5);
        let half = (self.max - self.min) * 0.5;
        let axes = transform.matrix3;
        let half = Vec3::from(
            axes.x_axis.abs() * half.x + axes.y_axis.abs() * half.y + axes.z_axis.abs() * half.z,
        );
        Self {
            min: center - half,
            max: center + half,
        }
    }

    fn intersects(self, origin: Vec3, direction: Vec3, range: f32) -> bool {
        let mut lo = 0.0f32;
        let mut hi = range;
        for axis in 0..3 {
            let start = origin[axis];
            let direction = direction[axis];
            let low = self.min[axis];
            let high = self.max[axis];
            if direction == 0.0 {
                if !(low..=high).contains(&start) {
                    return false;
                }
                continue;
            }
            let a = (low - start) / direction;
            let b = (high - start) / direction;
            lo = lo.max(a.min(b));
            hi = hi.min(a.max(b));
            if lo > hi {
                return false;
            }
        }
        true
    }
}

fn records(
    resources: &ResourceBundle,
    path: &str,
    magic: &[u8; 4],
    stride: usize,
    limit: usize,
) -> Result<Vec<u8>> {
    let mut input = BufReader::new(resources.read(path)?);
    let mut header = [0; 4];
    input.read_exact(&mut header)?;
    anyhow::ensure!(&header == magic, "Invalid collision format in {path}");
    let mut data = Vec::new();
    input
        .take((stride * limit + 1) as u64)
        .read_to_end(&mut data)?;
    anyhow::ensure!(
        data.len() <= stride * limit && data.len().is_multiple_of(stride),
        "Invalid collision records in {path}"
    );
    Ok(data)
}

fn visibility_mask(resources: &ResourceBundle, geometry: &str, arena_type: i32) -> Result<u32> {
    let gameplay = (arena_type >> 16) as u32;
    let tag = match gameplay {
        0 => "ctf",
        1 => "domination",
        2 => "assault",
        6 => "assault2",
        _ => anyhow::bail!("Unknown Training Room gameplay {gameplay}"),
    };
    let settings = pxml::from_reader(BufReader::new(
        resources.read(format!("{geometry}/space.settings"))?,
    ))?;
    let bit = if let Some(flags) = settings.get_child_element("spaceVisibilityFlags") {
        flags
            .iter_children("type")
            .filter_map(pxml::Value::as_element)
            .find(|flag| flag.get_child_string("tag") == Some(tag))
            .and_then(|flag| flag.get_child_integer("bitIndex"))
            .and_then(|bit| u32::try_from(bit).ok())
            .context("Missing gameplay collision visibility bit")?
    } else {
        gameplay
    };
    anyhow::ensure!(bit < 20, "Invalid gameplay collision visibility bit");
    Ok(1 << bit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ray_selects_the_first_transformed_surface_within_its_range() {
        let vertices = [Vec3::ZERO, Vec3::X * 2.0, Vec3::Y * 2.0];
        let bounds = Bounds {
            min: Vec3::ZERO,
            max: Vec3::new(2.0, 2.0, 0.0),
        };
        let mesh = |material| Mesh {
            bounds,
            triangles: vec![Triangle { vertices, material }],
        };
        let place = |x, z, mesh| {
            let transform = Affine3A::from_scale_rotation_translation(
                Vec3::new(2.0, 1.0, 0.5),
                glam::Quat::IDENTITY,
                Vec3::new(x, 0.0, z),
            );
            Instance {
                world_to_model: transform.inverse(),
                bounds: bounds.transform(transform),
                mesh,
            }
        };
        let mut instances = vec![place(0.0, 20.0, 0), place(0.0, 5.0, 1)];
        instances.extend((0..8).map(|z| place(20.0, z as f32, 0)));
        let index = Box::new(CollisionIndex::build(&mut instances));
        let obstacles = Obstacles {
            meshes: vec![mesh(40), mesh(111)],
            instances,
            index: Some(index),
        };
        let origin = Vec3::new(2.5, 0.5, 0.0);
        let hit = obstacles.intersect(origin, Vec3::Z, 30.0).unwrap();
        assert_eq!(hit.distance, 5.0);
        assert_eq!(hit.material, 111);
        assert!(obstacles.intersect(origin, Vec3::Z, 4.0).is_none());
        assert!(
            obstacles
                .intersect(origin + Vec3::Y * 3.0, Vec3::Z, 30.0)
                .is_none()
        );
    }
}
