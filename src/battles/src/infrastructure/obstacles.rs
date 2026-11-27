use super::*;
use byteorder::{LE, ReadBytesExt};
use glam::Affine3A;
use std::io::{BufReader, Read};
use wot_platform::client_formats::pxml;

mod physics;
pub(super) use physics::SceneryColliders;
mod trees;
pub(super) use trees::TREE_PART;

#[derive(Default)]
pub(super) struct Obstacles {
    meshes: Vec<Mesh>,
    instances: Vec<Instance>,
    index: Option<Box<CollisionIndex>>,
    trees: trees::Trees,
    physics: std::sync::OnceLock<Result<Arc<physics::PhysicsGeometry>, String>>,
}

#[derive(Clone, Copy)]
pub(super) struct ObstacleHit {
    pub distance: f32,
    pub material: u32,
    pub fragment: Option<Fragment>,
}

#[derive(Clone, Copy)]
pub(super) struct Fragment {
    object: Object,
    part: u32,
    health: u32,
    cascade: u32,
}

impl Fragment {
    pub(super) fn same_part(self, other: Self) -> bool {
        self.object == other.object && self.part == other.part
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Object {
    Placement(u32),
    Native(u32),
}

#[derive(Clone, Copy)]
pub(super) struct Breakage {
    pub native: Option<u32>,
    pub part: u32,
}

pub(super) const FIRST_MODULE: u32 = 73;
const VEHICLES_PASS: u32 = 1;
const SHELLS_PASS: u32 = 2;
const RETAIN_VEHICLE_WALL: u32 = 4;
const CASCADE_MASK: u32 = (1 << 13) - 1;

#[derive(Default)]
struct Damage(HashMap<(Object, u32), u32>);

impl Damage {
    fn destroyed(&self, fragment: Option<Fragment>) -> bool {
        fragment.is_some_and(|fragment| self.0.get(&(fragment.object, fragment.part)) == Some(&0))
    }
}

#[derive(Default)]
pub(super) struct Scenery {
    geometry: Arc<Obstacles>,
    damage: Damage,
    broken_count: usize,
}

impl Scenery {
    pub fn new(geometry: Arc<Obstacles>) -> Self {
        Self {
            geometry,
            damage: Damage::default(),
            broken_count: 0,
        }
    }

    pub fn intersect(&self, origin: Vec3, direction: Vec3, range: f32) -> Option<ObstacleHit> {
        self.geometry
            .intersect(origin, direction, range, &self.damage)
    }

    pub fn chunks(&self) -> impl Iterator<Item = u16> + '_ {
        self.geometry.instances.iter().filter_map(|instance| {
            instance
                .native
                .filter(|_| self.geometry.meshes[instance.mesh].has_fragments)
                .map(|number| (number >> 16) as u16)
        })
    }

    pub fn remaining_health(&self, fragment: Fragment) -> u32 {
        self.damage
            .0
            .get(&(fragment.object, fragment.part))
            .copied()
            .unwrap_or(fragment.health)
    }

    pub fn damage(&mut self, fragment: Fragment, amount: u32) -> Vec<Breakage> {
        if amount == 0 {
            return Vec::new();
        }
        let health = self
            .damage
            .0
            .entry((fragment.object, fragment.part))
            .or_insert(fragment.health);
        let was_intact = *health != 0;
        *health = health.saturating_sub(amount);
        if !was_intact || *health != 0 {
            return Vec::new();
        }

        let native = match fragment.object {
            Object::Placement(_) => None,
            Object::Native(number) => Some(number),
        };
        let mut broken = vec![Breakage {
            native,
            part: fragment.part,
        }];
        let mut cascade = fragment.cascade;
        while cascade != 0 {
            let offset = cascade.trailing_zeros();
            cascade &= cascade - 1;
            let part = FIRST_MODULE + offset;
            if self.damage.0.insert((fragment.object, part), 0) != Some(0) {
                broken.push(Breakage { native, part });
            }
        }
        self.broken_count += broken.len();
        broken
    }

    #[cfg(test)]
    pub fn support_height(&self, point: Vec2, low: f32, high: f32) -> Option<f32> {
        let index = self.geometry.index.as_ref()?;
        if low >= high {
            return None;
        }
        let query = Bounds {
            min: Vec3::new(point.x, low, point.y),
            max: Vec3::new(point.x, high, point.y),
        };
        let origin = query.max + Vec3::Y * 0.001;
        let mut closest = high - low + 0.001;
        let mut found = false;
        index.visit(&self.geometry.instances, query, &mut |instance| {
            let local_origin = instance.world_to_model.transform_point3(origin);
            let local_direction = instance.world_to_model.transform_vector3(-Vec3::Y);
            let normal_transform = instance.world_to_model.matrix3.transpose();
            for triangle in &self.geometry.meshes[instance.mesh].triangles {
                if triangle.pass_flags & VEHICLES_PASS != 0 || triangle.fragment.is_some() {
                    continue;
                }
                let [a, b, c] = triangle.vertices;
                let normal = (normal_transform * (b - a).cross(c - a)).normalize_or_zero();
                if normal.y >= 0.65
                    && let Some(distance) = triangle.intersect(local_origin, local_direction)
                    && distance < closest
                {
                    closest = distance;
                    found = true;
                }
            }
        });
        found.then_some(origin.y - closest)
    }
}

struct Mesh {
    has_fragments: bool,
    bounds: Bounds,
    triangles: Vec<Triangle>,
}

struct Triangle {
    vertices: [Vec3; 3],
    material: u32,
    pass_flags: u32,
    fragment: Option<(u32, u32, u32)>,
}

struct Instance {
    id: u32,
    native: Option<u32>,
    health_scale: f32,
    shells_pass: bool,
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
        output.obstacles.trees = tracing::field::Empty,
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
        let (header, input) = record_input(resources, &path)?;
        let stride = match &header {
            b"WCP1" => 56,
            b"WCP2" => 60,
            _ => anyhow::bail!("Invalid placement format in {path}"),
        };
        let placements = read_records(&path, input, stride, 200_000)?;
        let mut meshes = Vec::new();
        let mut ids = HashMap::new();
        let mut instances = Vec::new();
        let mut triangle_count = 0;
        for mut placement in placements.chunks_exact(stride) {
            let id = placement.read_u32::<LE>()?;
            if placement.read_u32::<LE>()? & mask == 0 {
                continue;
            }
            let native = if stride == 60 {
                let number = placement.read_u32::<LE>()?;
                anyhow::ensure!(
                    number == u32::MAX || number & 0xffff < 1024,
                    "Invalid destructible index"
                );
                (number != u32::MAX).then_some(number)
            } else {
                None
            };
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
                    id: u32::try_from(instances.len())?,
                    native,
                    health_scale: if native.is_some() {
                        transform.matrix3.x_axis.length_squared()
                    } else {
                        1.0
                    },
                    shells_pass: false,
                    world_to_model,
                    bounds,
                    mesh,
                });
            }
        }
        let trees = trees::Trees::load(resources, geometry, &mut meshes, &mut instances)?;
        let index =
            (!instances.is_empty()).then(|| Box::new(CollisionIndex::build(&mut instances)));
        tracing::Span::current().record("output.obstacles.instances", instances.len());
        Ok(Self {
            meshes,
            instances,
            index,
            trees,
            physics: Default::default(),
        })
    }

    fn intersect(
        &self,
        origin: Vec3,
        direction: Vec3,
        range: f32,
        damage: &Damage,
    ) -> Option<ObstacleHit> {
        let mut nearest = None;
        self.index.as_ref()?.intersect(
            &self.instances,
            (&self.meshes, damage),
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
        let (header, input) = record_input(resources, &path)?;
        let stride = match &header {
            b"WCM1" => 40,
            b"WCM2" => 48,
            b"WCM3" => 52,
            b"WCM4" | b"WCM5" => 56,
            _ => anyhow::bail!("Invalid mesh format in {path}"),
        };
        let data = read_records(&path, input, stride, 1_000_000)?;
        let mut triangles = Vec::new();
        let mut bounds = Bounds::empty();
        for mut record in data.chunks_exact(stride) {
            let material = record.read_u32::<LE>()?;
            let pass_flags = if stride == 56 {
                record.read_u32::<LE>()?
            } else {
                0
            };
            let known_flags = (VEHICLES_PASS | SHELLS_PASS)
                | if &header == b"WCM5" {
                    RETAIN_VEHICLE_WALL
                } else {
                    0
                };
            anyhow::ensure!(
                pass_flags & !known_flags == 0,
                "Invalid collision flags in {path}"
            );
            let fragment = if stride >= 48 {
                let id = record.read_u32::<LE>()?;
                let health = record.read_u32::<LE>()?;
                anyhow::ensure!((id == 0) == (health == 0), "Invalid mesh destructible");
                let cascade = if stride >= 52 {
                    record.read_u32::<LE>()?
                } else {
                    0
                };
                if stride >= 52 {
                    anyhow::ensure!(
                        (matches!(id, 0..=2) || (FIRST_MODULE..FIRST_MODULE + 13).contains(&id))
                            && cascade & !CASCADE_MASK == 0
                            && (id >= FIRST_MODULE || cascade == 0),
                        "Invalid scenery cascade"
                    );
                }
                (id != 0).then_some((id, health, cascade))
            } else {
                None
            };
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
            triangles.push(Triangle {
                vertices,
                material,
                pass_flags,
                fragment,
            });
        }
        anyhow::ensure!(!triangles.is_empty(), "Missing collision triangles");
        Ok(Self {
            bounds,
            has_fragments: triangles.iter().any(|triangle| triangle.fragment.is_some()),
            triangles,
        })
    }
}

