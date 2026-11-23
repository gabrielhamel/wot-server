mod application;
mod domain;
mod infrastructure;

/// Rosters, decoded commands and lifecycle operations for live battles.
pub(crate) mod contract {
    pub(crate) use super::application::{
        AvatarCommand, BattleLaunch, BattleMode, BattleParticipant, ParticipantId, VehicleCommand,
        VehicleControl, VehicleLoadout,
    };
    pub(crate) use super::infrastructure::{
        AvatarAction, BattleRuntime, ParticipantDeparture, PreparedBattle, TICK,
    };
}

/// In-process adapter for sessions connected through Mercury.
pub(crate) use infrastructure::mercury;
