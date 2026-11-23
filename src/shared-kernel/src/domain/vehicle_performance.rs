use glam::Vec3;
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct VehiclePerformance {
    pub forward_speed: f32,
    pub reverse_speed: f32,
    pub hull_traverse: f32,
    pub mobility: Mobility,
    pub turret_pivot: Option<Vec3>,
    pub gun: GunPerformance,
}

#[derive(Clone, Debug)]
pub struct Mobility {
    pub power_per_kg: f32,
    pub terrain_resistance: [f32; 3],
    pub rolling_friction: [f32; 3],
    pub track_half_width: f32,
    pub wheels: Vec<Wheel>,
    pub turns_in_place: bool,
}

impl Default for Mobility {
    fn default() -> Self {
        Self {
            power_per_kg: 14.0,
            terrain_resistance: [1.0, 1.3, 2.0],
            rolling_friction: [0.045, 0.065, 0.105],
            track_half_width: 1.25,
            wheels: Vec::new(),
            turns_in_place: true,
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
        }
    }
}
