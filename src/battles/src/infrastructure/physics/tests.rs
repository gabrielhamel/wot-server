use super::*;

struct Fixture {
    physics: ArenaPhysics,
    terrain: Terrain,
    tank: Tank,
    chassis: Chassis,
}

impl Fixture {
    fn new(position: Vec3) -> Self {
        Self::with_profile(position, None)
    }

    fn with_profile(
        position: Vec3,
        profile: Option<wot_shared_kernel::VehiclePerformance>,
    ) -> Self {
        Self::with_parameters(position, profile, BattleParameters::default())
    }

    fn with_parameters(
        position: Vec3,
        profile: Option<wot_shared_kernel::VehiclePerformance>,
        parameters: BattleParameters,
    ) -> Self {
        let terrain = Terrain::flat();
        let mut physics = ArenaPhysics::new(
            &terrain,
            &Scenery::default(),
            [Vec2::splat(-50.0), Vec2::splat(50.0)],
            parameters,
        )
        .unwrap();
        let tank = Tank::new(parameters, position, Vec3::ZERO, 100, Vec::new(), profile);
        let chassis = Chassis::insert(
            &mut physics.world,
            ParticipantId(1),
            &tank,
            Vec3::ZERO,
            RigidBodyType::Dynamic,
        );
        Self {
            physics,
            terrain,
            tank,
            chassis,
        }
    }

    fn step(&mut self) -> tracks::Support {
        let support = tracks::drive(
            &mut self.physics.world,
            &self.physics.scenery,
            &mut self.chassis,
            &self.terrain,
            &mut self.tank,
            &self.physics.parameters,
        );
        self.physics.world.step();
        let body = self.body();
        let position = game_vec(body.translation());
        let direction = game_direction(body);
        let forward = body.linvel().dot(*body.rotation() * Vector::Z);
        let angular = body.angvel().dot(*body.rotation() * Vector::Y);
        self.tank
            .sync_physics(position, direction, forward, angular, support.grounded);
        support
    }

    fn body(&self) -> &RigidBody {
        &self.physics.world.bodies[self.chassis.body]
    }
}

#[test]
fn unsupported_hull_falls_under_gravity_and_suspension_settles_at_ground_level() {
    let mut fixture = Fixture::new(Vec3::Y * 3.0);
    for _ in 0..12 {
        assert!(!fixture.step().grounded);
    }
    let body = fixture.body();
    assert!((body.linvel().y + 1.962).abs() < 0.02);
    assert!((body.translation().y - 2.8).abs() < 0.04);
    for _ in 0..220 {
        fixture.step();
    }
    assert!(fixture.step().grounded);
    assert!(fixture.body().translation().y.abs() < 0.05);
    assert!(fixture.body().linvel().length() < 0.05);
    assert!(game_direction(fixture.body()).length() < 0.01);
    let parked = *fixture.body().position();
    for _ in 0..20 {
        for _ in 0..6 {
            fixture.step();
        }
        assert_eq!(*fixture.body().position(), parked);
    }
}

#[test]
fn signed_gravity_above_the_old_limit_moves_hulls_in_both_directions() {
    for gravity in [-100.0, 100.0] {
        let parameters = BattleParameters::from_overrides(&BTreeMap::from([
            ("gravity_mps2".to_owned(), gravity),
            ("suspension_frequency_hz".to_owned(), 8.0),
        ]))
        .unwrap();
        let mut fixture = Fixture::with_parameters(Vec3::Y * 20.0, None, parameters);
        for _ in 0..12 {
            assert!(!fixture.step().grounded);
        }
        assert!(fixture.physics.world.quarantine().is_empty());
        let expected_speed = -gravity as f32 * 12.0 * STEP_SECONDS;
        assert!((fixture.body().linvel().y - expected_speed).abs() < 0.05);
    }
}

#[test]
fn extreme_suspension_runs_without_non_finite_physics_objects() {
    let parameters = BattleParameters::from_overrides(&BTreeMap::from([
        ("suspension_frequency_hz".to_owned(), f64::from(f32::MAX)),
        ("suspension_damping_ratio".to_owned(), f64::from(f32::MAX)),
        (
            "lateral_grip_response_seconds".to_owned(),
            f64::from(f32::from_bits(1)),
        ),
    ]))
    .unwrap();
    let mut fixture = Fixture::with_parameters(Vec3::Y * 0.1, None, parameters);
    assert!((0..12).any(|_| fixture.step().grounded));
    for _ in 0..12 {
        fixture.step();
        assert!(fixture.physics.world.quarantine().is_empty());
        assert!(fixture.body().translation().is_finite());
        assert!(fixture.body().linvel().is_finite());
    }
}

