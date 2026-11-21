use glam::Vec3;
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct VehiclePerformance {
    pub forward_speed: f32,
    pub reverse_speed: f32,
    pub hull_traverse: f32,
    pub turret_pivot: Option<Vec3>,
    pub gun: GunPerformance,
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
    pub caliber_mm: f32,
    pub legacy_index: u8,
    pub prefab_index: u8,
}

impl Default for ShellEffects {
    fn default() -> Self {
        Self {
            kind: 2,
            caliber_mm: 45.0,
            legacy_index: 0,
            prefab_index: 0,
        }
    }
}
