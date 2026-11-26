use super::devices::DEVICE_STRIKE_CAPACITY;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize)]
pub struct BattleParameterDefinition {
    pub key: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub default: f64,
    pub min: f64,
    pub max: f64,
    pub integer: bool,
}

#[derive(Debug, thiserror::Error)]
#[error("A battle parameter is unknown or outside its supported range.")]
pub struct InvalidBattleParameters;

macro_rules! battle_parameters {
    ($($group:literal { $($field:ident: $kind:ty = $default:literal, $min:literal ..= $max:expr, $integer:literal, $label:literal;)* })*) => {
        #[derive(Clone, Copy, Debug)]
        pub struct BattleParameters {
            $($(pub(crate) $field: $kind,)*)*
        }

        impl Default for BattleParameters {
            fn default() -> Self {
                Self { $($($field: $default,)*)* }
            }
        }

        impl BattleParameters {
            pub fn definitions() -> &'static [BattleParameterDefinition] {
                &[$($(BattleParameterDefinition {
                    key: stringify!($field),
                    label: $label,
                    group: $group,
                    default: $default as f64,
                    min: $min as f64,
                    max: $max as f64,
                    integer: $integer,
                },)*)*]
            }

            pub fn from_overrides(overrides: &BTreeMap<String, f64>) -> Result<Self, InvalidBattleParameters> {
                let mut parameters = Self::default();
                for (key, &value) in overrides {
                    match key.as_str() {
                        $($(stringify!($field)
                            if ($min as f64..=$max as f64).contains(&value)
                                && (!$integer || value.fract() == 0.0) =>
                        {
                            parameters.$field = value as $kind;
                        })*)*
                        _ => return Err(InvalidBattleParameters),
                    }
                }
                Ok(parameters)
            }
        }
    };
}

