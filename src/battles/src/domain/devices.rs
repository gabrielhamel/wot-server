use super::{armor::ImpactSegment, parameters::BattleParameters};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
use wot_shared_kernel::{Device, DeviceHealth};

#[derive(Clone, Copy, Debug)]
pub(crate) struct DeviceStrike {
    pub device: Device,
    pub can_ignite_engine: bool,
    pub health: Option<DeviceHealth>,
    pub damage: u16,
    pub impact: ImpactSegment,
}

pub(crate) const TRACKS: [Device; 2] = [Device::TrackLeft, Device::TrackRight];
pub(crate) const ENGINE_AND_FUEL: [Device; 2] = [Device::Engine, Device::Fuel];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeviceEvent {
    Damaged,
    Broken,
    RepairRestarted,
    RepairedCritical,
    Repaired,
}

impl DeviceEvent {
    pub fn name(self) -> &'static str {
        match self {
            Self::Damaged => "damaged",
            Self::Broken => "broken",
            Self::RepairRestarted => "repair_restarted",
            Self::RepairedCritical => "repaired_critical",
            Self::Repaired => "repaired",
        }
    }

    pub fn starts_repair(self) -> bool {
        matches!(self, Self::Broken | Self::RepairRestarted)
    }
}

#[derive(Clone, Copy)]
pub(crate) enum DeviceDamage {
    Hit,
    Critical,
    Destroyed,
}

impl DeviceDamage {
    pub fn change(self) -> Option<DeviceEvent> {
        match self {
            Self::Hit => None,
            Self::Critical => Some(DeviceEvent::Damaged),
            Self::Destroyed => Some(DeviceEvent::Broken),
        }
    }
}

pub(crate) const DEVICE_STRIKE_CAPACITY: usize = 32;

struct DeviceState {
    health: u16,
    profile: DeviceHealth,
    repair: Option<(Instant, Instant)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FireOrigin {
    Engine,
    Fuel,
}

impl FireOrigin {
    pub fn name(self) -> &'static str {
        match self {
            Self::Engine => "engine",
            Self::Fuel => "fuel_tank",
        }
    }
}

#[derive(Default)]
pub(crate) struct Condition {
    devices: BTreeMap<Device, DeviceState>,
    fire: Option<(Instant, u8)>,
}

#[derive(Default)]
pub(crate) struct Criticals {
    pub tracks: [Option<DeviceEvent>; 2],
    pub track_hits: u8,
    pub track_impact: Option<ImpactSegment>,
    pub damaged: u8,
    pub destroyed: u8,
    pub crew: u8,
    pub engine: Option<DeviceDamage>,
    pub fuel: Option<DeviceDamage>,
    pub detonated: bool,
    pub fire_started: Option<FireOrigin>,
}

