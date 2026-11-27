use super::parameters::BattleParameters;
use glam::Vec3;
use std::collections::{HashMap, HashSet};
use wot_shared_kernel::VehicleSpotting;

#[derive(Clone, Copy)]
pub(crate) struct SightGeometry {
    pub checkpoints: [Vec3; 6],
    pub ports: [Vec3; 2],
}

pub(crate) fn local_checkpoints(profile: &VehicleSpotting) -> [Vec3; 6] {
    let [min, max] = profile.hull_bounds;
    let mount = profile.turret_offset + profile.gun_mount;
    let top = max.y.max(profile.turret_offset.y + profile.turret_top);
    let middle = min.midpoint(max);
    [
        Vec3::Y * (profile.hull_offset.y + top),
        profile.hull_offset + mount,
        profile.hull_offset + Vec3::new(0.0, middle.y, max.z),
        profile.hull_offset + Vec3::new(0.0, middle.y, min.z),
        profile.hull_offset + Vec3::new(max.x, mount.y, middle.z),
        profile.hull_offset + Vec3::new(min.x, mount.y, middle.z),
    ]
}

pub(crate) struct Combatant {
    pub entity: u32,
    pub team: u8,
    pub position: Vec3,
    pub geometry: Option<SightGeometry>,
    pub view_range: f32,
    pub radio_range: f32,
    pub concealment: f32,
    pub firing: bool,
}

#[derive(Default)]
pub(crate) struct Spotting {
    checks: HashMap<(u32, u32), Check>,
    contacts: HashMap<u32, Contact>,
    radio_groups: HashMap<u32, u32>,
}

#[derive(Default)]
struct Check {
    last_tick: Option<u32>,
    detected: bool,
}

#[derive(Default)]
struct Contact {
    reporters: Vec<u32>,
    retention_until: Option<u32>,
    active: bool,
    visible_since: Option<u32>,
    alerted: bool,
}

#[derive(Default)]
pub(crate) struct SpottingChanges {
    pub alerts: Vec<u32>,
    pub newly_spotted: Vec<u32>,
    pub went_dark: Vec<u32>,
    pub checked_pairs: usize,
    pub vision_rays: usize,
    pub radio_groups: usize,
}

impl Spotting {
    pub fn update(
        &mut self,
        parameters: &BattleParameters,
        tick: u32,
        vehicles: &[Combatant],
        mut sight: impl FnMut(&Combatant, &Combatant, Vec3, Vec3) -> Option<f32>,
    ) -> SpottingChanges {
        let mut changes = SpottingChanges::default();
        let (groups, count) = radio_groups(vehicles);
        self.radio_groups = groups;
        changes.radio_groups = count;
        self.checks.retain(|(observer, target), _| {
            self.radio_groups.contains_key(observer) && self.radio_groups.contains_key(target)
        });
        self.contacts
            .retain(|target, _| self.radio_groups.contains_key(target));
        let port = usize::from(!(tick / parameters.spotting_port_ticks).is_multiple_of(2));
        for target in vehicles {
            let mut reporters = Vec::new();
            for observer in vehicles
                .iter()
                .filter(|observer| observer.team != target.team)
            {
                let distance = observer.position.distance_squared(target.position);
                let pair = (observer.entity, target.entity);
                if distance > parameters.spotting_max_metres.powi(2) {
                    self.checks.remove(&pair);
                    continue;
                }
                let check = self.checks.entry(pair).or_default();
                let interval = check_interval(parameters, distance);
                if check
                    .last_tick
                    .is_none_or(|last| tick.saturating_sub(last) >= interval)
                {
                    check.last_tick = Some(tick);
                    changes.checked_pairs += 1;
                    check.detected = distance <= parameters.spotting_proximity_metres.powi(2)
                        || observer.geometry.zip(target.geometry).is_some_and(
                            |(source, target_geometry)| {
                                let origin = source.ports[port];
                                let view = observer.view_range.clamp(
                                    parameters.spotting_proximity_metres,
                                    parameters.spotting_max_metres,
                                );
                                if distance
                                    > spotting_range(
                                        view,
                                        parameters.spotting_proximity_metres,
                                        target.concealment,
                                    )
                                    .powi(2)
                                {
                                    return false;
                                }
                                target_geometry.checkpoints.into_iter().any(|checkpoint| {
                                    changes.vision_rays += 1;
                                    sight(observer, target, origin, checkpoint).is_some_and(
                                        |foliage| {
                                            let concealment =
                                                (target.concealment + foliage).clamp(0.0, 1.0);
                                            distance
                                                <= spotting_range(
                                                    view,
                                                    parameters.spotting_proximity_metres,
                                                    concealment,
                                                )
                                                .powi(2)
                                        },
                                    )
                                })
                            },
                        );
                }
                if check.detected {
                    reporters.push(observer.entity);
                }
            }
            let contact = self.contacts.entry(target.entity).or_default();
            if !reporters.is_empty() {
                contact.reporters = reporters;
                contact.active = true;
                contact.retention_until = None;
            } else if contact.active {
                contact.active = false;
                contact.retention_until =
                    Some(tick.saturating_add(parameters.spotting_retention_ticks));
            }
            if contact.retention_until.is_some_and(|until| tick >= until) {
                contact.reporters.clear();
                contact.retention_until = None;
            }
            contact
                .reporters
                .retain(|source| self.radio_groups.contains_key(source));
            if contact.reporters.is_empty() {
                if contact.visible_since.take().is_some() {
                    changes.went_dark.push(target.entity);
                }
                contact.alerted = false;
            } else {
                if contact.visible_since.is_none() {
                    changes.newly_spotted.push(target.entity);
                }
                let since = *contact.visible_since.get_or_insert(tick);
                if !contact.alerted
                    && tick.saturating_sub(since) >= parameters.spotting_sixth_sense_ticks
                {
                    contact.alerted = true;
                    changes.alerts.push(target.entity);
                }
            }
        }
        changes
    }

