use super::*;
use std::sync::Arc;
use wot_shared_kernel::{
    ArmorDevice, ArmorMesh, ArmorTriangle, Device, ExplosiveShell, GunLoading, GunPerformance,
    MountedArmor, ShellBallistics, ShellEffects, VehiclePerformance,
};

fn shot(kind: ShellKind, penetration: f32) -> Shot {
    Shot {
        origin: Vec3::new(0.5, 1.0, -25.0),
        direction: Vec3::Z,
        damage: 100,
        max_distance: 700.0,
        effects: ShellEffects::default(),
        ballistics: ShellBallistics {
            kind,
            caliber_mm: 90.0,
            penetration_mm: [penetration; 2],
            penetration_randomization: 0.0,
            damage_randomization: 0.0,
            uniform_penetration: false,
            uniform_damage: false,
            far_damage: 100.0,
            damage_limited_range: f32::INFINITY,
            device_damage: [0.0; 2],
            explosive: None,
            speed: 500.0,
            gravity: 0.0,
            normalization: 5.0_f32.to_radians(),
            ricochet_cos: 70.0_f32.to_radians().cos(),
            trace_ricochet: true,
            obstacle_damage: None,
            obstacle_penetration_loss: 0.0,
        },
        penetration_roll: 1.0,
        damage_roll: 1.0,
        travelled: 0.0,
        penetration_loss: 0.0,
        ricocheted: false,
        heat_jet: false,
    }
}

fn plate(thickness_mm: f32, damage_factor: f32) -> ArmorPlate {
    ArmorPlate {
        thickness_mm,
        damage_factor,
        angled: true,
        ricochets: true,
        caliber_ricochet: true,
        caliber_normalization: true,
        once: false,
        device: None,
    }
}

fn surface(distance: f32, material: u32, plate: ArmorPlate, angle: f32) -> Surface {
    let angle = angle.to_radians();
    Surface {
        distance,
        material,
        plate,
        cosine: angle.cos(),
        track_damage_factor: 1.0,
        normal: Vec3::new(angle.sin(), 0.0, -angle.cos()),
        segment: ImpactSegment {
            part: 1,
            start: [128, 128, 0],
            end: [128, 128, 255],
        },
    }
}

#[test]
fn ap_normalizes_and_uses_both_caliber_rules() {
    let mut shot = shot(ShellKind::ArmorPiercing, 85.0);
    let angled = surface(30.0, 1, plate(50.0, 1.0), 60.0);
    let hit = shot
        .resolve_surfaces(&[angled], &BattleParameters::default())
        .unwrap();
    assert_eq!(hit.outcome, ArmorOutcome::Blocked);
    assert!(hit.effective_mm < 100.0 && hit.effective_mm > 85.0);

    shot.ballistics.caliber_mm = 120.0;
    assert_eq!(
        shot.resolve_surfaces(&[angled], &BattleParameters::default())
            .unwrap()
            .outcome,
        ArmorOutcome::Penetrated
    );
    let steep = surface(30.0, 1, plate(30.0, 1.0), 75.0);
    shot.ballistics.caliber_mm = 90.0;
    let bounce = shot
        .resolve_surfaces(&[steep], &BattleParameters::default())
        .unwrap();
    assert_eq!(bounce.outcome, ArmorOutcome::Ricochet);
    let reflected = bounce.reflected.unwrap();
    assert!(reflected.dot(steep.normal) > 0.0);
    let onward = shot
        .ricochet_from(bounce, &BattleParameters::default())
        .unwrap();
    assert!(onward.ricocheted);
    assert_eq!(onward.penetration_at(0.0), 85.0 * 0.75);
    assert!(
        (onward.max_distance + BattleParameters::default().projectile_contact_clearance_metres
            - 0.9)
            .abs()
            < 1e-6
    );
    shot.max_distance = bounce.distance + 0.3;
    let onward = shot
        .ricochet_from(bounce, &BattleParameters::default())
        .unwrap();
    assert!(
        (onward.max_distance + BattleParameters::default().projectile_contact_clearance_metres
            - 0.3)
            .abs()
            < 1e-5
    );
    shot.max_distance = bounce.distance;
    assert!(
        shot.ricochet_from(bounce, &BattleParameters::default())
            .is_none()
    );
    assert!(
        onward
            .resolve_surfaces(&[steep], &BattleParameters::default())
            .unwrap()
            .reflected
            .is_none()
    );
    shot.ballistics.caliber_mm = 100.0;
    shot.ballistics.penetration_mm = [45.0; 2];
    assert_eq!(
        shot.resolve_surfaces(&[steep], &BattleParameters::default())
            .unwrap()
            .outcome,
        ArmorOutcome::Blocked,
        "overmatch bypasses ricochet while penetration still depends on effective armor"
    );
}

