use glam::Vec3;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Debug)]
pub struct VehiclePerformance {
    pub forward_speed: f32,
    pub reverse_speed: f32,
    pub hull_traverse: f32,
    pub mobility: Mobility,
    pub turret_pivot: Option<Vec3>,
    pub gun: GunPerformance,
    pub armor: [Option<MountedArmor>; 4],
    pub device_health: BTreeMap<Device, DeviceHealth>,
}

#[derive(Clone, Debug)]
pub struct Mobility {
    pub mass_kg: f32,
    pub power_per_kg: f32,
    pub terrain_resistance: [f32; 3],
    pub turning_resistance: f32,
    pub rolling_friction: [f32; 3],
    pub track_half_width: f32,
    pub track_half_length: f32,
    pub collision_bounds: Option<[Vec3; 2]>,
    pub wheels: Vec<Wheel>,
    pub turns_in_place: bool,
    pub rotation_around_center: bool,
    pub track_lock: bool,
    pub traverse_limit: Option<f32>,
    pub max_climb_angle: Option<f32>,
}

impl Default for Mobility {
    fn default() -> Self {
        Self {
            mass_kg: 30_000.0,
            power_per_kg: 14.0,
            terrain_resistance: [1.0, 1.3, 2.0],
            turning_resistance: 1.0,
            rolling_friction: [0.045, 0.065, 0.105],
            track_half_width: 1.25,
            track_half_length: 2.5,
            collision_bounds: None,
            wheels: Vec::new(),
            turns_in_place: true,
            rotation_around_center: true,
            track_lock: false,
            traverse_limit: None,
            max_climb_angle: None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Wheel {
    pub center: glam::Vec2,
    pub steering_limit: Option<f32>,
}

#[derive(Clone, Debug)]
pub struct GunPerformance {
    pub turret_traverse: f32,
    pub gun_traverse: f32,
    pub yaw_limits: Option<[f32; 2]>,
    pub min_pitch: Vec<(f32, f32)>,
    pub max_pitch: Vec<(f32, f32)>,
    pub aiming_time: f32,
    pub base_dispersion_angle: Option<f32>,
    pub shot_position: Option<Vec3>,
    pub loading: GunLoading,
    pub shell_damage: BTreeMap<i32, u16>,
    pub shell_ranges: BTreeMap<i32, f32>,
    pub shell_effects: BTreeMap<i32, ShellEffects>,
    pub shell_ballistics: BTreeMap<i32, ShellBallistics>,
}

#[derive(Clone, Debug)]
pub struct GunLoading {
    pub capacity: u16,
    pub reload_time: f32,
    pub intra_clip_time: f32,
    pub forced_reload_time: f32,
    pub autoreload_times: Vec<f32>,
}

#[derive(Clone, Copy, Debug)]
pub struct ShellEffects {
    pub kind: u8,
    pub damage_log_type: Option<u8>,
    pub premium: bool,
    pub caliber_mm: f32,
    pub legacy_index: u8,
    pub prefab_index: u8,
    pub pierced_sticker: Option<u8>,
    pub resisted_sticker: Option<u8>,
}

impl Default for ShellEffects {
    fn default() -> Self {
        Self {
            kind: 2,
            damage_log_type: None,
            premium: false,
            caliber_mm: 45.0,
            legacy_index: 0,
            prefab_index: 0,
            pierced_sticker: None,
            resisted_sticker: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct MountedArmor {
    pub offset: Vec3,
    pub mesh: Arc<ArmorMesh>,
}

#[derive(Debug)]
pub struct ArmorMesh {
    pub bounds: [Vec3; 2],
    pub triangles: Vec<ArmorTriangle>,
}

#[derive(Clone, Copy, Debug)]
pub struct ArmorTriangle {
    pub vertices: [Vec3; 3],
    pub material: u32,
    pub plate: ArmorPlate,
}

#[derive(Clone, Copy, Debug)]
pub struct ArmorPlate {
    pub thickness_mm: f32,
    pub damage_factor: f32,
    pub angled: bool,
    pub ricochets: bool,
    pub caliber_ricochet: bool,
    pub caliber_normalization: bool,
    pub once: bool,
    pub device: Option<ArmorDevice>,
}

#[derive(Clone, Copy, Debug)]
pub enum ShellKind {
    ArmorPiercing,
    HollowCharge {
        jet_loss_per_metre: f32,
    },
    HighExplosive {
        shield_penetration: bool,
        obstacle_penetration: bool,
    },
    Other,
}

#[derive(Clone, Copy, Debug)]
pub struct ShellBallistics {
    pub kind: ShellKind,
    pub caliber_mm: f32,
    pub penetration_mm: [f32; 2],
    pub penetration_randomization: f32,
    pub damage_randomization: f32,
    pub uniform_penetration: bool,
    pub uniform_damage: bool,
    pub far_damage: f32,
    pub damage_limited_range: f32,
    pub device_damage: [f32; 2],
    pub explosive: Option<ExplosiveShell>,
    pub speed: f32,
    pub gravity: f32,
    pub normalization: f32,
    pub ricochet_cos: f32,
    pub trace_ricochet: bool,
    pub obstacle_damage: Option<f32>,
    pub obstacle_penetration_loss: f32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ExplosiveShell {
    pub scenery_radius: f32,
    pub artillery_radius: f32,
    pub splash_factor: f32,
    pub edge_factor: f32,
    pub armor_absorption: f32,
    pub fragment_damage: [f32; 2],
    pub fragment_absorption: f32,
    pub spall_damage: [f32; 2],
    pub spall_radius: f32,
    pub spall_cone_cos: f32,
    pub piercing_spalls: bool,
    pub spall_device_damage: [f32; 2],
    pub stun: Option<ShellStun>,
}

#[derive(Clone, Copy, Debug)]
pub struct ShellStun {
    pub radius: f32,
    pub duration: f32,
    pub guaranteed: f32,
    pub damage_coefficient: f32,
    pub minimum: f32,
    pub turret_traverse: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Device {
    TrackLeft,
    TrackRight,
    Wheel,
    Engine,
    Fuel,
    AmmoRack,
    Gun,
    Turret,
    Optics,
    Radio,
    Crew(u32),
}

#[derive(Clone, Copy, Debug)]
pub struct ArmorDevice {
    pub device: Device,
    pub projectile_chance: f32,
    pub explosion_chance: f32,
    pub health: Option<DeviceHealth>,
}

#[derive(Clone, Copy, Debug)]
pub struct DeviceHealth {
    pub maximum: u16,
    pub repaired: u16,
    pub repair_seconds: f32,
    pub fire_chance: f32,
}

/// Default distance anchors from the client's common shell helpers.
pub fn shell_value_at_distance([near, far]: [f32; 2], metres: f32) -> f32 {
    (near + (far - near) * ((metres - 50.0).max(0.0) / 450.0)).max(0.0)
}

pub fn shell_zero_distance([near, far]: [f32; 2]) -> f32 {
    if far < near {
        50.0 + 450.0 * near / (near - far)
    } else {
        f32::INFINITY
    }
}
