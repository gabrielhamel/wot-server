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
                if !(parameters.spotting_proximity_metres < parameters.spotting_near_metres
                    && parameters.spotting_near_metres < parameters.spotting_middle_metres
                    && parameters.spotting_middle_metres < parameters.spotting_max_metres)
                {
                    return Err(InvalidBattleParameters);
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
    "Spotting" {
        spotting_proximity_metres: f32 = 50.0, 0.0..=100.0, false, "Proximity spotting range (metres)";
        spotting_near_metres: f32 = 150.0, 1.0..=300.0, false, "Near spotting check range (metres)";
        spotting_middle_metres: f32 = 270.0, 1.0..=444.0, false, "Middle spotting check range (metres)";
        spotting_max_metres: f32 = 445.0, 1.0..=445.0, false, "Maximum direct spotting range (metres)";
        spotting_proximity_ticks: u32 = 1, 1..=20, true, "Proximity check interval (100 ms ticks)";
        spotting_near_ticks: u32 = 5, 1..=20, true, "Near check interval (100 ms ticks)";
        spotting_middle_ticks: u32 = 10, 1..=20, true, "Middle check interval (100 ms ticks)";
        spotting_far_ticks: u32 = 20, 1..=50, true, "Far check interval (100 ms ticks)";
        spotting_port_ticks: u32 = 20, 1..=100, true, "View port alternation interval (100 ms ticks)";
        spotting_retention_ticks: u32 = 100, 0..=600, true, "Spotted retention (100 ms ticks)";
        spotting_sixth_sense_ticks: u32 = 30, 0..=300, true, "Sixth Sense delay (100 ms ticks)";
        spotting_shot_ticks: u32 = 50, 0..=300, true, "Firing concealment penalty duration (100 ms ticks)";
        spotting_foliage_near_metres: f32 = 15.0, 0.0..=100.0, false, "Nearby foliage distance (metres)";
        spotting_leafy_cover: f32 = 0.5, 0.0..=1.0, false, "Leafy vegetation concealment";
        spotting_leafless_cover: f32 = 0.25, 0.0..=1.0, false, "Leafless vegetation concealment";
        spotting_foliage_cap: f32 = 0.8, 0.0..=1.0, false, "Maximum vegetation concealment";
        spotting_foliage_shot_fraction: f32 = 0.3, 0.0..=1.0, false, "Nearby foliage concealment retained after firing";
        spotting_crown_base_fraction: f32 = 0.4, 0.0..=1.0, false, "High tree crown base (fraction of height)";
        spotting_draw_metres: f32 = 565.0, 0.0..=1500.0, false, "Vehicle draw range (metres)";
    }
    "Network replication" {
        parked_vehicle_pose_interval_ticks: u32 = 5, 1..=5, true, "Other parked vehicle pose interval (100 ms ticks)";
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
    }
    "Hull physics" {
        suspension_travel_metres: f32 = 0.3, 0.05..=0.8, false, "Suspension travel (metres)";
        suspension_frequency_hz: f32 = 3.0, 1.0..=5.0, false, "Suspension frequency (Hz)";
        suspension_damping_ratio: f32 = 0.8, 0.1..=1.5, false, "Suspension damping ratio";
        lateral_grip_response_seconds: f32 = 0.08, 0.05..=2.0, false, "Lateral grip response (seconds)";
    }
    "World contacts" {
        contact_skin_metres: f32 = 0.025, 0.0..=0.25, false, "Collision clearance (metres)";
        hull_height_metres: f32 = 2.0, 0.25..=6.0, false, "Fallback hull height (metres)";
        fallback_hull_half_width_metres: f32 = 1.25, 0.25..=5.0, false, "Fallback hull half-width (metres)";
        fallback_hull_half_length_metres: f32 = 2.5, 0.25..=8.0, false, "Fallback hull half-length (metres)";
        fallback_mass_kg: f32 = 30_000.0, 100.0..=200_000.0, false, "Fallback vehicle mass (kg)";
        fallback_engine_power_watts: f32 = 300_000.0, 100.0..=1_000_000.0, false, "Fallback engine power (watts)";
        scenery_push_force_scale: f32 = 0.0002, 0.0..=0.002, false, "Scenery push force scale";
        hard_surface_grip: f32 = 0.58, 0.0..=2.0, false, "Hard surface grip";
        medium_surface_grip: f32 = 0.50, 0.0..=2.0, false, "Medium surface grip";
        soft_surface_grip: f32 = 0.42, 0.0..=2.0, false, "Soft surface grip";
        tip_terrain_roll_radians: f32 = 1.0, 0.05..=1.57, false, "Tip roll threshold (radians)";
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
    }
    "Fire" {
        engine_compartment_width: f32 = 0.7, 0.0..=1.0, false, "Estimated engine compartment width (hull fraction)";
        engine_compartment_height: f32 = 0.75, 0.0..=1.0, false, "Estimated engine compartment height (hull fraction)";
        engine_compartment_length: f32 = 0.35, 0.0..=1.0, false, "Estimated engine compartment length (hull fraction)";
        engine_ignition_min_damage: u16 = 21, 0..=u16::MAX, true, "Minimum module damage for engine ignition (HP)";
        fire_tick_seconds: f32 = 0.5, 0.1..=10.0, false, "Fire tick interval (seconds)";
        fire_ticks: u8 = 12, 0..=30, true, "Fire damage ticks";
        fire_damage_percent: f32 = 4.0, 0.01..=25.0, false, "Maximum HP lost on first fire tick (%)";
    }
    "Tracks" {
        track_repair_time_scale: f32 = 1.0, 0.1..=10.0, false, "Automatic track repair time multiplier";
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