    pub fn linked(&self, a: u32, b: u32) -> bool {
        self.radio_groups
            .get(&a)
            .is_some_and(|group| self.radio_groups.get(&b) == Some(group))
    }

    pub fn knows_enemy(&self, viewer: u32, target: u32) -> bool {
        self.contacts.get(&target).is_some_and(|contact| {
            contact
                .reporters
                .iter()
                .any(|&source| self.linked(viewer, source))
        })
    }
}

fn spotting_range(view: f32, proximity: f32, concealment: f32) -> f32 {
    view - (view - proximity) * concealment
}

fn check_interval(parameters: &BattleParameters, distance_squared: f32) -> u32 {
    if distance_squared <= parameters.spotting_proximity_metres.powi(2) {
        parameters.spotting_proximity_ticks
    } else if distance_squared <= parameters.spotting_near_metres.powi(2) {
        parameters.spotting_near_ticks
    } else if distance_squared <= parameters.spotting_middle_metres.powi(2) {
        parameters.spotting_middle_ticks
    } else {
        parameters.spotting_far_ticks
    }
}

fn radio_groups(vehicles: &[Combatant]) -> (HashMap<u32, u32>, usize) {
    let mut groups = HashMap::with_capacity(vehicles.len());
    let mut assigned = HashSet::with_capacity(vehicles.len());
    let mut count = 0;
    for seed in vehicles {
        if !assigned.insert(seed.entity) {
            continue;
        }
        count += 1;
        let mut frontier = vec![seed];
        while let Some(neighbor) = frontier.pop() {
            groups.insert(neighbor.entity, seed.entity);
            for candidate in vehicles
                .iter()
                .filter(|vehicle| vehicle.team == neighbor.team)
            {
                if !assigned.contains(&candidate.entity)
                    && candidate.position.distance_squared(neighbor.position)
                        <= (candidate.radio_range + neighbor.radio_range).powi(2)
                {
                    assigned.insert(candidate.entity);
                    frontier.push(candidate);
                }
            }
        }
    }
    (groups, count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vehicle(entity: u32, team: u8, position: Vec3) -> Combatant {
        Combatant {
            entity,
            team,
            position,
            geometry: Some(SightGeometry {
                checkpoints: [position + Vec3::Y; 6],
                ports: [position + Vec3::Y, position + Vec3::Y * 2.0],
            }),
            view_range: 400.0,
            radio_range: 80.0,
            concealment: 0.1,
            firing: false,
        }
    }

    #[test]
    fn proximity_is_three_dimensional_and_bypasses_geometry() {
        let mut state = Spotting::default();
        let parameters = BattleParameters::default();
        let observer = vehicle(1, 1, Vec3::ZERO);
        let mut targets = [observer, vehicle(2, 2, Vec3::new(0.0, 30.0, 45.0))];
        let results = state.update(&parameters, 0, &targets, |_, _, _, _| None);
        assert_eq!(results.vision_rays, 12);
        assert!(!state.knows_enemy(1, 2));
        targets[1].position.y = 0.0;
        let results = state.update(&parameters, 1, &targets, |_, _, _, _| {
            panic!("proximity uses no ray")
        });
        assert_eq!(results.vision_rays, 0);
        assert!(state.knows_enemy(1, 2));
    }

    #[test]
    fn a_stationary_vehicle_can_be_revealed_by_firing_and_then_go_dark() {
        let mut state = Spotting::default();
        let parameters = BattleParameters::default();
        let mut vehicles = [vehicle(1, 1, Vec3::ZERO), vehicle(2, 2, Vec3::X * 305.0)];
        vehicles[0].view_range = 360.0;
        let still = 0.388 * 0.5;
        vehicles[1].concealment = still;
        let uncovered = |_: &Combatant, _: &Combatant, _, _| Some(0.0);
        state.update(&parameters, 0, &vehicles, uncovered);
        assert!(!state.knows_enemy(1, 2));

        vehicles[1].concealment = still * 0.25;
        state.update(&parameters, 20, &vehicles, uncovered);
        assert!(state.knows_enemy(1, 2));

        vehicles[1].concealment = still;
        state.update(&parameters, 70, &vehicles, uncovered);
        state.update(&parameters, 169, &vehicles, uncovered);
        assert!(state.knows_enemy(1, 2));
        assert_eq!(
            state
                .update(&parameters, 170, &vehicles, uncovered)
                .went_dark,
            [2]
        );
        assert!(!state.knows_enemy(1, 2));
    }

    #[test]
    fn radio_chain_can_split_and_rejoin_during_target_retention() {
        let mut state = Spotting::default();
        let parameters = BattleParameters::default();
        let mut vehicles = [
            vehicle(1, 1, Vec3::ZERO),
            vehicle(2, 1, Vec3::X * 150.0),
            vehicle(3, 1, Vec3::X * 300.0),
            vehicle(4, 2, Vec3::X * 400.0),
        ];
        let visible = |observer: &Combatant, _: &Combatant, _: Vec3, _: Vec3| {
            (observer.entity == 3).then_some(0.0)
        };
        state.update(&parameters, 0, &vehicles, visible);
        assert!(state.knows_enemy(1, 4));
        state.update(&parameters, 5, &vehicles, |_, _, _, _| None);
        vehicles[1].position.z = 200.0;
        state.update(&parameters, 8, &vehicles, |_, _, _, _| None);
        assert!(!state.knows_enemy(1, 4));
        assert!(state.knows_enemy(3, 4));
        vehicles[1].position.z = 0.0;
        assert_eq!(
            state
                .update(&parameters, 55, &vehicles, |_, _, _, _| None)
                .alerts,
            [4]
        );
        assert!(state.knows_enemy(1, 4));
        assert!(
            state
                .update(&parameters, 104, &vehicles, |_, _, _, _| None)
                .alerts
                .is_empty()
        );
        assert!(state.knows_enemy(1, 4));
        state.update(&parameters, 105, &vehicles, |_, _, _, _| None);
        assert!(!state.knows_enemy(1, 4));
        state.update(&parameters, 115, &vehicles, visible);
        assert_eq!(
            state.update(&parameters, 145, &vehicles, visible).alerts,
            [4]
        );
        let [a, b, _, target] = vehicles;
        state.update(&parameters, 146, &[a, b, target], |_, _, _, _| None);
        assert!(!state.knows_enemy(1, 4));
    }

    #[test]
    fn an_exposed_checkpoint_can_win_and_the_port_follows_the_clock() {
        let mut state = Spotting::default();
        let parameters = BattleParameters::default();
        let mut target = vehicle(2, 2, Vec3::X * 300.0);
        target.geometry.as_mut().unwrap().checkpoints[5].z = 5.0;
        let vehicles = [vehicle(1, 1, Vec3::ZERO), target];
        state.update(&parameters, 0, &vehicles, |_, _, _, end| {
            Some(if end.z == 5.0 { 0.0 } else { 0.8 })
        });
        assert!(state.knows_enemy(1, 2));
        let changes = state.update(&parameters, 19, &vehicles, |_, _, _, _| {
            panic!("pair is not due")
        });
        assert_eq!(changes.checked_pairs, 0);
        state.update(&parameters, 20, &vehicles, |observer, _, start, _| {
            assert_eq!(start, observer.position + Vec3::Y * 2.0);
            None
        });
        assert!(state.knows_enemy(1, 2));
    }
}
