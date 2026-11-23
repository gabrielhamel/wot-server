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

pub(crate) const MAX_DEVICE_HITS: usize = 12;

struct DeviceState {
    health: u16,
    profile: DeviceHealth,
    repair_at: Option<Instant>,
}

#[derive(Default)]
pub(crate) struct Condition {
    devices: BTreeMap<Device, DeviceState>,
    fire: Option<(Instant, u8)>,
}

#[derive(Default)]
pub(crate) struct Criticals {
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
                repair_at: None,
            });
            if state.health == 0 {
                continue;
            }
            state.health = state.health.saturating_sub(strike.damage);
            result.damaged += 1;
            if state.health == 0 {
                result.destroyed += 1;
                let detonates = strike.device == Device::AmmoRack && armed;
                if crew {
                    result.crew += 1;
                } else if !detonates {
                    state.repair_at =
                        Some(now + Duration::from_secs_f32(state.profile.repair_seconds));
                }
                result.detonated |= detonates;
            }
            let starts_fire = (strike.device == Device::Fuel && state.health == 0)
                || (strike.device == Device::Engine && fastrand::f32() < state.profile.fire_chance);
            if starts_fire && self.fire.is_none() {
                self.fire = Some((now + Duration::from_secs(1), 5));
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
        [Device::TrackLeft, Device::TrackRight, Device::Engine]
            .into_iter()
            .any(|device| self.disabled(device))
    }

    pub fn tick(&mut self, now: Instant) -> (u8, bool) {
        let mut repairs = 0;
        for state in self.devices.values_mut() {
            if state.repair_at.is_some_and(|end| end <= now) {
                state.health = state.profile.repaired;
                state.repair_at = None;
                repairs += 1;
            }
        }
        let burn = self.fire.is_some_and(|(next, _)| next <= now);
        if burn
            && let Some((next, ticks)) = self.fire.take()
            && ticks > 1
        {
            self.fire = Some((next + Duration::from_secs(1), ticks - 1));
        }
        (repairs, burn)
    }

    pub fn active(&self) -> bool {
        self.burning() || self.devices.values().any(|state| state.repair_at.is_some())
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
        let critical = condition.apply(&[strike(Device::TrackLeft)], &profiles, true, now);
        assert_eq!(critical.destroyed, 1);
        assert!(condition.immobilized());
        assert_eq!(condition.tick(now + Duration::from_secs(1)), (1, false));
        assert!(!condition.immobilized());
        assert!(
            !condition
                .apply(&[strike(Device::AmmoRack)], &profiles, false, now)
                .detonated
        );
        let mut armed = Condition::default();
        assert!(
            armed
                .apply(&[strike(Device::AmmoRack)], &profiles, true, now)
                .detonated
        );
        assert!(
            armed
                .apply(&[strike(Device::Fuel)], &profiles, true, now)
                .fire_started
        );
        assert_eq!(armed.tick(now + Duration::from_secs(1)), (1, true));
    }
}
