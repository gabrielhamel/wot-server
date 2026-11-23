use std::time::{Duration, Instant};
use wot_shared_kernel::GunLoading;

pub(super) struct Ammunition {
    pub rounds: Vec<(i32, u16)>,
    current: usize,
    next: usize,
    loaded: u16,
    cooldown: Option<Reload>,
    refill: Option<Reload>,
}

#[derive(Clone, Copy)]
pub(crate) struct Reload {
    pub seconds: f32,
    deadline: Instant,
}

impl Reload {
    fn new(now: Instant, seconds: f32) -> Self {
        Self {
            seconds,
            deadline: now + Duration::from_secs_f32(seconds),
        }
    }

    pub fn remaining(self, now: Instant) -> f32 {
        self.deadline.saturating_duration_since(now).as_secs_f32()
    }
}

impl Ammunition {
    pub fn new(rounds: Vec<(i32, u16)>, capacity: u16) -> Self {
        let current = rounds.iter().position(|&(_, count)| count > 0).unwrap_or(0);
        let loaded = rounds
            .get(current)
            .map_or(0, |&(_, count)| count.min(capacity));
        Self {
            rounds,
            current,
            next: current,
            loaded,
            cooldown: None,
            refill: None,
        }
    }

    pub fn settings(&self) -> [i32; 2] {
        [self.current, self.next].map(|index| self.rounds.get(index).map_or(0, |&(id, _)| id))
    }

    pub fn loaded(&self, shell: i32) -> u16 {
        if self.settings()[0] == shell {
            self.loaded
        } else {
            0
        }
    }

    pub fn timers(&self, gun: &GunLoading) -> [Option<Reload>; 2] {
        if gun.autoreload_times.is_empty() {
            [self.refill.or(self.cooldown), None]
        } else {
            [self.cooldown, self.refill]
        }
    }

