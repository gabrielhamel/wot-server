use std::time::{Duration, Instant};

mod commands;

pub use commands::{AvatarCommand, VehicleCommand, VehicleControl};

/// The admission generation of a player session, unique within the gateway.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ParticipantId(pub u64);

#[derive(Clone, Copy)]
pub enum BattleMode {
    Training,
}

pub struct BattleLaunch {
    pub arena_type: i32,
    pub duration: Duration,
    pub mode: BattleMode,
    pub participants: Vec<BattleParticipant>,
}

pub struct BattleParticipant {
    pub id: ParticipantId,
    pub account_id: i64,
    pub name: String,
    pub team: u8,
    pub is_host: bool,
    pub observer: bool,
    pub customization_display_type: u8,
    pub vehicle: VehicleLoadout,
}

pub struct VehicleLoadout {
    pub descriptor: Vec<u8>,
    pub outfit: Vec<u8>,
    pub health: i16,
    pub crew_size: usize,
    pub performance: Option<wot_shared_kernel::VehiclePerformance>,
    pub ammunition: Vec<(i32, u16)>,
}

impl BattleLaunch {
    pub(super) fn valid_roster(&self) -> bool {
        !self.duration.is_zero()
            && Instant::now().checked_add(self.duration).is_some()
            && self.participants.len() <= 256
            && self.participants.iter().all(|player| {
                matches!(player.team, 1 | 2)
                    && (player.observer || player.vehicle.performance.is_some())
            })
            && [1, 2].into_iter().all(|team| {
                self.participants
                    .iter()
                    .any(|player| player.team == team && !player.observer)
            })
    }
}