battle_parameters! {
    "Round timing" {
        load_timeout_seconds: u64 = 60, 1..=180, true, "Loading timeout (seconds)";
        countdown_seconds: u64 = 10, 0..=60, true, "Countdown (seconds)";
        results_seconds: u64 = 6, 0..=60, true, "Results before garage return (seconds)";
    }
    "Network replication" {
        parked_vehicle_pose_interval_ticks: u32 = 5, 1..=5, true, "Parked vehicle pose interval (100 ms ticks)";
    }
    "Vehicle movement" {
        gravity_mps2: f32 = 9.81, 0.0..=30.0, false, "Gravity (m/s²)";
        coast_deceleration_mps2: f32 = 14.0, 0.0..=60.0, false, "Coast deceleration (m/s²)";
        brake_deceleration_mps2: f32 = 24.0, 0.0..=100.0, false, "Brake deceleration (m/s²)";
        turn_response_seconds: f32 = 0.2, 0.01..=5.0, false, "Turn response (seconds)";
        track_lock_min_speed_mps: f32 = 2.0, 0.0..=30.0, false, "Track-lock minimum speed (m/s)";
        track_lock_deceleration_mps2: f32 = 6.0, 0.0..=60.0, false, "Track-lock deceleration (m/s²)";
        track_lock_response_seconds: f32 = 0.1, 0.01..=5.0, false, "Track-lock response (seconds)";
        downhill_speed_limit_ratio: f32 = 1.5, 1.0..=3.0, false, "Downhill speed multiplier";
        traction_multiplier: f32 = 1.13, 0.0..=5.0, false, "Traction multiplier";
        traction_acceleration_mps2: f32 = 6.0, 0.0..=30.0, false, "Traction acceleration cap (m/s²)";
        half_speed_ratio: f32 = 0.5, 0.0..=1.0, false, "Half-speed drive multiplier";
        quarter_speed_ratio: f32 = 0.25, 0.0..=1.0, false, "Quarter-speed drive multiplier";
        wheel_turn_creep_mps: f32 = 2.0, 0.0..=10.0, false, "Wheeled turning creep speed (m/s)";
        moving_turn_min_speed_mps: f32 = 0.5, 0.0..=10.0, false, "Moving turn minimum speed (m/s)";
        moving_turn_multiplier: f32 = 0.85, 0.0..=2.0, false, "Moving turn multiplier";
        track_lock_heading_response: f32 = 2.0, 0.0..=20.0, false, "Track-lock heading response (per second)";
        terrain_contact_tolerance_metres: f32 = 0.35, 0.0..=2.0, false, "Terrain contact tolerance (metres)";
    }
    "World contacts" {
        contact_skin_metres: f32 = 0.025, 0.0..=0.25, false, "Collision clearance (metres)";
        hull_height_metres: f32 = 2.0, 0.25..=6.0, false, "Scenery and fallback hull height (metres)";
        fallback_hull_half_width_metres: f32 = 1.25, 0.25..=5.0, false, "Fallback hull half-width (metres)";
        fallback_hull_half_length_metres: f32 = 2.5, 0.25..=8.0, false, "Fallback hull half-length (metres)";
        fallback_mass_kg: f32 = 30_000.0, 100.0..=200_000.0, false, "Fallback vehicle mass (kg)";
        fallback_engine_power_watts: f32 = 300_000.0, 100.0..=1_000_000.0, false, "Fallback engine power (watts)";
        tipped_hull_clearance_metres: f32 = 0.35, 0.0..=2.0, false, "Overturned hull clearance (metres)";
        scenery_wall_normal_y_limit: f32 = 0.65, 0.0..=1.0, false, "Scenery wall vertical normal limit";
        scenery_push_force_scale: f32 = 0.0002, 0.0..=0.002, false, "Scenery push force scale";
        max_passive_speed_mps: f32 = 12.0, 0.0..=30.0, false, "Maximum passive speed (m/s)";
        contact_drag_mps2: f32 = 4.0, 0.0..=30.0, false, "Passive contact drag (m/s²)";
        hard_surface_grip: f32 = 0.58, 0.0..=2.0, false, "Hard surface grip";
        medium_surface_grip: f32 = 0.50, 0.0..=2.0, false, "Medium surface grip";
        soft_surface_grip: f32 = 0.42, 0.0..=2.0, false, "Soft surface grip";
        tip_terrain_roll_radians: f32 = 1.0, 0.05..=1.57, false, "Tip roll threshold (radians)";
        tip_min_support_roll_radians: f32 = 0.3, 0.0..=1.57, false, "Minimum support roll for tipping (radians)";
        tip_side_speed_mps: f32 = 5.5, 0.1..=20.0, false, "Tipping side speed (m/s)";
        tip_turning_factor: f32 = 0.28, 0.0..=2.0, false, "Turning contribution to tipping";
        righting_side_speed_mps: f32 = 1.5, 0.0..=20.0, false, "Righting side speed (m/s)";
        settle_time_seconds: f32 = 3.0, 0.0..=60.0, false, "Overturned settling time (seconds)";
        recovery_time_seconds: f32 = 15.0, 0.0..=120.0, false, "Overturned recovery time (seconds)";
    }
    "Projectiles and damage" {
        flight_step_seconds: f32 = 0.125, 0.025..=0.5, false, "Ballistic integration step (seconds)";
        max_flight_segments: usize = 320, 1..=640, true, "Maximum ballistic segments";
        fallback_turret_height_metres: f32 = 1.35, 0.0..=6.0, false, "Fallback turret pivot height (metres)";
        fallback_gun_ray_speed_mps: f32 = 450.0, 1.0..=2000.0, false, "Fallback gun ray speed (m/s)";
        projectile_contact_clearance_metres: f32 = 0.002, 0.0001..=0.1, false, "Projectile contact clearance (metres)";
        ricochet_range_calibers: f32 = 10.0, 0.0..=100.0, false, "Ricochet range (calibers)";
        ap_ricochet_penetration_retention: f32 = 0.75, 0.0..=1.0, false, "AP ricochet penetration retained";
        ap_ricochet_overmatch_ratio: f32 = 3.0, 1.0..=10.0, false, "AP ricochet overmatch ratio";
        ap_normalization_overmatch_ratio: f32 = 2.0, 1.0..=10.0, false, "AP normalization overmatch ratio";
        ap_normalization_caliber_factor: f32 = 0.7, 0.0..=2.0, false, "AP caliber normalization factor";
        internal_projectile_range_calibers: f32 = 10.0, 0.0..=100.0, false, "Internal projectile range (calibers)";
        he_screen_penetration_loss_factor: f32 = 3.0, 0.0..=10.0, false, "HE screen penetration loss multiplier";
        normal_roll_standard_deviation: f32 = 0.33, 0.0..=1.0, false, "Normal damage and penetration roll spread";
        normal_roll_max_samples: usize = 4, 1..=12, true, "Maximum normal roll samples";
        max_device_hits: usize = 12, 0..=DEVICE_STRIKE_CAPACITY, true, "Maximum device hits per contact";
        fire_tick_seconds: f32 = 1.0, 0.1..=10.0, false, "Fire tick interval (seconds)";
        fire_ticks: u8 = 5, 0..=30, true, "Fire damage ticks";
        fire_damage_percent: f32 = 1.0, 0.01..=25.0, false, "Maximum HP lost per fire tick (%)";
    }
    "Tracer visibility" {
        tracer_draw_distance_metres: f32 = 565.0, 0.0..=1500.0, false, "Known shooter tracer range (metres)";
        hidden_tracer_distance_metres: f32 = 445.0, 0.0..=1500.0, false, "Hidden shooter tracer range (metres)";
        hidden_tracer_view_cosine: f32 = 0.906_307_8, -1.0..=1.0, false, "Hidden shooter view cone cosine";
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definitions_advertise_valid_unique_defaults() {
        let definitions = BattleParameters::definitions();
        let defaults: BTreeMap<_, _> = definitions
            .iter()
            .map(|field| (field.key.to_owned(), field.default))
            .collect();
        assert_eq!(defaults.len(), definitions.len());
        assert!(BattleParameters::from_overrides(&defaults).is_ok());
    }
}