#[test]
fn uneven_support_rolls_the_hull_on_one_track_and_recovers_on_flat_ground() {
    let mut fixture = Fixture::new(Vec3::Y * 0.5);
    let platform =
        fixture.physics.world.colliders.insert(
            ColliderBuilder::cuboid(1.0, 0.18, 5.0).translation(Vector::new(1.5, 0.0, 0.0)),
        );
    for _ in 0..180 {
        fixture.step();
    }
    assert!(fixture.step().grounded);
    let tilt = game_direction(fixture.body());
    assert!(tilt.x > 0.03 && tilt.x < 0.18, "tilt {tilt:?}");
    fixture.physics.world.remove_collider(platform);
    for _ in 0..180 {
        fixture.step();
    }
    assert!(game_direction(fixture.body()).x.abs() < 0.02);
}

#[test]
fn physical_hull_contact_transfers_momentum_and_arena_bounds_stop_both_tanks() {
    let mut fixture = Fixture::new(Vec3::ZERO);
    let parked = Tank::new(
        BattleParameters::default(),
        Vec3::Z * 5.1,
        Vec3::ZERO,
        100,
        Vec::new(),
        None,
    );
    let mut second = Chassis::insert(
        &mut fixture.physics.world,
        ParticipantId(2),
        &parked,
        Vec3::ZERO,
        RigidBodyType::Dynamic,
    );
    fixture.physics.world.bodies[fixture.chassis.body].set_linvel(Vector::Z * 12.0, true);
    let mut second_tank = parked;
    let mut shared_contact = false;
    for _ in 0..90 {
        tracks::drive(
            &mut fixture.physics.world,
            &fixture.physics.scenery,
            &mut second,
            &fixture.terrain,
            &mut second_tank,
            &fixture.physics.parameters,
        );
        fixture.step();
        shared_contact |= fixture
            .physics
            .world
            .narrow_phase
            .contact_pairs_with(fixture.chassis.collider)
            .any(|pair| {
                (pair.collider1 == second.collider || pair.collider2 == second.collider)
                    && pair.has_any_active_contact()
            });
    }
    assert!(shared_contact);
    assert!(fixture.physics.world.bodies[second.body].translation().z > 6.0);
    fixture.physics.world.bodies[second.body].set_linvel(Vector::Z * 60.0, true);
    for _ in 0..100 {
        fixture.step();
    }
    assert!(fixture.physics.world.bodies[second.body].translation().z < 50.0);
}

#[test]
fn driving_off_a_deck_carries_momentum_and_ignores_steering_until_the_sloped_landing() {
    fn bridge() -> Fixture {
        let mut scene = Fixture::with_profile(
            Vec3::new(0.0, 3.5, -18.0),
            Some(crate::infrastructure::projectile_tests::profile()),
        );
        scene.physics.world.colliders.insert(
            ColliderBuilder::cuboid(3.0, 0.15, 15.0).translation(Vector::new(0.0, 3.35, -15.0)),
        );
        scene.physics.world.colliders.insert(
            ColliderBuilder::cuboid(15.0, 0.3, 23.0)
                .rotation(Vector::Z * 0.09)
                .translation(Vector::new(0.0, 1.0, 19.0)),
        );
        scene.physics.world.detect_collisions(&(), &());
        scene.tank.drive(1);
        scene
    }
    let mut accelerating = bridge();
    let mut steering = bridge();
    let mut takeoff = None;
    let mut airborne_steps = 0;
    let mut landed = false;
    assert!(accelerating.step().grounded && steering.step().grounded);
    for _ in 0..450 {
        let supported = accelerating.step().grounded;
        assert_eq!(steering.step().grounded, supported);
        if supported {
            if takeoff.is_some() && accelerating.tank.position.y < 2.0 {
                landed = true;
                break;
            }
            continue;
        }
        if accelerating.tank.position.z < 2.0 {
            continue;
        }
        takeoff.get_or_insert(accelerating.tank.position);
        airborne_steps += 1;
        steering.tank.drive(2 | 4);
        assert!(accelerating.tank.position.distance(steering.tank.position) < 0.001);
        assert!(
            accelerating
                .body()
                .angvel()
                .distance(steering.body().angvel())
                < 0.001
        );
    }
    let at = takeoff.expect("tracks left the deck");
    assert!(
        landed && airborne_steps > 6,
        "airborne steps={airborne_steps}, pose={:?}",
        accelerating.tank.position
    );
    assert!(accelerating.tank.position.z > at.z + 1.0);
    assert!(accelerating.tank.position.y < at.y - 0.35);
    accelerating.tank.drive(64);
    for _ in 0..160 {
        accelerating.step();
    }
    assert!(
        accelerating.step().grounded,
        "end state {:?} tilt {:?} velocity {:?}",
        accelerating.tank.position,
        game_direction(accelerating.body()),
        accelerating.body().linvel()
    );
    assert!(game_direction(accelerating.body()).x.abs() > 0.03);
}

