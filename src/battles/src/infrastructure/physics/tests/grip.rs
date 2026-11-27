use super::*;
use crate::infrastructure::projectile_tests::profile;

fn bank(angle: f32, yaw: f32) -> Fixture {
    let start = Vec3::Y * 10.0;
    let mut scene = Fixture::with_profile(start, Some(profile()));
    let bank = Rotation::from_rotation_z(angle);
    let top = physics_vec(start);
    scene.terrain = Terrain::flat_with_material(0);
    scene.physics.world.colliders.insert(
        ColliderBuilder::cuboid(25.0, 0.25, 30.0)
            .position(Pose::from_parts(top - bank * Vector::Y * 0.25, bank)),
    );
    scene.physics.world.bodies[scene.chassis.body].set_position(
        Pose::from_parts(top, bank * Rotation::from_rotation_y(yaw)),
        true,
    );
    scene.physics.world.detect_collisions(&(), &());
    scene
}

#[test]
fn modest_grades_hold_a_parked_vehicle_across_along_and_diagonally_to_the_slope() {
    for (yaw, flags) in [0.0, 0.75, std::f32::consts::FRAC_PI_2]
        .into_iter()
        .flat_map(|yaw| [0, 64].map(|flags| (yaw, flags)))
    {
        let mut scene = bank(0.20, yaw);
        scene.tank.drive(flags);
        let initial = scene.body().translation();
        for _ in 0..240 {
            scene.step();
        }
        let shift = game_vec(scene.body().translation() - initial).xz().length();
        let speed = scene.body().linvel().length();
        eprintln!(
            "moderate bank yaw {yaw} flags {flags}: drift {shift:.3} m in 4 s, speed {speed:.3} m/s"
        );
        assert!(shift < 0.12 && speed < 0.05);
    }
}

#[test]
fn steep_banks_overcome_the_authored_surface_grip() {
    let incline = 0.65;
    for (yaw, flags) in [0.0, 0.75, std::f32::consts::FRAC_PI_2]
        .into_iter()
        .flat_map(|yaw| [0, 64].map(|flags| (yaw, flags)))
    {
        let mut scene = bank(incline, yaw);
        scene.tank.drive(flags);
        let initial = scene.body().translation();
        for _ in 0..100 {
            scene.step();
        }
        let delta = scene.body().translation() - initial;
        let normal = Rotation::from_rotation_z(incline) * Vector::Y;
        eprintln!(
            "steep bank yaw {yaw} flags {flags}: downhill {:.3} m, speed {:.3} m/s",
            -delta.x,
            scene.body().linvel().length()
        );
        assert!(delta.x < -0.5 && scene.body().linvel().x < -0.6);
        assert!(normal.dot(delta).abs() < 0.20);
    }
}

#[test]
fn moving_hard_ground_tracks_arrest_a_lateral_impulse_and_keep_forward_momentum() {
    let mut scene = Fixture::with_profile(Vec3::ZERO, Some(profile()));
    scene.terrain = Terrain::flat_with_material(0);
    scene.physics.world.bodies[scene.chassis.body].set_linvel(Vector::new(2.0, 0.0, 5.0), true);
    scene.tank.drive(1);
    let mut stopped_at = None;
    for step in 1..=120 {
        scene.step();
        if stopped_at.is_none() && scene.body().linvel().x.abs() < 0.10 {
            stopped_at = Some(step as f32 * STEP_SECONDS);
        }
    }
    let displacement = scene.body().translation();
    eprintln!(
        "2 m/s lateral slip on hard ground: stopped at {stopped_at:?} s, drift {:.3} m, forward {:.3} m",
        displacement.x, displacement.z
    );
    assert!(displacement.x > 0.30 && displacement.x < 0.42 && displacement.z > 9.0);
    assert!(stopped_at.is_some_and(|elapsed| elapsed < 0.55));
}

#[test]
fn a_moving_turn_follows_the_hull_heading_without_suppressing_the_commanded_yaw() {
    let mut profile = profile();
    profile.forward_speed = 5.0;
    let mut scene = Fixture::with_profile(Vec3::ZERO, Some(profile));
    scene.terrain = Terrain::flat_with_material(0);
    scene.physics.world.bodies[scene.chassis.body].set_linvel(Vector::Z * 5.0, true);
    scene.tank.drive(1 | 8);
    for _ in 0..60 {
        scene.step();
    }
    let heading = game_direction(scene.body()).z;
    let side = scene
        .body()
        .linvel()
        .dot(*scene.body().rotation() * Vector::X)
        .abs();
    eprintln!("5 m/s powered turn: heading {heading:.3} rad in 1 s, lateral slip {side:.3} m/s");
    assert!(heading > 0.55 && side < 0.25);
}

#[test]
fn twenty_six_degree_bank_holds_on_hard_ground_and_slips_on_soft_ground() {
    let mut shifts = Vec::new();
    for material in [0, 2] {
        let mut scene = bank(26_f32.to_radians(), 0.0);
        scene.terrain = Terrain::flat_with_material(material);
        for _ in 0..120 {
            scene.step();
        }
        shifts.push(-scene.body().translation().x);
    }
    eprintln!("26 degree bank, two seconds: hard/soft downhill displacement {shifts:?}");
    assert!(shifts[0].abs() < 0.12 && shifts[1] > 0.50);
}

