use super::*;

type Wall = (f32, Option<(u32, u32)>);

fn approach(walls: &[Wall], speed: f32, on_deck: bool) -> (Fixture, Scenery, StepState) {
    let floor = if on_deck { 3.0 } else { 0.0 };
    let mut scene = Fixture::new(Vec3::Y * floor);
    let front = scene.chassis.hull_center.z
        + scene.chassis.contact_hull.inner_shape.half_extents.z
        + scene.chassis.contact_hull.border_radius;
    let mut faces = Vec::new();
    for &(gap, fragment) in walls {
        let z = front + gap;
        faces.push((
            [
                Vec3::new(-4.0, floor - 1.0, z),
                Vec3::new(4.0, floor - 1.0, z),
                Vec3::new(0.0, floor + 5.0, z),
            ],
            fragment,
        ));
    }
    if on_deck {
        faces.push((
            [
                Vec3::new(-20.0, floor, -20.0),
                Vec3::new(0.0, floor, 20.0),
                Vec3::new(20.0, floor, -20.0),
            ],
            None,
        ));
    }
    let mut obstacles = Scenery::test_numbered_mesh(Some(42), faces);
    scene.physics.scenery = obstacles.physics_colliders().unwrap();
    scene.physics.scenery.update_near(
        &mut scene.physics.world,
        &obstacles,
        [(Vec3::splat(-20.0), Vec3::splat(20.0))],
    );
    scene.physics.world.detect_collisions(&(), &());
    scene.physics.world.bodies[scene.chassis.body].set_linvel(Vector::Z * speed, true);
    scene.physics.world.bodies[scene.chassis.body].set_angvel(Vector::Y * 0.1, true);
    let mut state = StepState::new(ParticipantId(1), &scene.tank, 6);
    state.break_approaching_scenery(
        &mut scene.physics.world,
        &mut scene.physics.scenery,
        &scene.chassis,
        &mut obstacles,
    );
    (scene, obstacles, state)
}

#[test]
fn breaking_the_nearest_fragment_shortens_the_reach_to_the_next() {
    for (gap, expected_parts) in [(0.10, vec![1, 2]), (0.158, vec![1])] {
        let (scene, _, state) =
            approach(&[(0.01, Some((1, 200))), (gap, Some((2, 1)))], 10.0, false);
        assert_eq!(
            state
                .damage
                .broken
                .iter()
                .map(|e| e.part)
                .collect::<Vec<_>>(),
            expected_parts
        );
        assert!(scene.body().linvel().z < 9.0);
    }
}

#[test]
fn a_solid_wall_blocks_the_fracture_sweep_and_a_load_bearing_deck_allows_it() {
    let (_, _, blocked) = approach(&[(0.02, None), (0.13, Some((1, 1)))], 10.0, true);
    assert!(blocked.damage.broken.is_empty());
    let (mut scene, _, open) = approach(&[(0.13, Some((1, 1)))], 10.0, true);
    assert_eq!(open.damage.broken.len(), 1);
    for _ in 0..30 {
        assert!(scene.step().scenery);
    }
    assert!((scene.body().translation().y - 3.0).abs() < 0.05);
}

#[test]
fn a_nonlethal_track_prediction_leaves_full_damage_available_to_the_solver() {
    let (mut scene, mut obstacles, mut state) = approach(&[(0.01, Some((1, 200)))], 4.0, false);
    let fragment = obstacles
        .intersect(Vec3::Y, Vec3::Z, 10.0)
        .unwrap()
        .fragment
        .unwrap();
    assert!(state.track_contact.is_some());
    assert_eq!(obstacles.remaining_health(fragment), 200);
    assert_eq!(state.damage.remaining_contact_budget(), 4);
    state.contacts(
        &mut scene.physics.world,
        &mut scene.physics.scenery,
        &scene.chassis,
        &mut obstacles,
        0.0,
        true,
    );
    assert!(state.damage.broken.is_empty());
    assert!(obstacles.remaining_health(fragment) < 200);
    assert_eq!(state.damage.remaining_contact_budget(), 3);
}
