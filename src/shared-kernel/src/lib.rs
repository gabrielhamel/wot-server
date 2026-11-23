#![forbid(unsafe_code)]

mod domain;

pub use domain::AccountId;
pub use domain::vehicle_performance::{
    GunLoading, GunPerformance, Mobility, ShellEffects, VehiclePerformance, Wheel,
};
