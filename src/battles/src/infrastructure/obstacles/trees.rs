use super::*;
use glam::Vec3Swizzles;
use std::f32::consts::{PI, TAU};

pub(in crate::infrastructure) const TREE_PART: u32 = 3;
const TRUNK_RADIUS: f32 = 0.3;
const TRUNK_SIDES: usize = 6;
const LIMIT_STEP: f32 = PI / 90.0;
const MAX_LIMIT: f32 = PI * 0.75;

struct Tree {
    foot: Vec3,
    height: f32,
}

#[derive(Default)]
pub(super) struct Trees(HashMap<u32, Tree>);

impl Trees {
    pub(super) fn load(
        resources: &ResourceBundle,
        geometry: &str,
        meshes: &mut Vec<Mesh>,
        instances: &mut Vec<Instance>,
    ) -> Result<Self> {
        let path = format!("{geometry}/trees.bin");
        let mut trees = HashMap::new();
        if !resources.has_file(&path) {
            return Ok(Self(trees));
        }
        let (header, input) = record_input(resources, &path)?;
        anyhow::ensure!(&header == b"WCT1", "Invalid tree format in {path}");
        let data = read_records(&path, input, 28, 100_000)?;
        let trunk = trunk();
        let mesh = meshes.len();
        for record in data.as_chunks::<28>().0 {
            let mut record = record.as_slice();
            let native = record.read_u32::<LE>()?;
            let health = record.read_u32::<LE>()?;
            let mut values = [0.0; 5];
            record.read_f32_into::<LE>(&mut values)?;
            let [x, y, z, scale, height] = values;
            anyhow::ensure!(
                native & 0xffff < 1024
                    && health > 0
                    && values.iter().all(|value| value.is_finite())
                    && (0.01..=100.0).contains(&scale)
                    && (0.01..=500.0).contains(&height),
                "Invalid tree record in {path}"
            );
            let foot = Vec3::new(x, y, z);
            let transform = Affine3A::from_scale_rotation_translation(
                Vec3::new(scale, 1.0, scale),
                glam::Quat::IDENTITY,
                foot,
            );
            anyhow::ensure!(
                trees.insert(native, Tree { foot, height }).is_none(),
                "Duplicate tree in {path}"
            );
            instances.push(Instance {
                id: u32::try_from(instances.len())?,
                native: Some(native),
                health_scale: health as f32,
                shells_pass: true,
                world_to_model: transform.inverse(),
                bounds: trunk.bounds.transform(transform),
                mesh,
            });
        }
        meshes.push(trunk);
        tracing::Span::current().record("output.obstacles.trees", trees.len());
        Ok(Self(trees))
    }
}

impl Scenery {
    #[cfg(test)]
    pub(in crate::infrastructure) fn test_register_tree(&mut self, native: u32, foot: Vec3) {
        Arc::get_mut(&mut self.geometry)
            .unwrap()
            .trees
            .0
            .insert(native, Tree { foot, height: 10.0 });
    }

    pub(in crate::infrastructure) fn tree_fall_pitch(
        &self,
        ground: &terrain::Terrain,
        native: u32,
        yaw: f32,
    ) -> Option<f32> {
        let tree = self.geometry.trees.0.get(&native)?;
        let (sin, cos) = yaw.sin_cos();
        Some(fall_limit(|pitch| {
            let (horizontal, vertical) = pitch.sin_cos();
            let direction = Vec3::new(sin * horizontal, vertical, cos * horizontal);
            let top = tree.foot + direction * tree.height;
            let ground_y = ground.height_at(top.xz()).unwrap_or(tree.foot.y);
            if top.y <= ground_y {
                return true;
            }
            let clearance = tree.height.min(1.0);
            self.intersect(
                tree.foot + direction * clearance,
                direction,
                tree.height - clearance,
            )
            .is_some()
        }))
    }
}

fn fall_limit(mut blocked: impl FnMut(f32) -> bool) -> f32 {
    let mut clear = 0.0;
    let mut pitch = LIMIT_STEP;
    while pitch <= MAX_LIMIT {
        if blocked(pitch) {
            for _ in 0..6 {
                let mid = (clear + pitch) * 0.5;
                if blocked(mid) {
                    pitch = mid;
                } else {
                    clear = mid;
                }
            }
            return pitch;
        }
        clear = pitch;
        pitch = (pitch + LIMIT_STEP).min(MAX_LIMIT);
        if pitch == clear {
            break;
        }
    }
    MAX_LIMIT
}

fn trunk() -> Mesh {
    // Standing trunks have vehicle contact surfaces. The native client owns the fall animation.
    let corner = |side: usize, y| {
        let (sin, cos) = (side as f32 * TAU / TRUNK_SIDES as f32).sin_cos();
        Vec3::new(TRUNK_RADIUS * sin, y, TRUNK_RADIUS * cos)
    };
    let triangles = (0..TRUNK_SIDES)
        .flat_map(|side| {
            let a = corner(side, -1.0);
            let b = corner(side + 1, -1.0);
            let c = corner(side + 1, 4.0);
            let d = corner(side, 4.0);
            [[a, b, c], [a, c, d]].map(|vertices| Triangle {
                vertices,
                material: 71,
                pass_flags: SHELLS_PASS,
                fragment: Some((TREE_PART, 1, 0)),
            })
        })
        .collect();
    Mesh {
        has_fragments: true,
        bounds: Bounds {
            min: Vec3::new(-TRUNK_RADIUS, -1.0, -TRUNK_RADIUS),
            max: Vec3::new(TRUNK_RADIUS, 4.0, TRUNK_RADIUS),
        },
        triangles,
    }
}

impl Scenery {
    pub(in crate::infrastructure) fn blast_trees(
        &mut self,
        center: Vec3,
        radius: f32,
        damage: f32,
    ) -> Vec<Breakage> {
        let mut hits = Vec::new();
        if radius <= 0.0 || damage <= 0.0 {
            return Vec::new();
        }
        let geometry = &self.geometry;
        let bounds = Bounds {
            min: center - Vec3::splat(radius),
            max: center + Vec3::splat(radius),
        };
        if let Some(index) = &geometry.index {
            index.visit(&geometry.instances, bounds, &mut |instance| {
                let Some(native) = instance.native else {
                    return;
                };
                let Some(tree) = geometry.trees.0.get(&native) else {
                    return;
                };
                let closest = tree
                    .foot
                    .with_y(center.y.clamp(tree.foot.y, tree.foot.y + tree.height));
                let distance = center.distance(closest);
                if distance < radius {
                    let fragment = Fragment {
                        object: Object::Native(native),
                        part: TREE_PART,
                        health: instance.health_scale as u32,
                        cascade: 0,
                    };
                    hits.push((distance, native, fragment));
                }
            });
        }
        hits.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        hits.into_iter()
            .take(64)
            .flat_map(|(distance, _, fragment)| {
                self.damage(
                    fragment,
                    (damage * (1.0 - distance / radius)).round() as u32,
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pitch_stops_at_first_obstruction_and_allows_downhill_falls() {
        use std::f32::consts::FRAC_PI_2;
        for boundary in [0.5_f32, FRAC_PI_2, 2.0] {
            let pitch = fall_limit(|pitch| pitch >= boundary);
            assert!((boundary..boundary + 0.001).contains(&pitch));
        }
        assert_eq!(fall_limit(|_| false), MAX_LIMIT);
    }
}
