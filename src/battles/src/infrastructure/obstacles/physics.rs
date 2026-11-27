use super::*;
use glam::{Mat3A, Quat};
use rapier3d::prelude::{
    ColliderBuilder, ColliderHandle, ColliderSet, PhysicsWorld, Pose, Rotation, SharedShape,
    TriMesh, TriMeshFlags, Vector,
};
use std::collections::{BTreeSet, hash_map::Entry};
use std::sync::Mutex;

const PAGE_WIDTH_METRES: f32 = 32.0;

struct ModelCollider {
    mesh: Arc<TriMesh>,
    fragment: Option<(u32, u32, u32)>,
    retain: bool,
}

struct PlacedModel {
    pose: Pose,
    variant: usize,
}

struct ModelVariant {
    linear: Mat3A,
    shapes: Mutex<Weak<Vec<SharedShape>>>,
}

impl ModelVariant {
    fn shapes(&self, parts: &[ModelCollider]) -> Arc<Vec<SharedShape>> {
        let mut cache = self
            .shapes
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(shapes) = cache.upgrade() {
            return shapes;
        }
        let scale = Vec3::new(
            self.linear.x_axis.x,
            self.linear.y_axis.y,
            self.linear.z_axis.z,
        );
        let uniform = self.linear == Mat3A::from_diagonal(scale)
            && scale.min_element() > 0.0
            && scale.max_element() - scale.min_element() <= scale.max_element() * 1.0e-5;
        let shapes = Arc::new(
            parts
                .iter()
                .map(|part| {
                    if self.linear == Mat3A::IDENTITY {
                        return SharedShape(part.mesh.clone());
                    }
                    let mut mesh = (*part.mesh).clone();
                    if uniform {
                        // Positive uniform scale preserves face normals and lets Parry scale its BVH directly.
                        return SharedShape::new(mesh.scaled(Vector::from_array(scale.to_array())));
                    }
                    mesh.update_vertices(|vertices| {
                        for vertex in vertices {
                            *vertex = Vector::from_array(
                                (self.linear * Vec3::from_array(vertex.to_array())).to_array(),
                            );
                        }
                    });
                    SharedShape::new(mesh)
                })
                .collect(),
        );
        *cache = Arc::downgrade(&shapes);
        shapes
    }
}

pub(super) struct PhysicsGeometry {
    models: Vec<Vec<ModelCollider>>,
    placements: Vec<PlacedModel>,
    variants: Vec<ModelVariant>,
}

struct ActivePlacement {
    handles: Vec<ColliderHandle>,
    // Keep shared transformed shapes alive while any arena uses this placement variant.
    _shapes: Arc<Vec<SharedShape>>,
}

pub(in crate::infrastructure) struct SceneryColliders {
    geometry: Arc<PhysicsGeometry>,
    fragments: HashMap<ColliderHandle, Option<Fragment>>,
    placements: BTreeMap<usize, ActivePlacement>,
    pages: Vec<[i32; 4]>,
    next_pages: Vec<[i32; 4]>,
    broken_count: usize,
}

impl SceneryColliders {
    pub(in crate::infrastructure) fn contains(&self, handle: ColliderHandle) -> bool {
        self.fragments.contains_key(&handle)
    }

    pub(in crate::infrastructure) fn fragment(&self, handle: ColliderHandle) -> Option<Fragment> {
        self.fragments.get(&handle).copied().flatten()
    }

    pub(in crate::infrastructure) fn sync(&mut self, scene: &Scenery, colliders: &mut ColliderSet) {
        if self.broken_count == scene.broken_count {
            return;
        }
        // A single native fragment may span several placements and several collision layers.
        for (&index, active) in &self.placements {
            let instance = &scene.geometry.instances[index];
            for (&handle, part) in active
                .handles
                .iter()
                .zip(&self.geometry.models[instance.mesh])
            {
                if scene.damage.destroyed(self.fragment(handle)) {
                    if part.retain {
                        self.fragments.insert(handle, None);
                    } else {
                        self.fragments.remove(&handle);
                        if let Some(collider) = colliders.get_mut(handle) {
                            collider.set_enabled(false);
                        }
                    }
                }
            }
        }
        self.broken_count = scene.broken_count;
    }