impl CollisionIndex {
    fn visit(&self, instances: &[Instance], query: Bounds, visitor: &mut impl FnMut(&Instance)) {
        self.visit_indexed(instances, query, 0, &mut |_, instance| visitor(instance));
    }

    fn visit_indexed(
        &self,
        instances: &[Instance],
        query: Bounds,
        offset: usize,
        visitor: &mut impl FnMut(usize, &Instance),
    ) {
        if !self.bounds.overlaps(query) {
            return;
        }
        if let Some([left, right]) = &self.children {
            let (left_instances, right_instances) = instances.split_at(left.count);
            left.visit_indexed(left_instances, query, offset, visitor);
            right.visit_indexed(right_instances, query, offset + left.count, visitor);
        } else {
            for (i, instance) in instances.iter().enumerate() {
                if instance.bounds.overlaps(query) {
                    visitor(offset + i, instance);
                }
            }
        }
    }

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
        geometry: (&[Mesh], &Damage),
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
            left.intersect(left_instances, geometry, origin, direction, range, nearest);
            right.intersect(right_instances, geometry, origin, direction, range, nearest);
            return;
        }

        for instance in instances {
            let limit = nearest.map_or(range, |hit| hit.distance);
            if instance.shells_pass || !instance.bounds.intersects(origin, direction, limit) {
                continue;
            }
            let local_origin = instance.world_to_model.transform_point3(origin);
            let local_direction = instance.world_to_model.transform_vector3(direction);
            let (meshes, damage) = geometry;
            for triangle in &meshes[instance.mesh].triangles {
                if triangle.pass_flags & SHELLS_PASS != 0 {
                    continue;
                }
                let fragment = triangle.fragment(instance);
                if fragment.is_some_and(|part| part.part == TREE_PART) || damage.destroyed(fragment)
                {
                    continue;
                }
                if let Some(distance) = triangle.intersect(local_origin, local_direction)
                    && distance < nearest.map_or(range, |hit| hit.distance)
                {
                    *nearest = Some(ObstacleHit {
                        distance,
                        material: triangle.material,
                        fragment,
                    });
                }
            }
        }
    }
}

