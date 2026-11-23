use super::ParticipantId;
use glam::Vec3;
use std::sync::Arc;

#[derive(Clone, Copy)]
pub(crate) enum AvatarCommand {
    Ready,
    Leave,
}

pub(crate) struct VehicleControl {
    pub participant: ParticipantId,
    pub entity_id: u32,
    pub method: Arc<str>,
    pub argument_count: usize,
    pub command: VehicleCommand,
}

pub(crate) enum VehicleCommand {
    SetServerMarker(bool),
    Drive(u8),
    AimRelative(Vec3),
    AimWorld(Vec3),
    AimAngles { yaw: f32, pitch: f32 },
    Shoot,
    ChangeSetting { setting: u8, shell: i32 },
    Unsupported,
}