    pub(in crate::infrastructure) fn update_near(
        &mut self,
        world: &mut PhysicsWorld,
        scene: &Scenery,
        regions: impl IntoIterator<Item = (Vec3, Vec3)>,
    ) {
        let Some(index) = &scene.geometry.index else {
            return;
        };
        self.next_pages.clear();
        self.next_pages
            .extend(regions.into_iter().map(|(min, max)| {
                [min.x, min.z, max.x, max.z].map(|value| (value / PAGE_WIDTH_METRES).floor() as i32)
            }));
        self.next_pages.sort_unstable();
        self.next_pages.dedup();
        if self.next_pages == self.pages {
            return;
        }
        std::mem::swap(&mut self.pages, &mut self.next_pages);
        let mut needed = BTreeSet::new();
        for &[x0, z0, x1, z1] in &self.pages {
            let area = Bounds {
                min: Vec3::new(
                    x0 as f32 * PAGE_WIDTH_METRES,
                    index.bounds.min.y,
                    z0 as f32 * PAGE_WIDTH_METRES,
                ),
                max: Vec3::new(
                    (x1 as f32 + 1.0) * PAGE_WIDTH_METRES,
                    index.bounds.max.y,
                    (z1 as f32 + 1.0) * PAGE_WIDTH_METRES,
                ),
            };
            index.visit_indexed(&scene.geometry.instances, area, 0, &mut |slot, _| {
                needed.insert(slot);
            });
        }
        let mut changed = false;
        self.placements.retain(|slot, active| {
            if needed.contains(slot) {
                return true;
            }
            for handle in &active.handles {
                self.fragments.remove(handle);
                world.remove_collider(*handle);
                changed = true;
            }
            false
        });
        for slot in needed {
            let std::collections::btree_map::Entry::Vacant(entry) = self.placements.entry(slot)
            else {
                continue;
            };
            let instance = &scene.geometry.instances[slot];
            let placement = &self.geometry.placements[slot];
            let parts = &self.geometry.models[instance.mesh];
            let shapes = self.geometry.variants[placement.variant].shapes(parts);
            let mut handles = Vec::with_capacity(parts.len());
            for (part, shape) in parts.iter().zip(shapes.iter()) {
                let fragment = part.fragment.map(|part| instance.fragment(part));
                let destroyed = scene.damage.destroyed(fragment);
                let enabled = !destroyed || part.retain;
                let collider = ColliderBuilder::new(shape.clone())
                    .position(placement.pose)
                    .enabled(enabled);
                let handle = world.colliders.insert(collider);
                if enabled {
                    self.fragments
                        .insert(handle, fragment.filter(|_| !destroyed));
                }
                handles.push(handle);
                changed = true;
            }
            entry.insert(ActivePlacement {
                handles,
                _shapes: shapes,
            });
        }
        if changed {
            world.detect_collisions(&(), &());
        }
    }
}

impl Scenery {
    pub(in crate::infrastructure) fn physics_colliders(&self) -> Result<SceneryColliders> {
        let geometry = self
            .geometry
            .physics
            .get_or_init(|| {
                self.geometry
                    .prepare_physics()
                    .map(Arc::new)
                    .map_err(|error| error.to_string())
            })
            .as_ref()
            .map_err(|error| anyhow::anyhow!("{error}"))?
            .clone();
        Ok(SceneryColliders {
            geometry,
            fragments: HashMap::new(),
            placements: BTreeMap::new(),
            pages: Vec::new(),
            next_pages: Vec::new(),
            broken_count: self.broken_count,
        })
    }
}