impl Condition {
    pub fn apply(
        &mut self,
        strikes: &[Option<DeviceStrike>],
        profiles: &BTreeMap<Device, DeviceHealth>,
        armed: bool,
        now: Instant,
        parameters: &BattleParameters,
    ) -> Criticals {
        let mut result = Criticals::default();
        for strike in strikes.iter().flatten() {
            let crew = matches!(strike.device, Device::Crew(_));
            let profile = if crew {
                DeviceHealth {
                    maximum: 1,
                    repaired: 1,
                    repair_seconds: 0.0,
                    fire_chance: 0.0,
                }
            } else if let Some(profile) = profiles.get(&strike.device).copied().or(strike.health) {
                profile
            } else {
                continue;
            };
            let state = self.devices.entry(strike.device).or_insert(DeviceState {
                health: profile.maximum,
                profile,
                repair: None,
            });
            let track = TRACKS.iter().position(|&track| track == strike.device);
            let previous = state.health;
            if strike.damage == 0 || (previous == 0 && (track.is_none() || state.repair.is_none()))
            {
                continue;
            }
            state.health = previous.saturating_sub(strike.damage);
            if let Some(side) = track {
                result.track_hits += 1;
                result.track_impact.get_or_insert(strike.impact);
                if state.health == 0 {
                    result.tracks[side] = Some(if previous == 0 {
                        DeviceEvent::RepairRestarted
                    } else {
                        DeviceEvent::Broken
                    });
                } else if previous > profile.repaired && state.health <= profile.repaired {
                    result.tracks[side] = Some(DeviceEvent::Damaged);
                }
            }
            let transition = if state.health == 0 {
                DeviceDamage::Destroyed
            } else if previous > profile.repaired && state.health <= profile.repaired {
                DeviceDamage::Critical
            } else {
                DeviceDamage::Hit
            };
            match strike.device {
                Device::Engine => result.engine = Some(transition),
                Device::Fuel => result.fuel = Some(transition),
                _ => {}
            }
            result.damaged += 1;
            if state.health == 0 {
                result.destroyed += u8::from(previous > 0);
                let detonates = strike.device == Device::AmmoRack && armed;
                if crew {
                    result.crew += 1;
                } else if !detonates {
                    let time_scale = if track.is_some() {
                        parameters.track_repair_time_scale
                    } else {
                        1.0
                    };
                    let duration =
                        Duration::from_secs_f32(state.profile.repair_seconds * time_scale);
                    state.repair = Some((now, now + duration));
                }
                result.detonated |= detonates;
            }
            if parameters.fire_ticks > 0 && self.fire.is_none() {
                let origin = match strike.device {
                    Device::Fuel if state.health == 0 => Some(FireOrigin::Fuel),
                    Device::Engine
                        if strike.can_ignite_engine
                            && strike.damage >= parameters.engine_ignition_min_damage
                            && fastrand::f32() < state.profile.fire_chance =>
                    {
                        Some(FireOrigin::Engine)
                    }
                    _ => None,
                };
                if let Some(origin) = origin {
                    self.fire = Some((
                        now + Duration::from_secs_f32(parameters.fire_tick_seconds),
                        parameters.fire_ticks,
                    ));
                    result.fire_started = Some(origin);
                }
            }
        }
        result
    }

    pub fn disabled(&self, device: Device) -> bool {
        self.devices
            .get(&device)
            .is_some_and(|state| state.health == 0)
    }

    pub fn immobilized(&self) -> bool {
        self.disabled(Device::Engine) || TRACKS.into_iter().any(|track| self.disabled(track))
    }

    pub fn repair(&self, device: Device) -> Option<(Instant, Instant)> {
        self.devices.get(&device)?.repair
    }

    pub fn tick(
        &mut self,
        now: Instant,
        parameters: &BattleParameters,
    ) -> (u8, Option<u8>, [Option<DeviceEvent>; 4]) {
        let mut repairs = 0;
        let mut events = [None; 4];
        for (&device, state) in &mut self.devices {
            if state.repair.is_some_and(|(_, end)| end <= now) {
                state.health = state.profile.repaired;
                state.repair = None;
                repairs += 1;
                if let Some(index) = TRACKS
                    .into_iter()
                    .chain(ENGINE_AND_FUEL)
                    .position(|tracked| tracked == device)
                {
                    events[index] = Some(if state.health < state.profile.maximum {
                        DeviceEvent::RepairedCritical
                    } else {
                        DeviceEvent::Repaired
                    });
                }
            }
        }
        let mut burn = None;
        if self.fire.is_some_and(|(next, _)| next <= now)
            && let Some((next, ticks)) = self.fire.take()
        {
            burn = Some(ticks);
            if ticks > 1 {
                self.fire = Some((
                    next + Duration::from_secs_f32(parameters.fire_tick_seconds),
                    ticks - 1,
                ));
            }
        }
        (repairs, burn, events)
    }

    pub fn extinguish(&mut self) {
        self.fire = None;
    }

    pub fn has_other_device_repair(&self) -> bool {
        self.devices.iter().any(|(device, state)| {
            !TRACKS.contains(device) && !ENGINE_AND_FUEL.contains(device) && state.repair.is_some()
        })
    }

    pub fn active(&self) -> bool {
        self.burning() || self.devices.values().any(|state| state.repair.is_some())
    }

