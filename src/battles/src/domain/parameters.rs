use super::devices::DEVICE_STRIKE_CAPACITY;
use serde::Serialize;
use std::{collections::BTreeMap, f32::consts::PI, time::Duration};

pub const TICK: Duration = Duration::from_millis(100);
const MAX_ARENA_SECONDS: u64 = u32::MAX as u64 * TICK.as_millis() as u64 / 1000;
// Greatest finite f32 whose square is also finite.
const MAX_SQUARED_DISTANCE_METRES: f32 = f32::from_bits(0x5f7f_ffff);
pub(crate) const PHYSICS_HZ: u32 = 60;
pub(crate) const PHYSICS_STEP_SECONDS: f32 = 1.0 / PHYSICS_HZ as f32;
pub(crate) const MIN_COLLIDER_HALF_EXTENT: f32 = 0.001;
// Preserve the minimum collider scale in a single-precision coordinate.
const MAX_PHYSICS_DISTANCE: f32 = MIN_COLLIDER_HALF_EXTENT / f32::EPSILON;
pub(crate) const MAX_LINEAR_SPEED: f32 = MAX_PHYSICS_DISTANCE / PHYSICS_STEP_SECONDS;
pub(crate) const MAX_ANGULAR_SPEED: f32 = PI / PHYSICS_STEP_SECONDS;
pub(crate) const MAX_GRAVITY: f32 = MAX_LINEAR_SPEED / PHYSICS_STEP_SECONDS;
// Keep linear momentum at the solver's speed limit within a finite squared norm.
const MAX_FALLBACK_MASS: f32 = MAX_SQUARED_DISTANCE_METRES / MAX_LINEAR_SPEED;
// Keep the mass-to-volume ratio normal at the largest fallback cuboid.
const MIN_FALLBACK_MASS: f32 =
    f32::MIN_POSITIVE * 8.0 * MAX_PHYSICS_DISTANCE * MAX_PHYSICS_DISTANCE * MAX_PHYSICS_DISTANCE;
