use wot_platform::mercury::app::entity::{Bytes, EntityId, entity_schema};
use wot_shared_kernel::ShellEffects;

wot_platform::script_dict! {
    pub struct ShotResult {
        pub vehicleID: u32,
        pub hitFlags: u32,
        pub gunInstallationIndex: u8,
    }
}

wot_platform::script_dict! {
    pub struct BattleEvent {
        pub eventType: u8,
        pub targetID: EntityId,
        pub details: u64,
        pub count: u16,
    }
}

impl BattleEvent {
    pub(super) fn shot_damage(
        other_vehicle: u32,
        damage: u16,
        received: bool,
        shell: ShellEffects,
    ) -> Self {
        const DAMAGE: u8 = 7;
        const RECEIVED_DAMAGE: u8 = 10;
        const UNKNOWN_SHELL_TYPE: u8 = 127;
        const NO_SECONDARY_ATTACK_REASON: u64 = 16;
        const NO_EXTENDED_ATTACK_REASON: u64 = 255;

        let shell_type = shell.damage_log_type.filter(|_| received);
        Self {
            eventType: if received { RECEIVED_DAMAGE } else { DAMAGE },
            targetID: EntityId(other_vehicle),
            details: (u64::from(damage) << 25)
                | (u64::from(shell_type.unwrap_or(UNKNOWN_SHELL_TYPE)) << 9)
                | (u64::from(shell_type.is_some() && shell.premium) << 8)
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
            fn explodeProjectile(shot: i32, effects: u8, prefab: u8, material: u8, shell: u8, caliber: f32, endpoint: glam::Vec3, direction: glam::Vec3, speed: f32, destructibles: Vec<u32>);
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

pub(crate) enum AvatarUpdate {
    Arena {
        kind: u8,
        data: Vec<u8>,
    },
    RoundFinished {
        winner: i8,
        reason: u8,
    },
    HitDirection {
        yaw: f32,
        attacker: u32,
        damage: u16,
        explosive: bool,
        victim: u32,
    },
    Tracer {
        shooter: u32,
        id: i32,
        effects: ShellEffects,
        start: glam::Vec3,
        velocity: glam::Vec3,
        range: f32,
    },
    Impact {
        id: i32,
        effects: ShellEffects,
        material: u8,
        endpoint: glam::Vec3,
        direction: glam::Vec3,
        speed: f32,
    },
    StopTracer {
        id: i32,
        endpoint: glam::Vec3,
    },
    ShotResults(ShotResult),
    BattleEvents(BattleEvent),
}

impl AvatarUpdate {
    pub(super) fn send(self, mut avatar: AvatarView<'_>) -> std::io::Result<()> {
        match self {
            Self::Arena { kind, data } => avatar.updateArena(kind, Bytes(data)),
            Self::RoundFinished { winner, reason } => avatar.onRoundFinished(winner, reason),
            Self::HitDirection {
                yaw,
                attacker,
                damage,
                explosive,
                victim,
            } => avatar.showOwnVehicleHitDirection(
                yaw,
                EntityId(attacker),
                damage,
                0,
                false,
                explosive,
                EntityId(victim),
                0,
            ),
            Self::Tracer {
                shooter,
                id,
                effects,
                start,
                velocity,
                range,
            } => avatar.showTracer(
                EntityId(shooter),
                id,
                0,
                effects.legacy_index,
                effects.prefab_index,
                effects.kind,
                effects.caliber_mm,
                start,
                velocity,
                0.0,
                range,
                0,
                0,
            ),
            Self::Impact {
                id,
                effects,
                material,
                endpoint,
                direction,
                speed,
            } => avatar.explodeProjectile(
                id,
                effects.legacy_index,
                effects.prefab_index,
                material,
                effects.kind,
                effects.caliber_mm,
                endpoint,
                direction,
                speed,
                Vec::new(),
            ),
            Self::StopTracer { id, endpoint } => avatar.stopTracer(id, endpoint),
            Self::ShotResults(result) => avatar.showShotResults(vec![result]),
            Self::BattleEvents(event) => avatar.onBattleEvents(vec![event]),
        }
    }
}
