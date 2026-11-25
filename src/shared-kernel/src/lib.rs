#![forbid(unsafe_code)]

mod domain;

pub use domain::vehicle_performance::{
    ArmorDevice, ArmorMesh, ArmorPlate, ArmorTriangle, Device, DeviceHealth, ExplosiveShell,
    GunLoading, GunPerformance, Mobility, MountedArmor, ShellBallistics, ShellEffects, ShellKind,
    ShellStun, VehiclePerformance, Wheel, shell_value_at_distance, shell_zero_distance,
};
pub use domain::{
    AccountId,
    battle_outfits::{ArenaSeason, BattleOutfits},
};