#[test]
fn heat_uses_remaining_penetration_and_loses_power_across_the_gap() {
    let shot = shot(
        ShellKind::HollowCharge {
            jet_loss_per_metre: 0.5,
        },
        150.0,
    );
    let screen = surface(30.0, 1, plate(20.0, 0.0), 0.0);
    let close = surface(30.2, 2, plate(90.0, 1.0), 0.0);
    let far = surface(31.2, 2, plate(90.0, 1.0), 0.0);
    assert_eq!(
        shot.resolve_surfaces(&[screen, close], &BattleParameters::default())
            .unwrap()
            .outcome,
        ArmorOutcome::Penetrated
    );
    assert_eq!(
        shot.resolve_surfaces(&[screen, far], &BattleParameters::default())
            .unwrap()
            .outcome,
        ArmorOutcome::Blocked
    );
}

#[test]
fn modern_he_spalls_at_main_armor_and_respects_screen_loss() {
    let mut shot = shot(
        ShellKind::HighExplosive {
            shield_penetration: true,
            obstacle_penetration: true,
        },
        100.0,
    );
    shot.ballistics.explosive = Some(ExplosiveShell {
        spall_damage: [50.0; 2],
        ..ExplosiveShell::default()
    });
    let screen = surface(30.0, 1, plate(20.0, 0.0), 0.0);
    let hull = surface(30.3, 2, plate(60.0, 1.0), 0.0);
    let direct = shot
        .resolve_surfaces(&[hull], &BattleParameters::default())
        .unwrap();
    assert_eq!(
        (direct.outcome, direct.damage),
        (ArmorOutcome::Penetrated, 100)
    );
    let screened = shot
        .resolve_surfaces(&[screen, hull], &BattleParameters::default())
        .unwrap();
    assert_eq!(screened.outcome, ArmorOutcome::Spall);
    assert!((1..50).contains(&screened.damage));
    assert_eq!(screened.material, 2);
    assert_eq!(screened.remaining_mm, 40.0);

    let thick_screen = surface(30.0, 1, plate(110.0, 0.0), 0.0);
    let stopped = shot
        .resolve_surfaces(&[thick_screen, hull], &BattleParameters::default())
        .unwrap();
    assert_eq!(
        (stopped.outcome, stopped.damage),
        (ArmorOutcome::ScreenBlocked, 0)
    );
    let no_hull = shot
        .resolve_surfaces(&[screen], &BattleParameters::default())
        .unwrap();
    assert_eq!(
        (no_hull.outcome, no_hull.damage),
        (ArmorOutcome::ScreenPenetrated, 0)
    );
}

#[test]
fn a_ray_can_strike_an_off_center_part_or_pass_through_empty_space() {
    let mesh = Arc::new(ArmorMesh {
        engine_compartment: None,
        bounds: [Vec3::new(-1.0, 0.0, -0.2), Vec3::new(1.0, 2.0, 0.2)],
        triangles: vec![ArmorTriangle {
            vertices: [
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 1.0, 0.0),
                Vec3::new(0.0, 2.0, 0.0),
            ],
            material: 1,
            plate: plate(40.0, 1.0),
        }],
    });
    let armor = [
        None,
        None,
        Some(MountedArmor {
            offset: Vec3::ZERO,
            mesh,
        }),
        None,
    ];
    let tank = Tank::new(
        BattleParameters::default(),
        Vec3::ZERO,
        Vec3::ZERO,
        100,
        vec![],
        Some(VehiclePerformance {
            forward_speed: 12.0,
            reverse_speed: 5.0,
            hull_traverse: 0.5,
            mobility: Default::default(),
            turret_pivot: Some(Vec3::ZERO),
            armor,
            spotting: None,
            device_health: Default::default(),
            gun: GunPerformance {
                turret_traverse: 0.0,
                gun_traverse: 0.0,
                yaw_limits: None,
                min_pitch: vec![],
                max_pitch: vec![],
                aiming_time: 0.0,
                base_dispersion_angle: None,
                shot_position: Some(Vec3::ZERO),
                loading: GunLoading {
                    capacity: 1,
                    reload_time: 1.0,
                    intra_clip_time: 0.0,
                    forced_reload_time: 0.0,
                    autoreload_times: vec![],
                },
                shell_damage: Default::default(),
                shell_ranges: Default::default(),
                shell_effects: Default::default(),
                shell_ballistics: Default::default(),
            },
        }),
    );
    let mut shot = shot(ShellKind::ArmorPiercing, 100.0);
    let path = shot.armor_path(&tank, shot.max_distance).unwrap();
    assert_eq!(path.first_distance(), 25.0);
    let hit = shot
        .resolve_armor(42, path, &BattleParameters::default())
        .unwrap();
    assert_eq!(hit.distance, 25.0);
    assert_eq!(hit.segment.part, 2);
    assert_eq!(hit.segment.start[0], 191);
    assert_eq!(hit.segment.end[0], 191);
    assert!(hit.segment.start[2] < hit.segment.end[2]);
    assert!(shot.armor_path(&tank, 24.9).is_none());
    shot.origin.x = -0.5;
    assert!(shot.armor_path(&tank, shot.max_distance).is_none());
}

