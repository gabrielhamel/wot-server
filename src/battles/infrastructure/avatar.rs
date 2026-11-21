use crate::platform::mercury::app::entity::{Bytes, EntityId, entity_schema};

crate::script_dict! {
    pub struct ShotResult {
        pub vehicleID: u32,
        pub hitFlags: u32,
        pub gunInstallationIndex: u8,
    }
}

crate::script_dict! {
    pub struct BattleEvent {
        pub eventType: u8,
        pub targetID: EntityId,
        pub details: u64,
        pub count: u16,
    }
}

impl BattleEvent {
    pub(super) fn shot_damage(other_vehicle: u32, damage: u16, received: bool) -> Self {
        const DAMAGE: u8 = 7;
        const RECEIVED_DAMAGE: u8 = 10;
        const UNKNOWN_SHELL_KIND: u64 = 127;
        const NO_SECONDARY_ATTACK_REASON: u64 = 16;
        const NO_EXTENDED_ATTACK_REASON: u64 = 255;

        Self {
            eventType: if received { RECEIVED_DAMAGE } else { DAMAGE },
            targetID: EntityId(other_vehicle),
            details: (u64::from(damage) << 25)
                | (UNKNOWN_SHELL_KIND << 9)
                | NO_SECONDARY_ATTACK_REASON
                | (NO_EXTENDED_ATTACK_REASON << 43),
            count: 1,
        }
    }
}

entity_schema! {
    pub Avatar {
        client {
            fn updateArena(update_type: u8, data: Bytes);
            fn onRoundFinished(winner_team: i8, reason: u8);
            fn showOwnVehicleHitDirection(yaw: f32, attacker: EntityId, damage: u16, criticals: u32, blocked: bool, explosive: bool, victim: EntityId, reason: u8);
            fn showTracer(shooter: EntityId, shot: i32, ricochet: u8, effects: u8, prefab: u8, shell: u8, caliber: f32, start: glam::Vec3, velocity: glam::Vec3, gravity: f32, range: f32, gun: u8, installation: u8);
            fn stopTracer(shot: i32, endpoint: glam::Vec3);
            fn showShotResults(results: Vec<ShotResult>);
            fn onBattleEvents(events: Vec<BattleEvent>);
        }
        base {
            fn setClientReady();
            fn leaveArena();
        }
    }
}
