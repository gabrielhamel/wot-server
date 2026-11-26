#![forbid(unsafe_code)]

mod application;
mod domain;

pub mod contract {
    pub use crate::application::{RoomDeparture, TrainingRooms};
    pub use crate::domain::{
        JoinRejection, MAX_ASSIGNED_PLAYERS, MAX_ROOM_PLAYERS, RoomMember, RosterSlot,
        TrainingRoom, TrainingRoomSettings, VoiceChatMode, valid_invitation_comment,
    };
}
