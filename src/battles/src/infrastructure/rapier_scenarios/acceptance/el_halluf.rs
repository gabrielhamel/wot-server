use super::*;

#[test]
#[ignore = "Requires WOT_RESOURCE_DIR with El Halluf terrain and tree placements"]
fn recorded_tree_break_keeps_the_hull_on_its_downhill_course() {
    let tree = Vec3::new(217.7306, 54.5906, 237.3748);
    let start = Vec2::new(225.056, 248.522);
    let yaw = -2.563;
    let driver = ParticipantId(1);
    let run_approach = |intact: bool| {
        let (id, mut simulation) = prepare_on("29_el_hallouf", 2, 1);
        let battle = simulation.active.get_mut(&id).unwrap();
        if !intact {
            let removed = battle.obstacles.blast_trees(tree, 0.5, 100.0);
            assert_eq!(removed.len(), 1);
            assert_eq!(removed[0].native, Some(0x8181_0024));
        }
        let player = battle.players.get_mut(&driver).unwrap();
        let height = |p| battle.ground.height_at(p).unwrap();
        let (position, direction) = terrain::pose_from_samples(
            start,
            yaw,
            player.tank.collision_hull().half_size,
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
                if update.broken.iter().any(|part| {
                    part.native == Some(0x8181_0024) && part.part == obstacles::TREE_PART
                }) {
                    assert!(impact.replace(tick).is_none());
                }
            }
            let hull = &battle.players[&driver].vehicle;
            samples.push((hull.direction, hull.position.y));
        }
        (samples, impact)
    };
    let (clear, _) = run_approach(false);
    let (intact, impact) = run_approach(true);
    let impact = impact.expect("drove through the recorded tree");
    let mut max_pitch = 0.0_f32;
    let mut max_yaw = 0.0_f32;
    let mut max_rise = 0.0_f32;
    for ((direction, height), (clear_direction, clear_height)) in
        intact[impact..=impact + 18].iter().zip(&clear[impact..])
    {
        max_pitch = max_pitch.max((direction.y - clear_direction.y).abs().to_degrees());
        max_yaw = max_yaw.max((direction.z - clear_direction.z).abs().to_degrees());
        max_rise = max_rise.max(height - clear_height);
    }
    eprintln!(
        "El Halluf tree at step {impact}: {max_pitch:.3}° pitch, {max_yaw:.3}° yaw, {max_rise:.3} m rise above clear route"
    );
    assert!(max_pitch < 1.0 && max_yaw < 1.0 && max_rise < 0.10);
}
