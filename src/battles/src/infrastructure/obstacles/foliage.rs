use super::*;
use crate::domain::spotting::Combatant;
use glam::Quat;

const CELL_METRES: f32 = 32.0;

struct Plant {
    native: Option<u32>,
    bounds: Bounds,
    model_to_world: Affine3A,
    world_to_model: Affine3A,
    bare: bool,
    low_crown: bool,
}

#[derive(Default)]
pub(super) struct Foliage {
    plants: Vec<Plant>,
    cells: HashMap<(i32, i32), Vec<usize>>,
}

impl Foliage {
    pub(super) fn load(resources: &ResourceBundle, geometry: &str) -> Result<Self> {
        let path = format!("{geometry}/foliage.bin");
        let available = resources.has_file(&path);
        tracing::Span::current().record("output.obstacles.foliage.available", available);
        if !available {
            tracing::Span::current().record("output.obstacles.foliage", 0);
            tracing::warn!(%geometry, "Foliage data is missing from the resource bundle. Re-extract client resources to enable vegetation concealment");
            return Ok(Self::default());
        }
        let (header, input) = record_input(resources, &path)?;
        anyhow::ensure!(&header == b"WCF1", "Invalid foliage format in {path}");
        let records = read_records(&path, input, 80, 100_000)?;
        let mut foliage = Self::default();
        for record in records.as_chunks::<80>().0 {
            let mut record = record.as_slice();
            let native = record.read_u32::<LE>()?;
            let native = (native != u32::MAX).then_some(native);
            let flags = record.read_u32::<LE>()?;
            let mut affine = [0.0; 12];
            let mut bounds = [0.0; 6];
            record.read_f32_into::<LE>(&mut affine)?;
            record.read_f32_into::<LE>(&mut bounds)?;
            let transform = Affine3A::from_cols_array(&affine);
            let [x0, y0, z0, x1, y1, z1] = bounds;
            let min = Vec3::new(x0, y0, z0);
            let max = Vec3::new(x1, y1, z1);
            let determinant = transform.matrix3.determinant();
            anyhow::ensure!(
                flags & !3 == 0
                    && transform.is_finite()
                    && determinant.is_finite()
                    && determinant.abs() > f32::EPSILON
                    && min.is_finite()
                    && max.is_finite()
                    && min.cmplt(max).all(),
                "Invalid foliage record in {path}"
            );
            let world_to_model = transform.inverse();
            let bounds = Bounds { min, max };
            let world = bounds.transform(transform);
            let foot = Vec3::from(transform.translation);
            let radius = (world.max - foot)
                .abs()
                .max((world.min - foot).abs())
                .length();
            anyhow::ensure!(
                radius.is_finite() && radius <= 150.0 && foot.abs().max_element() <= 32_000.0,
                "Foliage placement exceeds bounds in {path}"
            );
            let reach = if native.is_some() {
                Bounds {
                    min: foot - Vec3::splat(radius),
                    max: foot + Vec3::splat(radius),
                }
            } else {
                world
            };
            let index = foliage.plants.len();
            foliage.plants.push(Plant {
                native,
                bounds,
                model_to_world: transform,
                world_to_model,
                bare: flags & 1 != 0,
                low_crown: flags & 2 != 0,
            });
            let (left, bottom) = cell(reach.min);
            let (right, top) = cell(reach.max);
            for x in left..=right {
                for z in bottom..=top {
                    foliage.cells.entry((x, z)).or_default().push(index);
                }
            }
        }
        tracing::Span::current().record("output.obstacles.foliage", foliage.plants.len());
        Ok(foliage)
    }
}

impl Scenery {
    pub(in crate::infrastructure) fn record_fallen_crown(
        &mut self,
        native: u32,
        yaw: f32,
        pitch: f32,
    ) {
        let (sin, cos) = yaw.sin_cos();
        let (horizontal, vertical) = pitch.sin_cos();
        let axis = Vec3::new(sin * horizontal, vertical, cos * horizontal);
        self.fallen_crowns
            .insert(native, Quat::from_rotation_arc(Vec3::Y, axis));
    }

