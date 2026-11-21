use crate::platform::mercury::app::entity::{Bytes, EntityId, entity_schema};

entity_schema! {
    pub Avatar {
        client {
            fn updateArena(update_type: u8, data: Bytes);
            fn onRoundFinished(winner_team: i8, reason: u8);
            fn showOwnVehicleHitDirection(yaw: f32, attacker: EntityId, damage: u16, criticals: u32, blocked: bool, explosive: bool, victim: EntityId, reason: u8);
            fn showTracer(shooter: EntityId, shot: i32, ricochet: u8, effects: u8, prefab: u8, shell: u8, caliber: f32, start: glam::Vec3, velocity: glam::Vec3, gravity: f32, range: f32, gun: u8, installation: u8);
        }
        base {
            fn setClientReady();
            fn leaveArena();
        }
    }
}