#[test]
fn disabling_combat_rng_uses_nominal_rolls_and_guarantees_module_and_crew_strikes() {
    let mut shot = shot(ShellKind::ArmorPiercing, 100.0);
    shot.ballistics.device_damage = [50.0; 2];
    let entry = surface(30.0, 1, plate(50.0, 1.0), 0.0);
    let mut engine = surface(30.25, 2, plate(0.0, 0.0), 0.0);
    engine.plate.device = Some(ArmorDevice {
        device: Device::Engine,
        can_ignite_engine: true,
        health: None,
        projectile_chance: 0.0,
        explosion_chance: 0.0,
    });
    let mut crew = surface(30.5, 3, plate(0.0, 0.0), 0.0);
    crew.plate.device = engine.plate.device.map(|device| ArmorDevice {
        device: Device::Crew(7),
        ..device
    });
    let mut beyond = crew;
    beyond.distance = 31.0;
    beyond.plate.device = crew.plate.device.map(|device| ArmorDevice {
        device: Device::Crew(8),
        ..device
    });
    let surfaces = [entry, engine, crew, beyond];
    let mut parameters = BattleParameters::default();
    let hit = shot.resolve_surfaces(&surfaces, &parameters).unwrap();
    assert!(hit.devices.iter().all(Option::is_none));

    parameters.combat_rng = false;
    assert_eq!(roll(0.25, true, &parameters), 1.0);
    assert_eq!(roll(0.25, false, &parameters), 1.0);
    let hit = shot.resolve_surfaces(&surfaces, &parameters).unwrap();
    assert_eq!(hit.damage, 100);
    assert_eq!(
        hit.devices
            .iter()
            .flatten()
            .map(|hit| (hit.device, hit.damage))
            .collect::<Vec<_>>(),
        [(Device::Engine, 50), (Device::Crew(7), 50)]
    );
}

#[test]
fn penetrating_shell_traces_devices_before_the_next_hull_surface() {
    let mut shot = shot(ShellKind::ArmorPiercing, 100.0);
    shot.ballistics.device_damage = [50.0; 2];
    let entry = surface(30.0, 1, plate(50.0, 1.0), 0.0);
    let mut engine = surface(30.25, 2, plate(0.0, 0.0), 0.0);
    engine.plate.device = Some(ArmorDevice {
        device: Device::Engine,
        can_ignite_engine: true,
        health: None,
        projectile_chance: 1.0,
        explosion_chance: 0.0,
    });
    let exit = surface(30.5, 3, plate(50.0, 1.0), 0.0);
    let hit = shot
        .resolve_surfaces(&[entry, engine, exit], &BattleParameters::default())
        .unwrap();
    assert_eq!(hit.outcome, ArmorOutcome::Penetrated);
    assert_eq!(hit.devices[0].unwrap().device, Device::Engine);
    assert_eq!(hit.devices[0].unwrap().damage, 50);
    let beyond = surface(29.8, 3, plate(50.0, 1.0), 0.0);
    let blocked = shot
        .resolve_surfaces(&[beyond, entry, engine], &BattleParameters::default())
        .unwrap();
    assert!(blocked.devices.iter().all(Option::is_none));
}

#[test]
fn layered_and_secondary_impacts_keep_distance_loss_and_remaining_penetration() {
    let mut shot = shot(ShellKind::ArmorPiercing, 100.0);
    shot.ballistics.penetration_mm = [100.0, 55.0];
    let screen = surface(150.0, 1, plate(25.0, 0.0), 0.0);
    let hit = shot
        .resolve_surfaces(&[screen], &BattleParameters::default())
        .unwrap();
    assert_eq!(hit.outcome, ArmorOutcome::ScreenPenetrated);
    assert_eq!(hit.remaining_mm, 65.0);
    let next = shot
        .through_screen(hit, &BattleParameters::default())
        .unwrap();
    assert!((next.penetration_at(100.0) - 55.0).abs() < 0.002);
    assert!(next.max_distance < shot.max_distance);
    let armor = surface(100.0, 2, plate(60.0, 1.0), 0.0);
    assert_eq!(
        next.resolve_surfaces(&[armor], &BattleParameters::default())
            .unwrap()
            .outcome,
        ArmorOutcome::Blocked
    );
}

