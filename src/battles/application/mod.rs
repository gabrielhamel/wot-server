use crate::platform::mercury::app::base::ClientKey;
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
pub(crate) enum BattleMode {
    Training,
}

pub(crate) struct BattleLaunch {
    pub arena_type: i32,
    pub duration: Duration,
    pub mode: BattleMode,
    pub participants: Vec<BattleParticipant>,
}

pub(crate) struct BattleParticipant {
    pub client: ClientKey,
    pub account_id: i64,
    pub name: String,
    pub team: u8,
    pub is_host: bool,
    pub observer: bool,
    pub vehicle: VehicleLoadout,
}

pub(crate) struct VehicleLoadout {
    pub descriptor: Vec<u8>,
    pub health: i16,
    pub crew_size: usize,
    pub ammunition: Vec<(i32, u16)>,
}

impl BattleLaunch {
    pub(super) fn valid_roster(&self) -> bool {
        !self.duration.is_zero()
            && Instant::now().checked_add(self.duration).is_some()
            && self.participants.len() <= 256
            && self
                .participants
                .iter()
                .all(|player| matches!(player.team, 1 | 2))
            && [1, 2].into_iter().all(|team| {
                self.participants
                    .iter()
                    .any(|player| player.team == team && !player.observer)
            })
    }
}