const MIN_POSITIVE_RESPONSE_SECONDS: f32 = f32::from_bits(1);
const MAX_FLIGHT_SEGMENTS: usize = 640;
const MAX_NORMAL_ROLL_SAMPLES: usize = 12;
const MAX_TRACK_BASE_REPAIR_SECONDS: f64 = 120.0;

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
    ($($group:literal { $($field:ident: $kind:ty = $default:literal, $min:expr, $max:expr, $integer:literal, $label:literal;)* })*) => {
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

impl BattleParameters {
    pub fn validate_round_length(
        &self,
        duration_seconds: u64,
    ) -> Result<(), InvalidBattleParameters> {
        let phase_seconds =
            self.load_timeout_seconds + self.countdown_seconds + self.results_seconds;
        if phase_seconds.saturating_add(duration_seconds) > MAX_ARENA_SECONDS {
            return Err(InvalidBattleParameters);
        }
        Ok(())
    }
}

battle_parameters! {
    "Round timing" {
        load_timeout_seconds: u64 = 60, 0, MAX_ARENA_SECONDS, true, "Loading timeout (seconds)";
        countdown_seconds: u64 = 10, 0, MAX_ARENA_SECONDS, true, "Countdown (seconds)";
        results_seconds: u64 = 6, 0, MAX_ARENA_SECONDS, true, "Results before garage return (seconds)";
    }
    "Spotting" {
        spotting_proximity_metres: f32 = 50.0, 0.0, MAX_SQUARED_DISTANCE_METRES, false, "Proximity spotting range (metres)";
        spotting_near_metres: f32 = 150.0, 0.0, MAX_SQUARED_DISTANCE_METRES, false, "Near spotting check range (metres)";
        spotting_middle_metres: f32 = 270.0, 0.0, MAX_SQUARED_DISTANCE_METRES, false, "Middle spotting check range (metres)";
        spotting_max_metres: f32 = 445.0, 0.0, MAX_SQUARED_DISTANCE_METRES, false, "Maximum direct spotting range (metres)";
        spotting_proximity_ticks: u32 = 1, 1, u32::MAX, true, "Proximity check interval (100 ms ticks)";
        spotting_near_ticks: u32 = 5, 1, u32::MAX, true, "Near check interval (100 ms ticks)";
        spotting_middle_ticks: u32 = 10, 1, u32::MAX, true, "Middle check interval (100 ms ticks)";
        spotting_far_ticks: u32 = 20, 1, u32::MAX, true, "Far check interval (100 ms ticks)";
        spotting_port_ticks: u32 = 20, 1, u32::MAX, true, "View port alternation interval (100 ms ticks)";
        spotting_retention_ticks: u32 = 100, 0, u32::MAX, true, "Spotted retention (100 ms ticks)";
        spotting_sixth_sense_ticks: u32 = 30, 0, u32::MAX, true, "Sixth Sense delay (100 ms ticks)";
        spotting_shot_ticks: u32 = 50, 0, u32::MAX, true, "Firing concealment penalty duration (100 ms ticks)";
        spotting_foliage_near_metres: f32 = 15.0, 0.0, MAX_SQUARED_DISTANCE_METRES, false, "Nearby foliage distance (metres)";
        spotting_leafy_cover: f32 = 0.5, 0.0, 1.0, false, "Leafy vegetation concealment";
        spotting_leafless_cover: f32 = 0.25, 0.0, 1.0, false, "Leafless vegetation concealment";
        spotting_foliage_cap: f32 = 0.8, 0.0, 1.0, false, "Maximum vegetation concealment";
        spotting_foliage_shot_fraction: f32 = 0.3, 0.0, 1.0, false, "Nearby foliage concealment retained after firing";
        spotting_crown_base_fraction: f32 = 0.4, 0.0, 1.0, false, "High tree crown base (fraction of height)";
        spotting_draw_metres: f32 = 565.0, 0.0, MAX_SQUARED_DISTANCE_METRES, false, "Vehicle draw range (metres)";
    }
    "Network replication" {
        parked_vehicle_pose_interval_ticks: u32 = 5, 1, u32::MAX, true, "Other parked vehicle pose interval (100 ms ticks)";
    }
    "Vehicle movement" {
        gravity_mps2: f32 = 9.81, -MAX_GRAVITY, MAX_GRAVITY, false, "Gravity (m/s²)";
        coast_deceleration_mps2: f32 = 14.0, 0.0, f32::MAX, false, "Coast deceleration (m/s²)";
        brake_deceleration_mps2: f32 = 24.0, 0.0, f32::MAX, false, "Brake deceleration (m/s²)";
        turn_response_seconds: f32 = 0.2, f32::MIN_POSITIVE, f32::MAX, false, "Turn response (seconds)";
        track_lock_min_speed_mps: f32 = 2.0, 0.0, f32::MAX, false, "Track-lock minimum speed (m/s)";
        track_lock_deceleration_mps2: f32 = 6.0, 0.0, f32::MAX, false, "Track-lock deceleration (m/s²)";
        track_lock_response_seconds: f32 = 0.1, f32::MIN_POSITIVE, f32::MAX, false, "Track-lock response (seconds)";
        downhill_speed_limit_ratio: f32 = 1.5, 0.0, f32::MAX, false, "Downhill speed multiplier";
        traction_multiplier: f32 = 1.13, 0.0, f32::MAX, false, "Traction multiplier";
        traction_acceleration_mps2: f32 = 6.0, 0.0, MAX_GRAVITY, false, "Traction acceleration cap (m/s²)";
        half_speed_ratio: f32 = 0.5, 0.0, f32::MAX, false, "Half-speed drive multiplier";
        quarter_speed_ratio: f32 = 0.25, 0.0, f32::MAX, false, "Quarter-speed drive multiplier";
        wheel_turn_creep_mps: f32 = 2.0, 0.0, f32::MAX, false, "Wheeled turning creep speed (m/s)";
        moving_turn_min_speed_mps: f32 = 0.5, 0.0, f32::MAX, false, "Moving turn minimum speed (m/s)";
        moving_turn_multiplier: f32 = 0.85, 0.0, f32::MAX, false, "Moving turn multiplier";
        track_lock_heading_response: f32 = 2.0, 0.0, f32::MAX, false, "Track-lock heading response (per second)";
    }
    "Hull physics" {
        suspension_travel_metres: f32 = 0.3, 0.0, MAX_PHYSICS_DISTANCE, false, "Suspension travel (metres)";
        suspension_frequency_hz: f32 = 3.0, 0.0, f32::MAX, false, "Suspension frequency (Hz)";
        suspension_damping_ratio: f32 = 0.8, 0.0, f32::MAX, false, "Suspension damping ratio";
        lateral_grip_response_seconds: f32 = 0.08, MIN_POSITIVE_RESPONSE_SECONDS, f32::MAX, false, "Lateral grip response (seconds)";
    }
    "World contacts" {
        contact_skin_metres: f32 = 0.025, 0.0, MAX_PHYSICS_DISTANCE, false, "Collision clearance (metres)";
        hull_height_metres: f32 = 2.0, 2.0 * MIN_COLLIDER_HALF_EXTENT, 2.0 * MAX_PHYSICS_DISTANCE, false, "Fallback hull height (metres)";
        fallback_hull_half_width_metres: f32 = 1.25, MIN_COLLIDER_HALF_EXTENT, MAX_PHYSICS_DISTANCE, false, "Fallback hull half-width (metres)";
        fallback_hull_half_length_metres: f32 = 2.5, MIN_COLLIDER_HALF_EXTENT, MAX_PHYSICS_DISTANCE, false, "Fallback hull half-length (metres)";
        fallback_mass_kg: f32 = 30_000.0, MIN_FALLBACK_MASS, MAX_FALLBACK_MASS, false, "Fallback vehicle mass (kg)";
        fallback_engine_power_watts: f32 = 300_000.0, 0.0, f32::MAX, false, "Fallback engine power (watts)";
        scenery_push_force_scale: f32 = 0.0002, 0.0, f32::MAX, false, "Scenery push force scale";
        hard_surface_grip: f32 = 0.58, 0.0, f32::MAX, false, "Hard surface grip";
        medium_surface_grip: f32 = 0.50, 0.0, f32::MAX, false, "Medium surface grip";
        soft_surface_grip: f32 = 0.42, 0.0, f32::MAX, false, "Soft surface grip";
        tip_terrain_roll_radians: f32 = 1.0, 0.0, std::f32::consts::PI, false, "Tip roll threshold (radians)";
        settle_time_seconds: f32 = 3.0, 0.0, MAX_ARENA_SECONDS, false, "Overturned settling time (seconds)";
        recovery_time_seconds: f32 = 15.0, 0.0, MAX_ARENA_SECONDS, false, "Overturned recovery time (seconds)";
    }
    "Projectiles and damage" {
        flight_step_seconds: f32 = 0.125, 0.0, MAX_ARENA_SECONDS as f64 / MAX_FLIGHT_SEGMENTS as f64, false, "Ballistic integration step (seconds)";
        max_flight_segments: usize = 320, 0, MAX_FLIGHT_SEGMENTS, true, "Maximum ballistic segments";
        fallback_turret_height_metres: f32 = 1.35, 0.0, MAX_PHYSICS_DISTANCE, false, "Fallback turret pivot height (metres)";
        fallback_gun_ray_speed_mps: f32 = 450.0, 0.0, MAX_SQUARED_DISTANCE_METRES, false, "Fallback gun ray speed (m/s)";
        projectile_contact_clearance_metres: f32 = 0.002, 0.0, f32::MAX, false, "Projectile contact clearance (metres)";
        ricochet_range_calibers: f32 = 10.0, 0.0, f32::MAX, false, "Ricochet range (calibers)";
        ap_ricochet_penetration_retention: f32 = 0.75, 0.0, 1.0, false, "AP ricochet penetration retained";
        ap_ricochet_overmatch_ratio: f32 = 3.0, 0.0, f32::MAX, false, "AP ricochet overmatch ratio";
        ap_normalization_overmatch_ratio: f32 = 2.0, 0.0, f32::MAX, false, "AP normalization overmatch ratio";
        ap_normalization_caliber_factor: f32 = 0.7, 0.0, f32::MAX, false, "AP caliber normalization factor";
        internal_projectile_range_calibers: f32 = 10.0, 0.0, f32::MAX, false, "Internal projectile range (calibers)";
        he_screen_penetration_loss_factor: f32 = 3.0, 0.0, f32::MAX, false, "HE screen penetration loss multiplier";
        normal_roll_standard_deviation: f32 = 0.33, 0.0, f32::MAX, false, "Normal damage and penetration roll spread";
        normal_roll_max_samples: usize = 4, 0, MAX_NORMAL_ROLL_SAMPLES, true, "Maximum normal roll samples";
        max_device_hits: usize = 12, 0, DEVICE_STRIKE_CAPACITY, true, "Maximum device hits per contact";
    }
    "Fire" {
        engine_compartment_width: f32 = 0.7, 0.0, 1.0, false, "Estimated engine compartment width (hull fraction)";
        engine_compartment_height: f32 = 0.75, 0.0, 1.0, false, "Estimated engine compartment height (hull fraction)";
        engine_compartment_length: f32 = 0.35, 0.0, 1.0, false, "Estimated engine compartment length (hull fraction)";
        engine_ignition_min_damage: u16 = 21, 0, u16::MAX, true, "Minimum module damage for engine ignition (HP)";
        fire_tick_seconds: f32 = 0.5, 0.0, MAX_ARENA_SECONDS, false, "Fire tick interval (seconds)";
        fire_ticks: u8 = 12, 0, u8::MAX, true, "Fire damage ticks";
        fire_damage_percent: f32 = 4.0, f32::MIN_POSITIVE, 100.0, false, "Maximum HP lost on first fire tick (%)";
    }
    "Tracks" {
        track_repair_time_scale: f32 = 1.0, 0.0, MAX_ARENA_SECONDS as f64 / MAX_TRACK_BASE_REPAIR_SECONDS, false, "Automatic track repair time multiplier";
    }
    "Tracer visibility" {
        tracer_draw_distance_metres: f32 = 565.0, 0.0, MAX_SQUARED_DISTANCE_METRES, false, "Known shooter tracer range (metres)";
        hidden_tracer_distance_metres: f32 = 445.0, 0.0, MAX_SQUARED_DISTANCE_METRES, false, "Hidden shooter tracer range (metres)";
        hidden_tracer_view_cosine: f32 = 0.906_307_8, -1.0, 1.0, false, "Hidden shooter view cone cosine";
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

    #[test]
    fn user_settings_accept_widened_bounds_and_reject_overflow() {
        let mut overrides = BTreeMap::from([
            ("spotting_retention_ticks".to_owned(), f64::from(u32::MAX)),
            (
                "parked_vehicle_pose_interval_ticks".to_owned(),
                f64::from(u32::MAX),
            ),
            ("fire_ticks".to_owned(), f64::from(u8::MAX)),
            (
                "normal_roll_standard_deviation".to_owned(),
                f64::from(f32::MAX),
            ),
            (
                "spotting_max_metres".to_owned(),
                f64::from(MAX_SQUARED_DISTANCE_METRES),
            ),
            ("track_repair_time_scale".to_owned(), 0.000_123_456_789),
        ]);
        let parameters = BattleParameters::from_overrides(&overrides).unwrap();
        assert_eq!(parameters.spotting_retention_ticks, u32::MAX);
        assert_eq!(parameters.fire_ticks, u8::MAX);
        assert_eq!(parameters.normal_roll_standard_deviation, f32::MAX);
        assert!(parameters.spotting_max_metres.powi(2).is_finite());
        assert_eq!(parameters.track_repair_time_scale, 0.000_123_456_79);

        for seconds in [255.5, 256.0, f64::NAN, f64::INFINITY] {
            overrides.insert("fire_ticks".to_owned(), seconds);
            assert!(BattleParameters::from_overrides(&overrides).is_err());
        }
        for (key, value) in [
            ("spotting_port_ticks", 0.0),
            ("spotting_near_metres", 270.0),
            (
                "spotting_max_metres",
                f64::from(MAX_SQUARED_DISTANCE_METRES) * 2.0,
            ),
            ("max_flight_segments", (MAX_FLIGHT_SEGMENTS + 1) as f64),
            (
                "normal_roll_max_samples",
                (MAX_NORMAL_ROLL_SAMPLES + 1) as f64,
            ),
            ("max_device_hits", (DEVICE_STRIKE_CAPACITY + 1) as f64),
        ] {
            assert!(
                BattleParameters::from_overrides(&BTreeMap::from([(key.to_owned(), value)]))
                    .is_err()
            );
        }
    }

    #[test]
    fn physics_ranges_follow_solver_limits() {
        let settings = |values: &[(&str, f64)]| {
            BattleParameters::from_overrides(
                &values
                    .iter()
                    .map(|&(key, value)| (key.to_owned(), value))
                    .collect(),
            )
        };

        for gravity in [-MAX_GRAVITY, -42.125, 100.0, MAX_GRAVITY] {
            let parameters = settings(&[("gravity_mps2", f64::from(gravity))]).unwrap();
            assert_eq!(parameters.gravity_mps2, gravity);
        }
        assert!(settings(&[("gravity_mps2", f64::from(MAX_GRAVITY.next_up()))]).is_err());

        assert!(settings(&[("suspension_frequency_hz", 6.0)]).is_ok());
        assert!(
            settings(&[
                ("suspension_frequency_hz", 18.0),
                ("suspension_damping_ratio", 1.0)
            ])
            .is_ok()
        );
        assert!(
            settings(&[
                ("suspension_frequency_hz", f64::from(f32::MAX)),
                ("suspension_damping_ratio", f64::from(f32::MAX)),
                ("contact_skin_metres", 0.5),
                ("fallback_mass_kg", 250_000.0),
                ("hard_surface_grip", 3.5),
                ("flight_step_seconds", 0.75),
            ])
            .is_ok()
        );
    }

    #[test]
    fn phase_durations_share_the_arena_clock_with_active_play() {
        let parameters = BattleParameters {
            load_timeout_seconds: 0,
            countdown_seconds: MAX_ARENA_SECONDS - 600,
            results_seconds: 0,
            ..BattleParameters::default()
        };

        assert!(parameters.validate_round_length(600).is_ok());
        assert!(parameters.validate_round_length(601).is_err());
    }
}
