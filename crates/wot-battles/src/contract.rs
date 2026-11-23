//! Rosters, decoded commands and lifecycle operations for live battles.

pub use crate::application::{
    AvatarCommand, BattleLaunch, BattleMode, BattleParticipant, ParticipantId, VehicleCommand,
    VehicleControl, VehicleLoadout,
};
pub use crate::infrastructure::{
    AvatarAction, BattleRuntime, ParticipantDeparture, PreparedBattle, TICK,
};
