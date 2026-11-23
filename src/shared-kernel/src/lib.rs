#![forbid(unsafe_code)]

mod domain;

pub use domain::AccountId;
pub use domain::vehicle_performance::{
    ArmorMesh, ArmorPlate, ArmorTriangle, GunLoading, GunPerformance, Mobility, MountedArmor,
    ShellBallistics, ShellEffects, ShellKind, VehiclePerformance, Wheel,
};