#[test]
fn shell_curves_can_limit_reach_before_the_nominal_range() {
    use wot_shared_kernel::shell_zero_distance;
    let reach = shell_zero_distance([50.0, 5.0]);
    assert_eq!(reach, 550.0);
    assert_eq!(shell_value_at_distance([50.0, 5.0], reach), 0.0);
    assert!(shell_zero_distance([50.0; 2]).is_infinite());
}

#[test]
fn across_hull_tracks_require_penetration_of_both_hull_faces() {
    let mut shot = shot(ShellKind::ArmorPiercing, 90.0);
    shot.ballistics.device_damage = [50.0; 2];
    let track = |distance, material, device| {
        let mut contact = surface(distance, material, plate(10.0, 0.0), 0.0);
        contact.plate.once = true;
        contact.plate.device = Some(ArmorDevice {
            device,
            can_ignite_engine: false,
            health: None,
            projectile_chance: 1.0,
            explosion_chance: 0.0,
        });
        contact
    };
    let left = track(29.5, 1, Device::TrackLeft);
    let entry = surface(30.0, 3, plate(20.0, 1.0), 0.0);
    let mut exit = surface(32.0, 3, plate(20.0, 1.0), 0.0);
    exit.normal = -exit.normal;
    let right = track(32.5, 2, Device::TrackRight);
    let parameters = BattleParameters::default();
    let devices = |hit: ArmorHit| {
        hit.devices
            .into_iter()
            .flatten()
            .map(|strike| strike.device)
            .collect::<Vec<_>>()
    };
    for (budget, expected) in [
        (90.0, vec![Device::TrackLeft, Device::TrackRight]),
        (55.0, vec![Device::TrackLeft]),
        (40.0, vec![Device::TrackLeft]),
    ] {
        shot.ballistics.penetration_mm = [budget; 2];
        let hit = shot
            .resolve_surfaces(&[left, entry, exit, right], &parameters)
            .unwrap();
        assert_eq!(hit.outcome, ArmorOutcome::Penetrated);
        assert_eq!(hit.damage, 100);
        assert_eq!(devices(hit), expected);
    }
    shot.ballistics.penetration_mm = [90.0; 2];
    assert_eq!(
        devices(shot.resolve_surfaces(&[left, right], &parameters).unwrap()),
        [Device::TrackLeft, Device::TrackRight],
    );
    assert_eq!(
        devices(
            shot.resolve_surfaces(&[left, entry, right], &parameters)
                .unwrap()
        ),
        [Device::TrackLeft],
    );
}

#[test]
fn amx_30_b_shell_can_break_a_fresh_hwk_40_track_at_the_ends() {
    use crate::domain::devices::Condition;
    use std::{collections::BTreeMap, time::Instant};
    use wot_shared_kernel::DeviceHealth;

    let parameters = BattleParameters::default();
    let mut shot = shot(ShellKind::ArmorPiercing, 260.0);
    shot.damage = 390;
    shot.ballistics.far_damage = 390.0;
    shot.ballistics.device_damage = [150.0; 2];
    let mut contact = surface(30.0, 1, plate(20.0, 0.0), 0.0);
    contact.plate.device = Some(ArmorDevice {
        device: Device::TrackLeft,
        can_ignite_engine: false,
        health: Some(DeviceHealth {
            maximum: 240,
            repaired: 180,
            repair_seconds: 6.0,
            fire_chance: 0.0,
        }),
        projectile_chance: 1.0,
        explosion_chance: 1.0,
    });
    for (factor, damage, broken) in [(1.0, 390, true), (1.0 / 3.0, 130, false)] {
        contact.track_damage_factor = factor;
        let hit = shot.resolve_surfaces(&[contact], &parameters).unwrap();
        assert_eq!(hit.damage, 0);
        assert_eq!(hit.devices[0].unwrap().damage, damage);
        let mut condition = Condition::default();
        condition.apply(
            &hit.devices,
            &BTreeMap::new(),
            false,
            Instant::now(),
            &parameters,
        );
        assert_eq!(condition.immobilized(), broken);
    }
}