impl Obstacles {
    fn prepare_physics(&self) -> Result<PhysicsGeometry> {
        let models: Vec<_> = self
            .meshes
            .iter()
            .map(Mesh::physics_colliders)
            .collect::<Result<_>>()?;
        let mut keys = HashMap::new();
        let mut variants = Vec::new();
        let mut placements = Vec::with_capacity(self.instances.len());
        let mut colliders = 0;
        for instance in &self.instances {
            let (pose, linear) = physics_transform(instance.world_to_model.inverse())?;
            let key = (instance.mesh, linear.to_cols_array().map(f32::to_bits));
            let variant = match keys.entry(key) {
                Entry::Occupied(entry) => *entry.get(),
                Entry::Vacant(entry) => {
                    let slot = variants.len();
                    variants.push(ModelVariant {
                        linear,
                        shapes: Mutex::default(),
                    });
                    *entry.insert(slot)
                }
            };
            colliders += models[instance.mesh].len();
            anyhow::ensure!(colliders <= 600_000, "Map exceeds physics collider limit");
            placements.push(PlacedModel { pose, variant });
        }
        Ok(PhysicsGeometry {
            models,
            placements,
            variants,
        })
    }
}

// Keep rotation and translation in the instance pose so repeated map models can share a shape.
// Scale and shear belong in the mesh because Rapier collider poses are rigid transforms.
fn physics_transform(transform: Affine3A) -> Result<(Pose, Mat3A)> {
    let matrix = transform.matrix3;
    let x = matrix.x_axis.normalize_or_zero();
    let z = x.cross(matrix.y_axis).normalize_or_zero();
    let y = z.cross(x);
    anyhow::ensure!(
        x.is_normalized() && z.is_normalized() && transform.is_finite(),
        "Invalid scenery transform"
    );
    let axes = Mat3A::from_cols(x, y, z);
    let rotation = Quat::from_mat3a(&axes).normalize();
    let linear = axes.transpose() * matrix;
    let scale = linear.to_cols_array().map(|value| {
        // Discard decomposition noise on unit axes, so rigid placements of a model share its BVH.
        for exact in [-1.0, 0.0, 1.0] {
            if (value - exact).abs() < 0.000001 {
                return exact;
            }
        }
        value
    });
    let pose = Pose::from_parts(
        Vector::from_array(transform.translation.to_array()),
        Rotation::from_array(rotation.to_array()),
    );
    Ok((pose, Mat3A::from_cols_array(&scale)))
}