    pub(in crate::infrastructure) fn foliage_cover(
        &self,
        parameters: &BattleParameters,
        observer: &Combatant,
        target: &Combatant,
        origin: Vec3,
        direction: Vec3,
        range: f32,
    ) -> f32 {
        let end = origin + direction * range;
        let (left, bottom) = cell(origin.min(end));
        let (right, top) = cell(origin.max(end));
        let mut tested = HashSet::new();
        let mut ordinary = 0.0;
        let mut strongest_nearby: f32 = 0.0;
        let nearby = parameters.spotting_foliage_near_metres.powi(2);
        let foliage = &self.geometry.foliage;
        for x in left..=right {
            for z in bottom..=top {
                let Some(indices) = foliage.cells.get(&(x, z)) else {
                    continue;
                };
                for &index in indices {
                    let Some(plant) = tested
                        .insert(index)
                        .then(|| foliage.plants.get(index))
                        .flatten()
                    else {
                        continue;
                    };
                    let foot = Vec3::from(plant.model_to_world.translation);
                    if observer.position.distance_squared(foot) <= nearby {
                        continue;
                    }
                    let mut bounds = plant.bounds;
                    if !plant.low_crown {
                        let base = bounds.min.y.max(0.0);
                        bounds.min.y = base
                            + (bounds.max.y - base).max(0.0)
                                * parameters.spotting_crown_base_fraction;
                    }
                    let rotation = plant
                        .native
                        .and_then(|native| self.fallen_crowns.get(&native));
                    let (start, ray) = rotation.map_or((origin, direction), |rotation| {
                        let inverse = rotation.conjugate();
                        (foot + inverse * (origin - foot), inverse * direction)
                    });
                    if !bounds.intersects(
                        plant.world_to_model.transform_point3(start),
                        plant.world_to_model.transform_vector3(ray),
                        range,
                    ) {
                        continue;
                    }
                    let factor = if plant.bare {
                        parameters.spotting_leafless_cover
                    } else {
                        parameters.spotting_leafy_cover
                    };
                    if target.firing && target.position.distance_squared(foot) <= nearby {
                        strongest_nearby = strongest_nearby.max(factor);
                    } else {
                        ordinary += factor;
                    }
                    if ordinary >= parameters.spotting_foliage_cap {
                        return parameters.spotting_foliage_cap;
                    }
                }
            }
        }
        (ordinary + strongest_nearby * parameters.spotting_foliage_shot_fraction)
            .min(parameters.spotting_foliage_cap)
    }
}

fn cell(position: Vec3) -> (i32, i32) {
    (
        (position.x / CELL_METRES).floor() as i32,
        (position.z / CELL_METRES).floor() as i32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fallen_crown_adds_cover_at_its_new_position() {
        let native = 0x7c7a0019;
        let transform = Affine3A::from_translation(Vec3::new(94.0, 0.0, -8.0));
        let foliage = Foliage {
            plants: vec![Plant {
                native: Some(native),
                bounds: Bounds {
                    min: Vec3::new(-2.0, 0.0, -2.0),
                    max: Vec3::new(2.0, 12.0, 2.0),
                },
                model_to_world: transform,
                world_to_model: transform.inverse(),
                bare: false,
                low_crown: false,
            }],
            cells: HashMap::from([(cell(Vec3::X * 94.0), vec![0])]),
        };
        let mut scene = Scenery::new(Arc::new(Obstacles {
            foliage,
            ..Default::default()
        }));
        let combatant = |position| Combatant {
            entity: 0,
            team: 0,
            position,
            geometry: None,
            view_range: 0.0,
            radio_range: 0.0,
            concealment: 0.0,
            firing: false,
        };
        let observer = combatant(Vec3::ZERO);
        let mut target = combatant(Vec3::X * 100.0);
        let parameters = BattleParameters::default();
        let cover = |scene: &Scenery, target: &Combatant| {
            scene.foliage_cover(&parameters, &observer, target, Vec3::Y, Vec3::X, 100.0)
        };
        assert_eq!(cover(&scene, &target), 0.0);
        scene.record_fallen_crown(native, 0.0, std::f32::consts::FRAC_PI_2);
        assert_eq!(cover(&scene, &target), 0.5);
        target.firing = true;
        assert_eq!(cover(&scene, &target), 0.15);
    }

    #[test]
    fn firing_uses_only_the_strongest_nearby_plant_and_preserves_distant_cover() {
        let mut foliage = Foliage::default();
        for (x, bare) in [(10.0, false), (50.0, false), (89.0, false), (95.0, true)] {
            let transform = Affine3A::from_translation(Vec3::X * x);
            foliage
                .cells
                .entry(cell(Vec3::X * x))
                .or_default()
                .push(foliage.plants.len());
            foliage.plants.push(Plant {
                native: None,
                bounds: Bounds {
                    min: Vec3::splat(-1.0),
                    max: Vec3::ONE,
                },
                model_to_world: transform,
                world_to_model: transform.inverse(),
                bare,
                low_crown: true,
            });
        }
        let scene = Scenery::new(Arc::new(Obstacles {
            foliage,
            ..Default::default()
        }));
        let vehicle = |position, firing| Combatant {
            entity: 0,
            team: 0,
            position,
            firing,
            geometry: None,
            view_range: 0.0,
            radio_range: 0.0,
            concealment: 0.0,
        };
        let parameters = BattleParameters::default();
        let observer = vehicle(Vec3::ZERO, false);
        let standing = vehicle(Vec3::X * 100.0, false);
        let firing = vehicle(Vec3::X * 100.0, true);
        let cover = |target| {
            scene.foliage_cover(&parameters, &observer, target, Vec3::ZERO, Vec3::X, 100.0)
        };
        assert_eq!(cover(&standing), 0.8);
        assert!((cover(&firing) - 0.65).abs() < 0.0001);
    }
}