impl Triangle {
    fn fragment(&self, instance: &Instance) -> Option<Fragment> {
        self.fragment.map(|part| instance.fragment(part))
    }
}

impl Instance {
    fn fragment(&self, (part, health, cascade): (u32, u32, u32)) -> Fragment {
        Fragment {
            object: self
                .native
                .map_or(Object::Placement(self.id), Object::Native),
            part,
            health: ((health as f32 * self.health_scale - 0.001).ceil() as u32).max(1),
            cascade,
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
    fn overlaps(self, other: Self) -> bool {
        self.min.cmple(other.max).all() && other.min.cmple(self.max).all()
    }
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

#[cfg(test)]
type TestTriangle = ([Vec3; 3], Option<(u32, u32)>);

#[cfg(test)]
impl Scenery {
    pub(super) fn test_mesh(triangles: Vec<TestTriangle>) -> Self {
        Self::test_numbered_mesh(None, triangles)
    }

    pub(super) fn test_numbered_mesh(native: Option<u32>, triangles: Vec<TestTriangle>) -> Self {
        let bounds = triangles
            .iter()
            .flat_map(|(vertices, _)| vertices)
            .fold(Bounds::empty(), |bounds, &v| {
                bounds.union(Bounds { min: v, max: v })
            });
        let mesh = Mesh {
            bounds,
            has_fragments: triangles.iter().any(|(_, fragment)| fragment.is_some()),
            triangles: triangles
                .into_iter()
                .map(|(vertices, fragment)| Triangle {
                    vertices,
                    material: 1,
                    pass_flags: 0,
                    fragment: fragment.map(|(part, health)| (part, health, 0)),
                })
                .collect(),
        };
        let mut instances = vec![Instance {
            id: 0,
            native,
            health_scale: 1.0,
            shells_pass: false,
            world_to_model: Affine3A::IDENTITY,
            bounds,
            mesh: 0,
        }];
        let index = Box::new(CollisionIndex::build(&mut instances));
        Self::new(Arc::new(Obstacles {
            meshes: vec![mesh],
            instances,
            index: Some(index),
            trees: Default::default(),
            physics: Default::default(),
        }))
    }

    pub(super) fn test_new_round(&self) -> Self {
        Self::new(self.geometry.clone())
    }
}

fn record_input(resources: &ResourceBundle, path: &str) -> Result<([u8; 4], impl Read)> {
    let mut input = BufReader::new(resources.read(path)?);
    let mut header = [0; 4];
    input.read_exact(&mut header)?;
    Ok((header, input))
}

fn read_records(path: &str, input: impl Read, stride: usize, limit: usize) -> Result<Vec<u8>> {
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
    #[ignore = "Requires WOT_RESOURCE_DIR with the extracted EU client bundle"]
    fn ensk_structure_breaks_its_numbered_dependants_once() {
        let path = std::env::var_os("WOT_RESOURCE_DIR").unwrap();
        let resources = ResourceBundle::open(std::path::Path::new(&path)).unwrap();
        let geometry = Arc::new(Obstacles::load(&resources, "spaces/06_ensk", 6).unwrap());
        let (origin, direction, fragment) = geometry
            .instances
            .iter()
            .filter(|instance| instance.native.is_some())
            .find_map(|instance| {
                let transform = instance.world_to_model.inverse();
                geometry.meshes[instance.mesh].triangles.iter().find_map(|triangle| {
                    let fragment = triangle.fragment(instance).filter(|part| part.cascade != 0)?;
                    let [a, b, c] = triangle.vertices.map(|point| transform.transform_point3(point));
                    let normal = (b - a).cross(c - a).try_normalize()?;
                    let origin = (a + b + c) / 3.0 + normal * 0.01;
                    let mut scene = Scenery::new(geometry.clone());
                    let masked = (fragment.object, fragment.part);
                    let hits_fragment = |scene: &Scenery| {
                        scene.intersect(origin, -normal, 0.02).and_then(|hit| hit.fragment)
                            .is_some_and(|part| (part.object, part.part) == masked)
                    };
                    if !hits_fragment(&scene) {
                        return None;
                    }
                    let broken = scene.damage(fragment, fragment.health);
                    assert!(!hits_fragment(&scene));
                    assert_eq!(broken.len(), 1 + fragment.cascade.count_ones() as usize);
                    for broken in broken {
                        assert!(matches!(fragment.object, Object::Native(id) if Some(id) == broken.native));
                        assert_eq!(scene.damage.0.get(&(fragment.object, broken.part)), Some(&0));
                        assert!(scene.damage(Fragment { part: broken.part, ..fragment }, u32::MAX).is_empty());
                    }
                    Some((origin, -normal, fragment))
                })
            })
            .expect("EU Ensk has collision fragments with native IDs and dependencies");
        let another_round = Scenery::new(geometry);
        assert_eq!(
            another_round
                .intersect(origin, direction, 0.02)
                .unwrap()
                .fragment
                .unwrap()
                .part,
            fragment.part
        );
    }

    #[test]
    fn ray_selects_the_first_transformed_surface_within_its_range() {
        let vertices = [Vec3::ZERO, Vec3::X * 2.0, Vec3::Y * 2.0];
        let bounds = Bounds {
            min: Vec3::ZERO,
            max: Vec3::new(2.0, 2.0, 0.0),
        };
        let mesh = |material| Mesh {
            bounds,
            has_fragments: false,
            triangles: vec![Triangle {
                vertices,
                material,
                pass_flags: 0,
                fragment: None,
            }],
        };
        let place = |x, z, mesh| {
            let transform = Affine3A::from_scale_rotation_translation(
                Vec3::new(2.0, 1.0, 0.5),
                glam::Quat::IDENTITY,
                Vec3::new(x, 0.0, z),
            );
            Instance {
                id: mesh as u32,
                native: None,
                health_scale: 1.0,
                shells_pass: false,
                world_to_model: transform.inverse(),
                bounds: bounds.transform(transform),
                mesh,
            }
        };
        let mut instances = vec![place(0.0, 20.0, 0), place(0.0, 5.0, 1)];
        instances.extend((0..8).map(|z| place(20.0, z as f32, 0)));
        let index = Box::new(CollisionIndex::build(&mut instances));
        let obstacles = Scenery::new(Arc::new(Obstacles {
            meshes: vec![mesh(40), mesh(111)],
            instances,
            index: Some(index),
            trees: Default::default(),
            physics: Default::default(),
        }));
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
