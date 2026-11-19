use super::{Peer, Pickle, Value, dict};
use wot_server::{pickle, player_sessions::test_support::mercury::app::dispatch::ScriptDispatch};

fn row(value: &Pickle, id: i64) -> Pickle {
    let Pickle::Dict(rows) = value else {
        panic!("Expected inventory table");
    };
    rows.get(&Pickle::from(id)).cloned().unwrap_or(Pickle::None)
}

pub(super) fn accelerated_training(
    peer: &mut Peer,
    dispatch: &ScriptDispatch,
    initial: &Pickle,
) -> Pickle {
    let Pickle::I64(revision) = dict(initial, "rev") else {
        panic!("Expected cache revision");
    };
    let settings = |cache: &Pickle| dict(&row(&dict(cache, "inventory"), 1), "settings");
    let Pickle::I64(previous_flags) = row(&settings(initial), 3) else {
        panic!("Missing ISU-130 settings");
    };
    assert_eq!(previous_flags & 1, 0);
    for (id, vehicle, enabled, result) in [
        (720, 3, 2, -2),
        (721, 99, 1, -1),
        (722, 1, 1, -1),
        (723, 3, 1, 0),
        (724, 3, 1, 0),
        (725, 3, 0, 0),
        (726, 3, 1, 0),
    ] {
        peer.command(
            dispatch,
            "doCmdInt4",
            vec![
                Value::Int16(id),
                Value::Int16(107),
                Value::Int64(vehicle),
                Value::Int64(1),
                Value::Int64(enabled),
                Value::Int64(0),
            ],
        );
        peer.until(dispatch, |p| p.responses.contains(&(id, result)));
    }
    for (offset, flag) in [(1, 1), (2, 0), (3, 1)] {
        let diff = super::ammunition_flow::update(peer, revision + offset);
        assert_eq!(settings(&diff), pickle!({3: (previous_flags | flag)}));
        assert_eq!(dict(&dict(&diff, "stats"), "vehTypeXP"), pickle!({}));
    }
    peer.command(
        dispatch,
        "doCmdInt3",
        vec![
            Value::Int16(727),
            Value::Int16(100),
            Value::Int64(0),
            Value::Int64(0),
            Value::Int64(0),
        ],
    );
    peer.until(dispatch, |p| p.has_resource(727));
    let full = peer.resource(727);
    assert_eq!(dict(&full, "rev"), Pickle::from(revision + 3));
    assert_eq!(row(&settings(&full), 3), Pickle::from(previous_flags | 1));
    full
}