    pub fn select_initial(&mut self, gun: &GunLoading, shell: i32) -> &'static str {
        if let Some(index) = self.shell_with_stock(shell) {
            self.current = index;
            self.next = index;
            self.loaded = self.stock(index).min(gun.capacity);
            "shell_selected"
        } else {
            "shell_unavailable"
        }
    }

    pub fn select(
        &mut self,
        gun: &GunLoading,
        now: Instant,
        shell: i32,
        immediate: bool,
    ) -> &'static str {
        let Some(index) = self.shell_with_stock(shell) else {
            return "shell_unavailable";
        };
        self.next = index;
        if index == self.current {
            return "shell_selected";
        }
        if immediate {
            let fully_loaded = self.loaded > 0
                && self.loaded == self.stock(self.current).min(gun.capacity)
                && self.cooldown.is_none_or(|timer| timer.deadline <= now);
            self.load_magazine(gun, now, fully_loaded);
            "shell_change_started"
        } else {
            "shell_queued"
        }
    }

    pub fn manual_reload(&mut self, gun: &GunLoading, now: Instant) -> &'static str {
        if gun.capacity <= 1 || !gun.autoreload_times.is_empty() {
            return "manual_reload_unavailable";
        }
        if self.loaded == 0 && self.refill.is_some() {
            return "reloading";
        }
        if self.next == self.current
            && self.loaded > 0
            && self.loaded == self.stock(self.current).min(gun.capacity)
        {
            return "magazine_full";
        }
        if self.stock(self.next) == 0 {
            return "empty";
        }
        self.load_magazine(gun, now, false);
        "reload_started"
    }

    pub fn fire(&mut self, gun: &GunLoading, now: Instant) -> Result<(), &'static str> {
        if self.refill.is_some() && self.loaded == 0
            || self.cooldown.is_some_and(|timer| timer.deadline > now)
        {
            return Err("reloading");
        }
        let Some((_, count)) = self
            .rounds
            .get_mut(self.current)
            .filter(|(_, count)| *count > 0)
        else {
            return Err("empty");
        };
        if self.loaded == 0 {
            return Err("empty");
        }
        *count -= 1;
        self.loaded -= 1;
        if self.loaded == 0
            && self.stock(self.next) == 0
            && let Some(index) = self.rounds.iter().position(|&(_, count)| count > 0)
        {
            self.next = index;
        }
        if self.loaded == 0 && (gun.autoreload_times.is_empty() || self.next != self.current) {
            self.load_magazine(gun, now, false);
        } else {
            self.cooldown = Some(Reload::new(now, gun.intra_clip_time));
            self.refill = self.refill_timer(gun, now);
        }
        Ok(())
    }

    pub fn advance(&mut self, gun: &GunLoading, now: Instant) -> bool {
        let mut changed = false;
        if self.cooldown.is_some_and(|timer| timer.deadline <= now) {
            self.cooldown = None;
            changed = true;
        }
        while let Some(timer) = self.refill.filter(|timer| timer.deadline <= now) {
            let available = self.stock(self.current).min(gun.capacity);
            if gun.autoreload_times.is_empty() {
                self.loaded = available;
                self.refill = None;
            } else {
                self.loaded = self.loaded.saturating_add(1).min(available);
                self.refill = self.refill_timer(gun, timer.deadline);
            }
            changed = true;
        }
        changed
    }

    fn shell_with_stock(&self, shell: i32) -> Option<usize> {
        self.rounds
            .iter()
            .position(|&(id, count)| id == shell && count > 0)
    }

    fn stock(&self, index: usize) -> u16 {
        self.rounds.get(index).map_or(0, |&(_, count)| count)
    }

    fn load_magazine(&mut self, gun: &GunLoading, now: Instant, forced: bool) {
        self.loaded = 0;
        self.current = self.next;
        self.cooldown = None;
        self.refill = if forced && gun.forced_reload_time > 0.0 {
            Some(Reload::new(now, gun.forced_reload_time))
        } else if gun.autoreload_times.is_empty() {
            (self.stock(self.current) > 0).then(|| Reload::new(now, gun.reload_time))
        } else {
            self.refill_timer(gun, now)
        };
    }

    fn refill_timer(&self, gun: &GunLoading, now: Instant) -> Option<Reload> {
        let missing = gun.capacity.checked_sub(self.loaded)?.checked_sub(1)?;
        if self.loaded >= self.stock(self.current) || gun.autoreload_times.is_empty() {
            return None;
        }
        let index = usize::from(missing).min(gun.autoreload_times.len() - 1);
        Some(Reload::new(now, *gun.autoreload_times.get(index)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn magazine() -> GunLoading {
        GunLoading {
            capacity: 3,
            reload_time: 8.0,
            intra_clip_time: 2.0,
            forced_reload_time: 5.0,
            autoreload_times: vec![],
        }
    }

    #[test]
    fn changing_shells_queues_or_reloads_without_spending_unfired_rounds() {
        let gun = magazine();
        let mut ammo = Ammunition::new(vec![(10, 5), (20, 2), (30, 0)], gun.capacity);
        let now = Instant::now();
        assert_eq!(ammo.select(&gun, now, 30, false), "shell_unavailable");
        assert_eq!(ammo.select(&gun, now, 20, false), "shell_queued");
        assert_eq!(ammo.settings(), [10, 20]);
        assert_eq!(ammo.loaded(10), 3);
        assert!(ammo.fire(&gun, now).is_ok());
        assert_eq!(ammo.fire(&gun, now), Err("reloading"));
        assert_eq!(ammo.select(&gun, now, 20, true), "shell_change_started");
        assert_eq!(ammo.settings(), [20, 20]);
        assert_eq!(ammo.loaded(20), 0);
        assert!(!ammo.advance(&gun, now + Duration::from_secs(5)));
        assert!(ammo.advance(&gun, now + Duration::from_secs(8)));
        assert_eq!(ammo.loaded(20), 2);
        assert_eq!(ammo.rounds, [(10, 4), (20, 2), (30, 0)]);
        let change = now + Duration::from_secs(10);
        assert_eq!(ammo.select(&gun, change, 10, true), "shell_change_started");
        assert!(ammo.advance(&gun, change + Duration::from_secs(5)));
        assert_eq!(ammo.loaded(10), 3);
    }

    #[test]
    fn manual_magazine_reload_uses_the_queued_shell_and_full_reload_time() {
        let gun = magazine();
        let mut ammo = Ammunition::new(vec![(10, 5), (20, 5)], gun.capacity);
        let now = Instant::now();
        assert_eq!(ammo.manual_reload(&gun, now), "magazine_full");
        ammo.select(&gun, now, 20, false);
        assert_eq!(ammo.manual_reload(&gun, now), "reload_started");
        assert_eq!(ammo.manual_reload(&gun, now), "reloading");
        assert_eq!(ammo.fire(&gun, now), Err("reloading"));
        assert!(ammo.advance(&gun, now + Duration::from_secs(8)));
        assert_eq!(ammo.loaded(20), 3);
        assert_eq!(ammo.rounds, [(10, 5), (20, 5)]);
    }

    #[test]
    fn autoreload_fills_one_round_at_a_time_and_restarts_after_a_shot() {
        let gun = GunLoading {
            autoreload_times: vec![4.0, 6.0, 9.0],
            ..magazine()
        };
        let mut ammo = Ammunition::new(vec![(10, 7)], gun.capacity);
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        assert_eq!(ammo.manual_reload(&gun, at(0)), "manual_reload_unavailable");
        assert!(ammo.fire(&gun, at(0)).is_ok());
        assert_eq!(
            ammo.timers(&gun).map(|timer| timer.unwrap().seconds),
            [2.0, 4.0]
        );
        assert!(ammo.advance(&gun, at(2)));
        assert!(ammo.fire(&gun, at(2)).is_ok());
        assert_eq!(ammo.loaded(10), 1);
        assert!(ammo.advance(&gun, at(4)));
        assert_eq!(ammo.loaded(10), 1);
        assert!(ammo.advance(&gun, at(8)));
        assert_eq!(ammo.loaded(10), 2);
        assert!(ammo.advance(&gun, at(12)));
        assert_eq!(ammo.loaded(10), 3);
        assert_eq!(ammo.rounds, [(10, 5)]);
        assert!(ammo.timers(&gun).iter().all(Option::is_none));
    }
}
