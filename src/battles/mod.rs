mod application;
mod domain;
mod infrastructure;

/// Launch and lifecycle operations for callers that assemble battle rosters.
pub(crate) mod contract {
    pub(crate) use super::application::{
        BattleLaunch, BattleMode, BattleParticipant, VehicleLoadout,
    };
    pub(crate) use super::infrastructure::{
        AvatarAction, BattleRuntime, ParticipantDeparture, PreparedBattle, TICK,
    };
}