pub(super) fn exercise(peer: &mut Peer, dispatch: &ScriptDispatch, initial: &Pickle) -> Pickle {
    let int = |peer: &mut Peer, id, command, args: &[i64]| {
        let mut values = vec![Value::Int16(id), Value::Int16(command)];
        values.extend(args.iter().copied().map(Value::Int64));
        let method = match args.len() {
            3 => "doCmdInt3",
            4 => "doCmdInt4",
            _ => panic!("Unexpected command arity"),
        };
        peer.command(dispatch, method, values);
    };
    let array = |peer: &mut Peer, id, command, args: &[i32]| {
        peer.command(
            dispatch,
            "doCmdIntArr",
            vec![
                Value::Int16(id),
                Value::Int16(command),
                Value::Seq(args.iter().copied().map(Value::Int32).collect()),
            ],
        );
    };
    let wait = |peer: &mut Peer, id, result| {
        peer.until(dispatch, |p| p.responses.contains(&(id, result)));
    };
    let inventory = dict(initial, "inventory");
    let original = row(&dict(&row(&inventory, 1), "crew"), 1);
    let Pickle::Dict(members) = dict(&row(&inventory, 8), "compDescr") else {
        panic!("Expected crew inventory");
    };
    let Some(Pickle::I64(last)) = members.keys().last() else {
        panic!("Missing starter crew");
    };
    let hired = i32::try_from(last + 1).unwrap();

    array(peer, 680, 105, &[1, -1, -1, 0, i32::MAX]);
    wait(peer, 680, -2);
    int(peer, 681, 303, &[0, 3089, 1, 0]);
    wait(peer, 681, -4);
    int(peer, 682, 303, &[1, 3089, 4, 0]);
    wait(peer, 682, -1);
    array(peer, 683, 105, &[1, -1, -1, 0, 1]);
    wait(peer, 683, 0);
    let update = super::ammunition_flow::update(peer, 10);
    let vehicles = row(&dict(&update, "inventory"), 1);
    assert_eq!(
        row(&dict(&vehicles, "crew"), 1),
        pickle!([None, None, None])
    );
    int(peer, 684, 157, &[1, 0, 0]);
    wait(peer, 684, 0);
    let update = super::ammunition_flow::update(peer, 11);
    assert_eq!(
        row(&dict(&row(&dict(&update, "inventory"), 1), "crew"), 1),
        original
    );

    int(peer, 685, 303, &[1, 3089, 1, 0]);
    wait(peer, 685, 0);
    let update = super::ammunition_flow::update(peer, 12);
    let descriptors = dict(&row(&dict(&update, "inventory"), 8), "compDescr");
    let descriptor = row(&descriptors, i64::from(hired));
    assert!(matches!(descriptor, Pickle::Bytes(_)));
    assert_eq!(
        super::ammunition_flow::receipt(peer, 685),
        pickle!({
            "tmanInvID": (hired), "tmanCompDescr": (descriptor),
        })
    );

    array(peer, 686, 105, &[1, 0, hired, 23, 3]);
    // Exactly enough XP for two full perks, plus the client's selectable 0% slot.
    const FREE_XP_FOR_TWO_PERKS: i32 = 126_036;
    array(peer, 687, 155, &[1, hired, FREE_XP_FOR_TWO_PERKS, 23, 3]);
    array(peer, 712, 156, &[hired, 1, 6, 8, 9, 16]);
    wait(peer, 686, 0);
    wait(peer, 687, 0);
    wait(peer, 712, -1);
    array(peer, 688, 156, &[hired, 1, 6, 8, 9]);
    wait(peer, 688, 0);
    let update = super::ammunition_flow::update(peer, 15);
    let descriptors = dict(&row(&dict(&update, "inventory"), 8), "compDescr");
    let Pickle::Bytes(trained) = row(&descriptors, hired.into()) else {
        panic!("Trained crew descriptor missing");
    };
    assert_eq!(&trained[6..11], &[3, 6, 8, 9, 0]);
    // Two full major perks yield bonus slots at 100% and 0% for the secondary role.
    array(peer, 713, 156, &[hired, 2, 39, 40, 41]);
    wait(peer, 713, -1);
    array(peer, 689, 156, &[hired, 2, 39, 40]);
    peer.until(dispatch, |p| p.responses.iter().any(|&(id, _)| id == 689));
    assert!(
        peer.responses.contains(&(689, 0)),
        "Bonus perk batch failed"
    );
    let update = super::ammunition_flow::update(peer, 16);
    let descriptors = dict(&row(&dict(&update, "inventory"), 8), "compDescr");
    let Pickle::Bytes(trained) = row(&descriptors, hired.into()) else {
        panic!("Trained crew descriptor missing");
    };
    assert_eq!(dict(&update, "prevRev"), Pickle::from(15));
    assert_eq!(&trained[6..14], &[3, 6, 8, 9, 0, 2, 39, 40]);

    const BOOK: i32 = 42 * 256 + 16 + 14;
    int(peer, 690, 302, &[1, BOOK.into(), 1, 0]);
    wait(peer, 690, 0);
    array(peer, 691, 162, &[BOOK, 1, 1, -1, 0, 1]);
    wait(peer, 691, 0);
    let update = super::ammunition_flow::update(peer, 18);
    assert_eq!(
        row(&row(&dict(&update, "inventory"), 14), BOOK.into()),
        Pickle::None
    );
    array(peer, 692, 162, &[BOOK, 1, 1, -1, 0, 1]);
    wait(peer, 692, -1);

    int(peer, 693, 152, &[1, hired.into(), 0]);
    wait(peer, 693, 0);
    int(peer, 694, 306, &[hired.into(), 0, 0]);
    wait(peer, 694, 0);
    let update = super::ammunition_flow::update(peer, 20);
    let recycle = dict(&dict(&dict(&update, "recycleBin"), "tankmen"), "buffer");
    assert!(matches!(row(&recycle, hired.into()), Pickle::Tuple(_)));
    array(peer, 695, 10015, &[hired, 0, 1]);
    wait(peer, 695, 0);
    int(peer, 696, 206, &[1, 1, 0]);
    wait(peer, 696, 0);
    int(peer, 697, 309, &[1, 1, 0, 1]);
    wait(peer, 697, 0);
    array(peer, 699, 154, &[1, hired, 0, 0, 0, 32767, 0, -1, 0, -1]);
    wait(peer, 699, -1);
    array(peer, 700, 154, &[1, hired, 0, 0, 0, 2, 0, -1, 0, -1]);
    wait(peer, 700, 0);
    const SKIN: i64 = 107 * 256 + 16 + 13;
    int(peer, 701, 302, &[1, SKIN, 1, 0]);
    wait(peer, 701, 0);
    int(peer, 702, 160, &[hired.into(), 107, 0]);
    wait(peer, 702, 0);
    int(peer, 703, 161, &[hired.into(), 0, 0]);
    wait(peer, 703, 0);
    int(peer, 704, 160, &[hired.into(), 107, 0]);
    wait(peer, 704, 0);

    array(peer, 705, 158, &[1, hired, 3, 3089, 24, 3]);
    array(peer, 706, 153, &[1, hired, 1, 3089, 24, 3]);
    array(peer, 707, 105, &[1, 1, hired, 24, 3]);
    for id in 705..=707 {
        wait(peer, id, 0);
    }
    int(peer, 708, 107, &[1, 512, 1, 0]);
    wait(peer, 708, 0);

    let undertrained = hired + 2;
    int(peer, 709, 303, &[1, 3089, 1, 0]);
    wait(peer, 709, 0);
    array(peer, 710, 155, &[1, undertrained, 600, 0, 1]);
    wait(peer, 710, 0);
    peer.command(
        dispatch,
        "doCmdInt",
        vec![Value::Int16(711), Value::Int16(165), Value::Int64(0)],
    );
    wait(peer, 711, 0);
    let conversion = super::ammunition_flow::receipt(peer, 711);
    assert_eq!(dict(&conversion, "count"), Pickle::from(2));

    int(peer, 698, 100, &[0, 0, 0]);
    peer.until(dispatch, |p| p.has_resource(698));
    let full = peer.resource(698);
    assert_eq!(dict(&full, "rev"), Pickle::from(35));
    let stats = dict(&full, "stats");
    assert_eq!(dict(&stats, "berths"), Pickle::from(116));
    assert_eq!(
        dict(&stats, "freeXP"),
        Pickle::from(1_000_000 - 35 - 275 - i64::from(FREE_XP_FOR_TWO_PERKS) - 600)
    );
    assert_eq!(dict(&stats, "gold"), Pickle::from(1_000_000 - 300));
    let inventory = dict(&full, "inventory");
    let tankmen = row(&inventory, 8);
    assert_eq!(
        row(&dict(&tankmen, "vehicle"), hired.into()),
        Pickle::from(1)
    );
    assert_eq!(
        row(&dict(&tankmen, "vehicle"), i64::from(hired + 1)),
        Pickle::from(1)
    );
    assert_eq!(
        row(&dict(&tankmen, "compDescr"), undertrained.into()),
        Pickle::None
    );
    let Pickle::Bytes(retrained) = row(&dict(&tankmen, "compDescr"), hired.into()) else {
        panic!("Retrained crew descriptor missing");
    };
    assert_eq!(&retrained[3..6], &[6, 0x9c, 0x40]);
    assert_eq!(&retrained[8..10], &[0, 2]);
    let skins = row(&inventory, 13);
    assert_eq!(row(&row(&skins, 2), hired.into()), Pickle::from(107));
    assert_eq!(row(&row(&skins, 1), 107), Pickle::None);
    assert_eq!(row(&row(&inventory, 14), 542), Pickle::from(1));
    assert_eq!(
        row(&dict(&row(&inventory, 1), "settings"), 1),
        Pickle::from(14 | 512)
    );
    full
}