    pub fn burning(&self) -> bool {
        self.fire.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_ignites_above_threshold_without_resetting_an_active_fire() {
        let now = Instant::now();
        let mut condition = Condition::default();
        let parameters = BattleParameters::default();
        let strike = |damage| {
            Some(DeviceStrike {
                device: Device::Engine,
                can_ignite_engine: true,
                health: Some(DeviceHealth {
                    maximum: 100,
                    repaired: 50,
                    repair_seconds: 10.0,
                    fire_chance: 1.0,
                }),
                damage,
                impact: ImpactSegment {
                    part: 1,
                    start: [0; 3],
                    end: [255; 3],
                },
            })
        };
        let threshold = parameters.engine_ignition_min_damage;
        let profiles = BTreeMap::new();
        assert_eq!(
            condition
                .apply(&[strike(threshold - 1)], &profiles, true, now, &parameters)
                .fire_started,
            None
        );
        assert_eq!(
            condition
                .apply(&[strike(threshold)], &profiles, true, now, &parameters)
                .fire_started,
            Some(FireOrigin::Engine)
        );
        assert!(!condition.disabled(Device::Engine));
        assert_eq!(
            condition
                .apply(
                    &[strike(threshold)],
                    &profiles,
                    true,
                    now + Duration::from_millis(250),
                    &parameters
                )
                .fire_started,
            None
        );
        assert_eq!(
            condition
                .tick(now + Duration::from_millis(500), &parameters)
                .1,
            Some(parameters.fire_ticks)
        );
        condition.extinguish();
        assert_eq!(
            condition.tick(now + Duration::from_secs(1), &parameters).1,
            None
        );
        let mut transmission_strike = strike(threshold).unwrap();
        transmission_strike.can_ignite_engine = false;
        assert_eq!(
            condition
                .apply(
                    &[Some(transmission_strike)],
                    &profiles,
                    true,
                    now,
                    &parameters
                )
                .fire_started,
            None
        );
        let disabled = BattleParameters {
            fire_ticks: 0,
            ..parameters
        };
        assert_eq!(
            condition
                .apply(&[strike(threshold)], &profiles, true, now, &disabled)
                .fire_started,
            None
        );
    }

    #[test]
    fn destroyed_track_repairs_and_loaded_rack_detonates() {
        let now = Instant::now();
        let parameters = BattleParameters {
            track_repair_time_scale: 2.0,
            ..BattleParameters::default()
        };
        let profile = DeviceHealth {
            maximum: 10,
            repaired: 5,
            repair_seconds: 1.0,
            fire_chance: 0.0,
        };
        let strike = |device| {
            Some(DeviceStrike {
                device,
                can_ignite_engine: false,
                health: Some(profile),
                damage: 10,
                impact: ImpactSegment {
                    part: 0,
                    start: [0; 3],
                    end: [255; 3],
                },
            })
        };
        let mut condition = Condition::default();
        let profiles = BTreeMap::new();
        let critical = condition.apply(
            &[strike(Device::TrackLeft)],
            &profiles,
            true,
            now,
            &parameters,
        );
        assert_eq!(critical.destroyed, 1);
        assert!(condition.immobilized());
        let mut no_damage = strike(Device::TrackLeft).unwrap();
        no_damage.damage = 0;
        let ignored = condition.apply(
            &[Some(no_damage)],
            &profiles,
            true,
            now + Duration::from_secs(1),
            &parameters,
        );
        assert_eq!(ignored.tracks, [None; 2]);
        assert_eq!(
            condition.tick(now + Duration::from_secs(1), &parameters),
            (0, None, [None; 4])
        );
        assert_eq!(
            condition.tick(now + Duration::from_secs(2), &parameters),
            (
                1,
                None,
                [Some(DeviceEvent::RepairedCritical), None, None, None]
            )
        );
        assert!(!condition.immobilized());
        assert!(
            !condition
                .apply(
                    &[strike(Device::AmmoRack)],
                    &profiles,
                    false,
                    now,
                    &parameters
                )
                .detonated
        );
        let mut armed = Condition::default();
        assert!(
            armed
                .apply(
                    &[strike(Device::AmmoRack)],
                    &profiles,
                    true,
                    now,
                    &parameters
                )
                .detonated
        );
        assert!(
            armed
                .apply(&[strike(Device::Fuel)], &profiles, true, now, &parameters)
                .fire_started
                .is_some()
        );
        assert_eq!(
            armed.tick(now + Duration::from_secs(1), &parameters),
            (
                1,
                Some(parameters.fire_ticks),
                [None, None, None, Some(DeviceEvent::RepairedCritical)]
            )
        );
    }
}
