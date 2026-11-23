use super::*;
use std::sync::Arc;
use wot_shared_kernel::{
    ArmorMesh, ArmorTriangle, GunLoading, GunPerformance, MountedArmor, ShellBallistics,
    ShellEffects, VehiclePerformance,
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
            speed: 500.0,
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
    }
}

fn surface(distance: f32, material: u32, plate: ArmorPlate, angle: f32) -> Surface {
    let angle = angle.to_radians();
    Surface {
        distance,
        material,
        plate,
        cosine: angle.cos(),
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
    let hit = shot.resolve_surfaces(&[angled]).unwrap();
    assert_eq!(hit.outcome, ArmorOutcome::Blocked);
    assert!(hit.effective_mm < 100.0 && hit.effective_mm > 85.0);

    shot.ballistics.caliber_mm = 120.0;
    assert_eq!(
        shot.resolve_surfaces(&[angled]).unwrap().outcome,
        ArmorOutcome::Penetrated
    );
    let steep = surface(30.0, 1, plate(30.0, 1.0), 75.0);
    shot.ballistics.caliber_mm = 90.0;
    let bounce = shot.resolve_surfaces(&[steep]).unwrap();
    assert_eq!(bounce.outcome, ArmorOutcome::Ricochet);
    let reflected = bounce.reflected.unwrap();
    assert!(reflected.dot(steep.normal) > 0.0);
    let onward = shot.ricochet_from(bounce).unwrap();
    assert!(onward.ricocheted);
    assert_eq!(onward.penetration_at(0.0), 85.0 * 0.75);
    assert!(onward.max_distance < shot.max_distance);
    assert!(
        onward
            .resolve_surfaces(&[steep])
            .unwrap()
            .reflected
            .is_none()
    );
    shot.ballistics.caliber_mm = 100.0;
    shot.ballistics.penetration_mm = [45.0; 2];
    assert_eq!(
        shot.resolve_surfaces(&[steep]).unwrap().outcome,
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
        shot.resolve_surfaces(&[screen, close]).unwrap().outcome,
        ArmorOutcome::Penetrated
    );
    assert_eq!(
        shot.resolve_surfaces(&[screen, far]).unwrap().outcome,
        ArmorOutcome::Blocked
    );
}

#[test]
fn modern_he_spalls_at_main_armor_and_respects_screen_loss() {
    let shot = shot(
        ShellKind::HighExplosive {
            spall_damage: 50.0,
            shield_penetration: true,
            obstacle_penetration: true,
        },
        100.0,
    );
    let screen = surface(30.0, 1, plate(20.0, 0.0), 0.0);
    let hull = surface(30.3, 2, plate(60.0, 1.0), 0.0);
    let direct = shot.resolve_surfaces(&[hull]).unwrap();
    assert_eq!(
        (direct.outcome, direct.damage),
        (ArmorOutcome::Penetrated, 100)
    );
    let screened = shot.resolve_surfaces(&[screen, hull]).unwrap();
    assert_eq!(screened.outcome, ArmorOutcome::Spall);
    assert!((1..50).contains(&screened.damage));
    assert_eq!(screened.material, 2);
    assert_eq!(screened.remaining_mm, 40.0);

    let thick_screen = surface(30.0, 1, plate(110.0, 0.0), 0.0);
    let stopped = shot.resolve_surfaces(&[thick_screen, hull]).unwrap();
    assert_eq!(
        (stopped.outcome, stopped.damage),
        (ArmorOutcome::ScreenBlocked, 0)
    );
    let no_hull = shot.resolve_surfaces(&[screen]).unwrap();
    assert_eq!(
        (no_hull.outcome, no_hull.damage),
        (ArmorOutcome::ScreenPenetrated, 0)
    );
}

#[test]
fn a_ray_can_strike_an_off_center_part_or_pass_through_empty_space() {
    let mesh = Arc::new(ArmorMesh {
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
    let hit = shot.resolve_armor(42, path).unwrap();
    assert_eq!(hit.distance, 25.0);
    assert_eq!(hit.segment.part, 2);
    assert_eq!(hit.segment.start[0], 191);
    assert_eq!(hit.segment.end[0], 191);
    assert!(hit.segment.start[2] < hit.segment.end[2]);
    assert!(shot.armor_path(&tank, 24.9).is_none());
    shot.origin.x = -0.5;
    assert!(shot.armor_path(&tank, shot.max_distance).is_none());
}