#[test]
fn track_lock_skids_more_than_powered_steering_and_preserves_driver_control() {
    fn slide(flags: u8) -> (f32, f32) {
        let mut profile = profile();
        profile.mobility.track_lock = true;
        profile.forward_speed = 5.0;
        let mut scene = Fixture::with_profile(Vec3::ZERO, Some(profile));
        scene.terrain = Terrain::flat_with_material(0);
        scene.physics.world.bodies[scene.chassis.body].set_linvel(Vector::Z * 5.0, true);
        scene.tank.drive(flags);
        for _ in 0..21 {
            scene.step();
        }
        let body = scene.body();
        (
            game_direction(body).z,
            body.linvel().dot(*body.rotation() * Vector::X),
        )
    }
    let turn = slide(1 | 8);
    let right = slide(64 | 8);
    let left = slide(64 | 4);
    eprintln!(
        "flat steering and track lock (heading, lateral speed) normal {turn:?}, right {right:?}, left {left:?}"
    );
    assert!(turn.0 > 0.10 && right.0 > 0.15 && left.0 < -0.15);
    assert!(right.1 < -turn.1.abs() - 0.25 && left.1 > turn.1.abs() + 0.25);
}

#[test]
fn zero_surface_grip_has_no_hill_hold_or_lateral_force() {
    let mut scene = bank(0.20, 0.0);
    scene.physics.parameters.hard_surface_grip = 0.0;
    scene.tank.drive(64);
    for _ in 0..90 {
        scene.step();
    }
    assert!(scene.body().position().is_finite() && scene.body().translation().x < -1.0);
    let mut flat = Fixture::new(Vec3::ZERO);
    flat.terrain = Terrain::flat_with_material(0);
    flat.physics.parameters.hard_surface_grip = 0.0;
    flat.physics.world.bodies[flat.chassis.body].set_linvel(Vector::X, true);
    for _ in 0..30 {
        flat.step();
    }
    assert!(flat.body().linvel().x > 0.98);
}

#[test]
fn centered_pivot_and_speed_modifiers_hold_the_hull_on_an_incline() {
    let uphill = std::f32::consts::FRAC_PI_2;
    for flags in [4, 16, 32, 1 | 2] {
        let mut scene = bank(10_f32.to_radians(), uphill);
        // Isolate the passive hold from the configurable drive coast deceleration.
        scene.tank.parameters.coast_deceleration_mps2 = 0.0;
        scene.tank.drive(flags);
        let start = scene.body().center_of_mass();
        for _ in 0..90 {
            scene.step();
        }
        let displacement = game_vec(scene.body().center_of_mass() - start)
            .xz()
            .length();
        let yaw = game_direction(scene.body()).z;
        eprintln!(
            "10-degree uphill control {flags} COM displacement {displacement:.3} m, heading {yaw:.3} rad"
        );
        assert!(displacement < 0.06);
        if flags == 4 {
            assert!(yaw - uphill < -0.5 && yaw - uphill > -2.0);
        }
    }
}

#[test]
fn a_broken_track_holds_a_modest_grade_even_with_throttle_held() {
    use crate::domain::{armor::ImpactSegment, devices::DeviceStrike};
    use wot_shared_kernel::{Device, DeviceHealth};
    let mut scene = bank(0.2, std::f32::consts::FRAC_PI_2);
    scene.tank.apply_criticals(
        &[Some(DeviceStrike {
            device: Device::TrackLeft,
            health: Some(DeviceHealth {
                maximum: 10,
                repaired: 5,
                repair_seconds: 30.0,
                fire_chance: 0.0,
            }),
            damage: u16::MAX,
            can_ignite_engine: false,
            impact: ImpactSegment {
                part: 0,
                start: [0; 3],
                end: [255; 3],
            },
        })],
        std::time::Instant::now(),
    );
    assert!(scene.tank.condition.immobilized());
    scene.tank.drive(1);
    let start = scene.body().center_of_mass();
    for _ in 0..180 {
        scene.step();
    }
    assert!(
        game_vec(scene.body().center_of_mass() - start)
            .xz()
            .length()
            < 0.03
    );
    assert!(scene.body().linvel().length() < 0.03);
}

#[test]
fn powered_traverse_holds_a_modest_sidehill_and_loses_height_beyond_grip() {
    for (angle, steep) in [(0.20, false), (0.65, true)] {
        let mut scene = bank(angle, 0.0);
        scene.tank.drive(1);
        let start = scene.body().center_of_mass();
        for _ in 0..100 {
            scene.step();
        }
        let path = scene.body().center_of_mass() - start;
        eprintln!(
            "powered sidehill {angle:.2} rad, dx={:.3} m, dy={:.3} m, dz={:.3} m",
            path.x, path.y, path.z
        );
        assert!(path.z > 1.0);
        if steep {
            assert!(path.x < -0.5 && path.y < -0.3);
        } else {
            assert!(path.x.abs() < 0.4 && path.y.abs() < 0.10);
        }
    }
}
