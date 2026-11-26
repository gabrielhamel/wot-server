use super::parameters::BattleParameters;
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
use wot_shared_kernel::{Device, DeviceHealth};

#[derive(Clone, Copy, Debug)]
pub(crate) struct DeviceStrike {
    pub device: Device,
    pub health: Option<DeviceHealth>,
    pub damage: u16,
}

pub(crate) const TRACKS: [Device; 2] = [Device::TrackLeft, Device::TrackRight];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TrackEvent {
    Damaged,
    Broken,
    RepairedCritical,
    Repaired,
}

impl TrackEvent {
    pub fn name(self) -> &'static str {
        match self {
            Self::Damaged => "damaged",
            Self::Broken => "broken",
            Self::RepairedCritical => "repaired_critical",
            Self::Repaired => "repaired",
        }
    }
}

pub(crate) const DEVICE_STRIKE_CAPACITY: usize = 32;

struct DeviceState {
    health: u16,
    profile: DeviceHealth,
    repair: Option<(Instant, Instant)>,
}

#[derive(Default)]
pub(crate) struct Condition {
    devices: BTreeMap<Device, DeviceState>,
    fire: Option<(Instant, u8)>,
}

#[derive(Default)]
pub(crate) struct Criticals {
    pub tracks: [Option<TrackEvent>; 2],
    pub track_hits: u8,
    pub damaged: u8,
    pub destroyed: u8,
    pub crew: u8,
    pub detonated: bool,
    pub fire_started: bool,
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
            if state.health == 0 {
                continue;
            }
            let previous = state.health;
            state.health = previous.saturating_sub(strike.damage);
            let track = TRACKS.iter().position(|&track| track == strike.device);
            if let Some(side) = track {
                result.track_hits += 1;
                if state.health == 0 {
                    result.tracks[side] = Some(TrackEvent::Broken);
                } else if previous > profile.repaired && state.health <= profile.repaired {
                    result.tracks[side] = Some(TrackEvent::Damaged);
                }
            }
            result.damaged += 1;
            if state.health == 0 {
                result.destroyed += 1;
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
            let starts_fire = (strike.device == Device::Fuel && state.health == 0)
                || (strike.device == Device::Engine && fastrand::f32() < state.profile.fire_chance);
            if starts_fire && parameters.fire_ticks > 0 && self.fire.is_none() {
                self.fire = Some((
                    now + Duration::from_secs_f32(parameters.fire_tick_seconds),
                    parameters.fire_ticks,
                ));
                result.fire_started = true;
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

    pub fn track_repair(&self, track: Device) -> Option<(Instant, Instant)> {
        self.devices.get(&track)?.repair
    }

    pub fn tick(
        &mut self,
        now: Instant,
        parameters: &BattleParameters,
    ) -> (u8, bool, [Option<TrackEvent>; 2]) {
        let mut repairs = 0;
        let mut tracks = [None; 2];
        for (&device, state) in &mut self.devices {
            if state.repair.is_some_and(|(_, end)| end <= now) {
                state.health = state.profile.repaired;
                state.repair = None;
                repairs += 1;
                if let Some(side) = TRACKS.iter().position(|&track| track == device) {
                    tracks[side] = Some(if state.health < state.profile.maximum {
                        TrackEvent::RepairedCritical
                    } else {
                        TrackEvent::Repaired
                    });
                }
            }
        }
        let burn = self.fire.is_some_and(|(next, _)| next <= now);
        if burn
            && let Some((next, ticks)) = self.fire.take()
            && ticks > 1
        {
            self.fire = Some((
                next + Duration::from_secs_f32(parameters.fire_tick_seconds),
                ticks - 1,
            ));
        }
        (repairs, burn, tracks)
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
                health: Some(profile),
                damage: 10,
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
        assert_eq!(
            condition.tick(now + Duration::from_secs(1), &parameters),
            (0, false, [None; 2])
        );
        assert_eq!(
            condition.tick(now + Duration::from_secs(2), &parameters),
            (1, false, [Some(TrackEvent::RepairedCritical), None])
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
        );
        assert_eq!(
            armed.tick(now + Duration::from_secs(1), &parameters),
            (1, true, [None; 2])
        );
    }
}