impl Mesh {
    fn physics_colliders(&self) -> Result<Vec<ModelCollider>> {
        let mut groups = BTreeMap::<_, Vec<[Vector; 3]>>::new();
        for triangle in &self.triangles {
            if triangle.pass_flags & VEHICLES_PASS != 0 {
                continue;
            }
            let vertices = triangle.vertices.map(|v| Vector::from_array(v.to_array()));
            let [a, b, c] = vertices;
            let normal = (b - a).cross(c - a);
            anyhow::ensure!(normal.is_finite(), "Invalid scenery collision surface");
            if normal.length_squared() > 0.0 {
                let retain = triangle.pass_flags & RETAIN_VEHICLE_WALL != 0;
                groups
                    .entry((triangle.fragment, retain))
                    .or_default()
                    .push(vertices);
            }
        }
        groups
            .into_iter()
            .map(|((fragment, retain), triangles)| {
                let indices = (0..u32::try_from(triangles.len())?)
                    .map(|i| [3 * i, 3 * i + 1, 3 * i + 2])
                    .collect();
                let vertices = triangles.into_iter().flatten().collect();
                let mesh = TriMesh::with_flags(
                    vertices,
                    indices,
                    TriMeshFlags::FIX_INTERNAL_EDGES_TWO_SIDED
                        | TriMeshFlags::DELETE_DUPLICATE_TRIANGLES,
                )?;
                Ok(ModelCollider {
                    mesh: Arc::new(mesh),
                    fragment,
                    retain,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rapier3d::prelude::Ray;

    #[test]
    fn physics_placements_preserve_affine_scale_mirrors_and_shear() {
        let linear = Mat3A::from_cols_array(&[-2.0, 0.1, 0.0, 0.6, 1.8, 0.2, 0.4, 0.0, 1.3]);
        let transform = Affine3A {
            matrix3: linear,
            translation: glam::Vec3A::new(20.0, 7.0, -3.0),
        };
        let (pose, shape_transform) = physics_transform(transform).unwrap();
        for vertex in [Vec3::X, Vec3::Y, Vec3::Z, Vec3::new(-7.0, 12.0, 14.0)] {
            let actual =
                pose.transform_point(Vector::from_array((shape_transform * vertex).to_array()));
            let expected = transform.transform_point3(vertex);
            assert!(expected.abs_diff_eq(Vec3::from_array(actual.to_array()), 0.00001));
        }
    }

    #[test]
    fn physics_filters_and_fracture_cascades_follow_the_arena_state() {
        let face = |x| {
            [
                Vec3::new(x, 0., 0.),
                Vec3::new(x, 2., 0.),
                Vec3::new(x, 0., 2.),
            ]
        };
        let mut scene = Scenery::test_mesh(vec![
            (face(1.), None),
            (face(2.), Some((FIRST_MODULE, 10))),
            (face(3.), Some((FIRST_MODULE, 10))),
            (face(4.), Some((FIRST_MODULE + 1, 10))),
            (face(5.), None),
        ]);
        let geometry = Arc::get_mut(&mut scene.geometry).unwrap();
        let triangles = &mut geometry.meshes[0].triangles;
        triangles[0].pass_flags = VEHICLES_PASS;
        triangles[1].fragment = Some((FIRST_MODULE, 10, 2));
        triangles[2].fragment = Some((FIRST_MODULE, 10, 2));
        triangles[2].pass_flags = RETAIN_VEHICLE_WALL | SHELLS_PASS;
        let fresh = scene.test_new_round();
        let mut world = PhysicsWorld::default();
        let mut physics = scene.physics_colliders().unwrap();
        let nearby = [(Vec3::ZERO, Vec3::splat(10.0))];
        physics.update_near(&mut world, &scene, nearby);
        let ray = Ray::new(Vector::new(0., 0.5, 0.5), Vector::X);
        let hits = |colliders: &ColliderSet| {
            let mut hits: Vec<_> = colliders
                .iter()
                .filter(|(_, c)| c.is_enabled())
                .filter_map(|(h, c)| {
                    c.shape()
                        .cast_ray(c.position(), &ray, 10., false)
                        .map(|distance| (distance, h))
                })
                .collect();
            hits.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
            hits
        };
        let before = hits(&world.colliders);
        assert_eq!(
            before.iter().map(|hit| hit.0).collect::<Vec<_>>(),
            [2., 3., 4., 5.]
        );
        let part = physics.fragment(before[0].1).unwrap();
        assert_eq!(scene.damage(part, 10).len(), 2);
        physics.sync(&scene, &mut world.colliders);
        let after = hits(&world.colliders);
        assert_eq!(after.iter().map(|hit| hit.0).collect::<Vec<_>>(), [3., 5.]);
        assert!(physics.contains(after[0].1) && physics.fragment(after[0].1).is_none());

        let mut other = PhysicsWorld::default();
        fresh
            .physics_colliders()
            .unwrap()
            .update_near(&mut other, &fresh, nearby);
        assert_eq!(hits(&other.colliders).len(), 4);
        physics.update_near(&mut world, &scene, []);
        assert!(world.colliders.is_empty());
        physics.update_near(&mut world, &scene, nearby);
        assert_eq!(hits(&world.colliders).len(), 2);
    }

    #[test]
    #[ignore = "Requires WOT_RESOURCE_DIR with the extracted EU Serene Coast map"]
    fn physics_serene_coast_ray_hits_deck_or_ground_from_below() {
        let path = std::env::var_os("WOT_RESOURCE_DIR").unwrap();
        let resources = ResourceBundle::open(std::path::Path::new(&path)).unwrap();
        let name = "spaces/47_canada_a";
        let scene = Scenery::new(Arc::new(Obstacles::load(&resources, name, 47).unwrap()));
        let terrain = terrain::Terrain::load(&resources, name).unwrap();
        let mut world = PhysicsWorld::default();
        terrain.insert_physics_colliders(&mut world.colliders);
        let mut scenery = scene.physics_colliders().unwrap();
        let point = Vec2::new(-143.73, 102.71);
        let center = Vec3::new(point.x, -5.47, point.y);
        scenery.update_near(
            &mut world,
            &scene,
            [(center - Vec3::splat(6.0), center + Vec3::splat(6.0))],
        );
        let floor = terrain.height_at(point).unwrap();
        let deck = scene
            .support_height(point, -5.47 - 0.7, -5.47 + 0.7)
            .unwrap();
        for (start, expected, on_bridge) in
            [(deck + 0.15, deck, true), (floor + 0.15, floor, false)]
        {
            let ray = Ray::new(Vector::new(point.x, start, point.y), -Vector::Y);
            let (distance, handle) = world
                .colliders
                .iter()
                .filter_map(|(handle, c)| {
                    c.shape()
                        .cast_ray(c.position(), &ray, 1., false)
                        .map(|distance| (distance, handle))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .unwrap();
            assert!((start - distance - expected).abs() < 0.001);
            assert_eq!(scenery.contains(handle), on_bridge);
        }
    }
    #[test]
    #[ignore = "Local release benchmark with WOT_RESOURCE_DIR and the EU Serene Coast map"]
    fn measure_scenery_pages_for_clustered_and_spread_hulls() {
        let path = std::env::var_os("WOT_RESOURCE_DIR").unwrap();
        let resources = ResourceBundle::open(std::path::Path::new(&path)).unwrap();
        let scene = Scenery::new(Arc::new(
            Obstacles::load(&resources, "spaces/47_canada_a", 47).unwrap(),
        ));
        let index = scene.geometry.index.as_ref().unwrap();
        let radius = Vec3::splat(10.0);
        // Pick the busiest separated neighborhoods within the playable area by placed triangle count.
        let mut centers = Vec::new();
        for x in -7..=7 {
            for z in -7..=7 {
                let center = Vec3::new((x * 64) as f32, 0.0, (z * 64) as f32);
                let reach = Vec3::new(32.0, 1000.0, 32.0);
                let mut triangles = 0;
                index.visit(
                    &scene.geometry.instances,
                    Bounds {
                        min: center - reach,
                        max: center + reach,
                    },
                    &mut |instance| {
                        triangles += scene.geometry.meshes[instance.mesh].triangles.len();
                    },
                );
                centers.push((triangles, center));
            }
        }
        centers.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        for spread in [false, true] {
            let mut world = PhysicsWorld::default();
            let mut geometry = scene.physics_colliders().unwrap();
            let starts: Vec<_> = centers
                .iter()
                .take(30)
                .enumerate()
                .map(|(i, (_, center))| {
                    if spread {
                        *center
                    } else {
                        centers[0].1 + Vec3::new((i % 6) as f32 * 4.0, 0., (i / 6) as f32 * 4.0)
                    }
                })
                .collect();
            let mut timings = Vec::new();
            let mut peak = 0;
            for step in 0..301 {
                let offset = Vec3::X * (step as f32 * 0.5);
                let now = Instant::now();
                geometry.update_near(
                    &mut world,
                    &scene,
                    starts
                        .iter()
                        .map(|start| (*start + offset - radius, *start + offset + radius)),
                );
                timings.push(now.elapsed());
                peak = peak.max(world.colliders.len());
            }
            let cold = timings.remove(0);
            timings.sort_unstable();
            eprintln!(
                "spread={spread}, cold={cold:?}, p95={:?}, max={:?}, peak active colliders={peak}",
                timings[284], timings[299]
            );
            assert!(peak > 0);
            geometry.update_near(&mut world, &scene, []);
            assert!(world.colliders.is_empty());
        }
    }
}
