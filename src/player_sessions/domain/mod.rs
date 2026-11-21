use std::{
    collections::HashMap,
    net::SocketAddr,
    time::{Duration, Instant},
};

pub(super) mod training;

pub struct Admissions {
    pending: HashMap<SocketAddr, (u64, Instant)>,
    next: u64,
    capacity: usize,
}

impl Admissions {
    pub fn new(capacity: usize) -> Self {
        Self {
            pending: HashMap::new(),
            next: 0,
            capacity,
        }
    }

    pub fn begin(&mut self, addr: SocketAddr, now: Instant) -> Option<u64> {
        self.pending.retain(|_, (_, expires)| *expires > now);
        if self.pending.contains_key(&addr) || self.pending.len() >= self.capacity {
            return None;
        }
        self.next = self.next.checked_add(1)?;
        self.pending
            .insert(addr, (self.next, now + Duration::from_secs(15)));
        Some(self.next)
    }

    pub fn complete(&mut self, addr: SocketAddr, generation: u64, now: Instant) -> bool {
        if self.is_pending(addr, generation, now) {
            self.pending.remove(&addr);
            true
        } else {
            false
        }
    }

    pub fn is_pending(&self, addr: SocketAddr, generation: u64, now: Instant) -> bool {
        self.pending
            .get(&addr)
            .is_some_and(|&(id, expires)| id == generation && expires > now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounds_work_and_rejects_stale_results_after_retry() {
        let mut admissions = Admissions::new(1);
        let a = "127.0.0.1:1".parse().unwrap();
        let b = "127.0.0.1:2".parse().unwrap();
        let now = Instant::now();
        let first = admissions.begin(a, now).unwrap();
        assert!(admissions.begin(a, now).is_none());
        assert!(admissions.begin(b, now).is_none());
        let later = now + Duration::from_secs(16);
        let second = admissions.begin(a, later).unwrap();
        assert!(!admissions.complete(a, first, later));
        assert!(admissions.complete(a, second, later));
        assert!(admissions.begin(b, later).is_some());
    }
}

pub(crate) mod training_vehicle;