#[test]
fn righting_preserves_heading_on_a_supported_grade_and_requires_overhead_clearance() {
    let mut scene = Fixture::new(Vec3::Y * 4.0);
    let slope = 0.22;
    scene.physics.world.colliders.insert(
        ColliderBuilder::cuboid(5.0, 0.25, 5.0)
            .rotation(Vector::X * slope)
            .translation(Vector::Y),
    );
    scene.physics.world.bodies[scene.chassis.body]
        .set_rotation(Rotation::from_rotation_z(std::f32::consts::PI), true);
    scene.physics.world.detect_collisions(&(), &());
    let recovered = upright_pose(&scene.physics.world, &scene.chassis, 1.0)
        .expect("a clear mild grade can support recovery");
    let up = recovered.rotation * Vector::Y;
    assert!(up.distance(Rotation::from_rotation_x(slope) * Vector::Y) < 0.001);
    assert!((recovered.rotation * Vector::Z).x.abs() < 0.001);
    scene
        .physics
        .world
        .colliders
        .insert(ColliderBuilder::cuboid(1.0, 0.1, 1.0).translation(Vector::new(1.5, 2.6, 0.0)));
    scene.physics.world.detect_collisions(&(), &());
    assert!(upright_pose(&scene.physics.world, &scene.chassis, 1.0).is_none());
}

#[test]
fn touching_the_floor_without_any_suspension_load_cannot_power_the_tracks() {
    let mut scene = Fixture::with_profile(
        Vec3::ZERO,
        Some(crate::infrastructure::projectile_tests::profile()),
    );
    scene.physics.world.gravity = Vector::ZERO;
    scene.physics.parameters.gravity_mps2 = 0.0;
    scene.tank.parameters.gravity_mps2 = 0.0;
    scene.tank.drive(1 | 8);
    for _ in 0..60 {
        scene.step();
    }
    assert!(scene.body().linvel().length() < 0.001 && scene.body().angvel().length() < 0.001);
}

#[test]
fn overturned_recovery_timer_starts_only_after_resting_on_ground() {
    let mut scene = Fixture::new(Vec3::Y * 4.0);
    scene.tank.parameters.settle_time_seconds = 0.2;
    scene.tank.parameters.recovery_time_seconds = 0.3;
    scene.physics.world.bodies[scene.chassis.body]
        .set_rotation(Rotation::from_rotation_z(std::f32::consts::PI), true);
    let mut stationary_steps = 0;
    for _ in 0..240 {
        scene.step();
        let stationary =
            scene.body().is_sleeping() || scene.body().activation().time_since_can_sleep > 0.0;
        let stable = stationary
            && has_recovery_support(
                &scene.physics.world,
                &scene.chassis,
                scene.tank.parameters.tip_terrain_roll_radians,
            );
        if stable {
            stationary_steps += 1;
        } else {
            stationary_steps = 0;
        }
        if scene.tank.physical_recovery_due(stable, STEP_SECONDS) {
            assert!(stationary_steps >= 30);
            assert!(
                upright_pose(
                    &scene.physics.world,
                    &scene.chassis,
                    scene.tank.parameters.tip_terrain_roll_radians
                )
                .is_some()
            );
            return;
        }
    }
    panic!("the resting upside-down hull did not recover");
}

mod fractures;
mod grip;
