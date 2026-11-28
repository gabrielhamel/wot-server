use super::*;

#[test]
#[ignore = "Requires WOT_RESOURCE_DIR with current El Halluf terrain and tree placements"]
fn verified_tree_break_keeps_the_hull_on_its_downhill_course() {
    const TREE_ID: u32 = 0x8081_0014;
    let tree = Vec3::new(179.8929, 53.7170, 271.7996);
    let start = tree.xz() + Vec2::Y * 13.0;
    let run_approach = |intact: bool| {
        let (id, mut simulation) = prepare_on("29_el_hallouf", 2);
        let battle = simulation.active.get_mut(&id).unwrap();
        if !intact {
            let removed = battle.obstacles.blast_trees(tree, 0.5, 100.0);
            assert_eq!(removed.len(), 1);
            assert_eq!(removed[0].native, Some(TREE_ID));
        }
        drive_approach(battle, start, std::f32::consts::PI, TREE_ID)
    };
    let (clear, _) = run_approach(false);
    let (intact, impact) = run_approach(true);
    let impact = impact.expect("drove through the verified tree");
    let mut max_pitch = 0.0_f32;
    let mut max_yaw = 0.0_f32;
    let mut max_rise = 0.0_f32;
    for ((position, direction), (clear_position, clear_direction)) in
        intact[impact..=impact + 18].iter().zip(&clear[impact..])
    {
        max_pitch = max_pitch.max((direction.y - clear_direction.y).abs().to_degrees());
        max_yaw = max_yaw.max((direction.z - clear_direction.z).abs().to_degrees());
        max_rise = max_rise.max(position.y - clear_position.y);
    }
    eprintln!(
        "El Halluf tree at step {impact}: {max_pitch:.3}° pitch, {max_yaw:.3}° yaw, {max_rise:.3} m rise above clear route"
    );
    assert!(max_pitch < 1.0 && max_yaw < 1.0 && max_rise < 0.10);
}

#[test]
#[ignore = "Requires WOT_RESOURCE_DIR with El Halluf terrain and foliage placements"]
fn driving_through_the_recorded_bush_emits_no_tree_fall() {
    let bush = Vec2::new(217.7306, 237.3748);
    let start = Vec2::new(225.056, 248.522);
    let (id, mut simulation) = prepare_on("29_el_hallouf", 2);
    let battle = simulation.active.get_mut(&id).unwrap();
    let (samples, impact) = drive_approach(battle, start, -2.563, 0x8181_0024);

    assert!(
        samples
            .iter()
            .any(|(position, _)| position.xz().distance(bush) < 1.0)
    );
    let last_position = samples.last().unwrap().0.xz();
    assert!((last_position - bush).dot(bush - start) > 0.0);
    assert_eq!(impact, None);
}

fn drive_approach(
    battle: &mut Battle,
    start: Vec2,
    yaw: f32,
    object: u32,
) -> (Vec<(Vec3, Vec3)>, Option<usize>) {
    let driver = ParticipantId(1);
    let player = battle.players.get_mut(&driver).unwrap();
    let height = |p| battle.ground.height_at(p).unwrap();
    let (position, direction) = terrain::pose_from_samples(
        start,
        yaw,
        player.tank.collision_extents().0.xz(),
        height(start),
        height,
    );
    place(player, position, yaw);
    player.tank.set_hull_tilt(direction);
    player.vehicle.direction = direction;
    player.tank.drive(1);

    let mut samples = Vec::new();
    let mut impact = None;
    for tick in 0..240 {
        for update in battle.advance_world(1.0 / 60.0) {
            if update
                .broken
                .iter()
                .any(|part| part.native == Some(object) && part.part == obstacles::TREE_PART)
            {
                assert!(impact.replace(tick).is_none());
            }
        }
        let hull = &battle.players[&driver].vehicle;
        samples.push((hull.position, hull.direction));
    }
    (samples, impact)
}
