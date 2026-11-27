use super::*;

#[test]
#[ignore = "Requires WOT_RESOURCE_DIR with current EU entity definitions"]
fn fracturing_tall_and_low_props_preserves_flat_ground_pitch_and_vertical_speed() {
    let (id, mut simulation) = prepare(2, 0);
    let battle = simulation.active.get_mut(&id).unwrap();
    let driver = ParticipantId(1);
    battle.players.get_mut(&ParticipantId(2)).unwrap().observer = true;
    battle.ground = Arc::new(terrain::Terrain::flat_with_material(0));
    let dt = 1.0 / 60.0;
    let mut runs = Vec::new();
    for (height, part) in [(0.0, 0), (4.0, obstacles::TREE_PART), (0.24, 1)] {
        let player = battle.players.get_mut(&driver).unwrap();
        let mut entrant = Entrant::sample();
        entrant.vehicle.performance = Some(profile());
        player.tank = entrant.tank();
        place(player, Vec3::ZERO, 0.0);
        battle.obstacles = if part == 0 {
            obstacles::Scenery::default()
        } else {
            let a = Vec3::new(0.6, -0.5, 14.0);
            let b = Vec3::new(1.8, -0.5, 14.0);
            let c = b.with_y(height);
            let d = a.with_y(height);
            let e = c + Vec3::Z * 0.5;
            let f = d + Vec3::Z * 0.5;
            obstacles::Scenery::test_numbered_mesh(
                Some(0x7f7f_0002),
                [[a, b, c], [a, c, d], [d, c, e], [d, e, f]]
                    .map(|tri| (tri, Some((part, 1))))
                    .to_vec(),
            )
        };
        battle.physics = Some(
            physics::ArenaPhysics::new(
                &battle.ground,
                &battle.obstacles,
                battle.map.bounds,
                battle.parameters,
            )
            .unwrap(),
        );
        battle.players.get_mut(&driver).unwrap().tank.drive(1);
        let mut broken = 0;
        let mut samples = Vec::new();
        let mut prev_y = 0.0;
        for _ in 0..260 {
            broken += battle
                .advance_world(dt)
                .iter()
                .map(|u| u.broken.len())
                .sum::<usize>();
            let vehicle = &battle.players[&driver].vehicle;
            samples.push((vehicle.direction.y, (vehicle.position.y - prev_y) / dt));
            prev_y = vehicle.position.y;
        }
        let z = battle.players[&driver].vehicle.position.z;
        eprintln!("part {part}, fractures={broken}, z={z:.3}");
        runs.push((samples, broken));
    }
    for (samples, broken) in &runs[1..] {
        let (mut pitch, mut vertical) = (0.0f32, 0.0f32);
        for ((a, b), (c, d)) in samples.iter().zip(&runs[0].0) {
            pitch = pitch.max((a - c).abs().to_degrees());
            vertical = vertical.max((b - d).abs());
        }
        eprintln!(
            "fractures={broken}, peak pitch difference={pitch:.4} degrees, peak vertical speed difference={vertical:.4} m/s"
        );
    }
    for (samples, broken) in &runs[1..] {
        assert_eq!(*broken, 1);
        for ((a, b), (c, d)) in samples.iter().zip(&runs[0].0) {
            assert!((a - c).abs().to_degrees() < 1.0 && (b - d).abs() < 0.15);
        }
    }
}
