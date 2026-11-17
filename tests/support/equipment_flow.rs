use super::{Peer, Pickle, Value, dict};
use wot_server::{client_gateway::mercury::app::dispatch::ScriptDispatch, pickle};

const AIMING: i32 = (101 << 8) | 0xf9;
const RAMMER: i32 = (81 << 8) | 0xf9;
const WRONG_CLASS_RAMMER: i32 = (82 << 8) | 0xf9;
const IMPROVED_CONFIGURATION: i32 = (43 << 8) | 0xf9;
const IMPROVED_OPTICS: i32 = (46 << 8) | 0xf9;
const IMPROVED_RAMMER: i32 = (45 << 8) | 0xf9;
const BOUNTY_AIM_DRIVES: i32 = (50 << 8) | 0xf9;
const EXPERIMENTAL_MOBILITY_2: i32 = (154 << 8) | 0xf9;
const EXPERIMENTAL_MOBILITY_3: i32 = (155 << 8) | 0xf9;

pub fn exercise(peer: &mut Peer, dispatch: &ScriptDispatch) -> Pickle {
    for (id, command, args, result) in [
        (601, 10060, vec![1, 1, i32::MAX], -2),
        (602, 10060, vec![1, 1, 1, -AIMING], -2),
        (603, 10060, vec![0, 1, 1, AIMING], -4),
        (604, 10060, vec![1, 99, 1, AIMING], -1),
        (605, 10060, vec![1, 2, 1, AIMING], -1),
        (606, 10060, vec![1, 1, 2, AIMING, AIMING], -1),
        (607, 10060, vec![1, 1, 2, AIMING, WRONG_CLASS_RAMMER], -1),
        (608, 10060, vec![1, 1, 4, AIMING, 0, 0, 0], -1),
        (609, 102, vec![1, 1, AIMING, 0, 1, 0, 0], -1),
    ] {
        send_layout(peer, dispatch, id, command, &args, result);
    }

    // Buy one rammer in the shop, then consume it while buying and mounting aiming.
    peer.command(
        dispatch,
        "doCmdInt4",
        vec![
            Value::Int16(610),
            Value::Int16(302),
            Value::Int64(1),
            Value::Int64(i64::from(RAMMER)),
            Value::Int64(1),
            Value::Int64(0),
        ],
    );
    peer.until(dispatch, |p| p.responses.contains(&(610, 0)));
    let purchase = super::ammunition_flow::update(peer, 2);
    assert_eq!(dict(&purchase, "inventory"), pickle!({9: {(RAMMER): 1}}));
    send_layout(peer, dispatch, 611, 10060, &[1, 1, 3, AIMING, 0, RAMMER], 0);
    let mounted = super::ammunition_flow::update(peer, 3);
    let inventory = dict(&mounted, "inventory");
    assert_eq!(row(&inventory, 9), pickle!({(RAMMER): (Pickle::None)}));
    assert_eq!(
        dict(&row(&inventory, 1), "devicesLayout"),
        pickle!({(Pickle::Tuple(vec![1.into(), "_r".into()])): [[(AIMING), 0, (RAMMER)]]})
    );
    let descriptor = row(&dict(&row(&inventory, 1), "compDescr"), 1);
    let Pickle::Bytes(descriptor) = descriptor else {
        panic!("missing vehicle descriptor");
    };
    assert_eq!(&descriptor[14..19], &[0b0101, 81, 0, 101, 0]);
    assert_eq!(
        dict(&dict(&mounted, "stats"), "credits"),
        Pickle::from(99_500_000)
    );

    send_layout(peer, dispatch, 612, 10060, &[1, 1, 3, AIMING, 0, RAMMER], 0);
    send_layout(peer, dispatch, 613, 10060, &[1, 1, 1, AIMING], -1);
    // Move equipment within the setup without paying again.
    send_layout(peer, dispatch, 614, 10060, &[1, 1, 2, RAMMER, AIMING], 0);
    let moved = super::ammunition_flow::update(peer, 4);
    assert_eq!(dict(&moved, "stats"), dict(&mounted, "stats"));
    // Paid demount returns the rammer to the depot, and the single-slot command can reuse it.
    send_layout(peer, dispatch, 615, 102, &[1, 1, 0, 0, 1, 1, 0], 0);
    let removed = super::ammunition_flow::update(peer, 5);
    assert_eq!(row(&dict(&removed, "inventory"), 9), pickle!({(RAMMER): 1}));
    assert_eq!(
        dict(&dict(&removed, "stats"), "gold"),
        Pickle::from(999_990)
    );
    send_layout(peer, dispatch, 616, 102, &[1, 1, RAMMER, 2, 1, 0, 0], 0);
    let remounted = super::ammunition_flow::update(peer, 6);
    assert_eq!(dict(&remounted, "stats"), dict(&removed, "stats"));

    let cache = full_cache(peer, dispatch);
    assert_eq!(dict(&cache, "rev"), Pickle::from(6));
    assert_eq!(
        row(
            &dict(&row(&dict(&cache, "inventory"), 1), "devicesLayout"),
            1
        ),
        pickle!([[0, (AIMING), (RAMMER)]])
    );
    let inventory = dict(&cache, "inventory");
    let descriptor = row(&dict(&row(&inventory, 1), "compDescr"), 1);
    let Pickle::Bytes(descriptor) = descriptor else {
        panic!("missing vehicle descriptor");
    };
    assert_eq!(&descriptor[14..19], &[0b0011, 81, 0, 101, 0]);
    cache
}

pub fn full_cache(peer: &mut Peer, dispatch: &ScriptDispatch) -> Pickle {
    sync(peer, dispatch, 617)
}

pub fn reward_stock() -> std::collections::BTreeMap<i32, u32> {
    [
        BOUNTY_AIM_DRIVES,
        EXPERIMENTAL_MOBILITY_2,
        EXPERIMENTAL_MOBILITY_3,
    ]
    .into_iter()
    .map(|item| (item, 1))
    .collect()
}

pub fn exercise_special_equipment(
    peer: &mut Peer,
    dispatch: &ScriptDispatch,
    before: &Pickle,
) -> Pickle {
    use super::{ammunition_flow::update, depot_flow::send_four};

    const ISU_130: i32 = 3;
    let Pickle::I64(revision) = dict(before, "rev") else {
        panic!("missing revision");
    };
    let previous_stats = dict(before, "stats");
    let Pickle::I64(bonds) = dict(&previous_stats, "crystal") else {
        panic!("missing bonds");
    };
    let Pickle::I64(gold) = dict(&previous_stats, "gold") else {
        panic!("missing gold");
    };
    for (id, item) in [(780, BOUNTY_AIM_DRIVES), (781, EXPERIMENTAL_MOBILITY_2)] {
        send_four(peer, dispatch, id, 302, [1, item.into(), 1, 0], -1);
    }
    send_four(
        peer,
        dispatch,
        782,
        302,
        [1, IMPROVED_OPTICS.into(), 1, 0],
        0,
    );
    let purchase = update(peer, revision + 1);
    assert_eq!(
        dict(&purchase, "inventory"),
        pickle!({9: {(IMPROVED_OPTICS): 1}})
    );
    assert_eq!(
        dict(&dict(&purchase, "stats"), "crystal"),
        (bonds - 4_000).into()
    );

    let apply = |peer: &mut Peer, id, items: [i32; 3], result| {
        let [first, second, third] = items;
        send_layout(
            peer,
            dispatch,
            id,
            10060,
            &[1, ISU_130, 3, first, second, third],
            result,
        );
    };
    apply(
        peer,
        783,
        [IMPROVED_CONFIGURATION, IMPROVED_OPTICS, IMPROVED_RAMMER],
        -1,
    );
    let cache = sync(peer, dispatch, 784);
    assert_eq!(dict(&cache, "rev"), (revision + 1).into());
    assert_eq!(
        row(&row(&dict(&cache, "inventory"), 9), IMPROVED_OPTICS),
        1.into()
    );
    assert_eq!(
        dict(&dict(&cache, "stats"), "crystal"),
        (bonds - 4_000).into()
    );

    let mounted = [
        IMPROVED_CONFIGURATION,
        BOUNTY_AIM_DRIVES,
        EXPERIMENTAL_MOBILITY_2,
    ];
    apply(peer, 786, mounted, 0);
    let applied = update(peer, revision + 2);
    let inventory = dict(&applied, "inventory");
    assert_eq!(
        row(&inventory, 9),
        pickle!({(BOUNTY_AIM_DRIVES): None, (EXPERIMENTAL_MOBILITY_2): None})
    );
    let key = Pickle::Tuple(vec![ISU_130.into(), "_r".into()]);
    assert_eq!(
        dict(&row(&inventory, 1), "devicesLayout"),
        pickle!({(key): [[(IMPROVED_CONFIGURATION), (BOUNTY_AIM_DRIVES), (EXPERIMENTAL_MOBILITY_2)]]})
    );
    assert_eq!(
        dict(&dict(&applied, "stats"), "crystal"),
        (bonds - 7_000).into()
    );
    apply(peer, 787, mounted, 0);

    for (id, slot, item, revision_offset, currency, balance) in [
        (788, 0, IMPROVED_CONFIGURATION, 3, "crystal", bonds - 7_200),
        (789, 1, BOUNTY_AIM_DRIVES, 4, "gold", gold - 10),
        (790, 2, EXPERIMENTAL_MOBILITY_2, 5, "equipCoin", 100),
    ] {
        send_layout(peer, dispatch, id, 102, &[1, ISU_130, 0, slot, 1, 1, 0], 0);
        let removed = update(peer, revision + revision_offset);
        assert_eq!(row(&dict(&removed, "inventory"), 9), pickle!({(item): 1}));
        assert_eq!(dict(&dict(&removed, "stats"), currency), balance.into());
    }
    apply(
        peer,
        791,
        [
            IMPROVED_CONFIGURATION,
            BOUNTY_AIM_DRIVES,
            EXPERIMENTAL_MOBILITY_3,
        ],
        0,
    );
    send_layout(peer, dispatch, 792, 102, &[1, ISU_130, 0, 2, 1, 1, 0], 0);
    let removed = update(peer, revision + 7);
    assert_eq!(dict(&dict(&removed, "stats"), "equipCoin"), 0.into());
    send_layout(
        peer,
        dispatch,
        793,
        102,
        &[1, ISU_130, EXPERIMENTAL_MOBILITY_3, 2, 1, 0, 0],
        0,
    );
    send_layout(peer, dispatch, 794, 102, &[1, ISU_130, 0, 2, 1, 1, 0], -1);
    let cache = sync(peer, dispatch, 795);
    assert_eq!(dict(&cache, "rev"), (revision + 8).into());
    let stats = dict(&cache, "stats");
    assert_eq!(dict(&stats, "crystal"), (bonds - 7_200).into());
    assert_eq!(dict(&stats, "gold"), (gold - 10).into());
    assert_eq!(dict(&stats, "equipCoin"), 0.into());
    assert_eq!(
        row(
            &dict(&row(&dict(&cache, "inventory"), 1), "devicesLayout"),
            ISU_130
        ),
        pickle!([[
            (IMPROVED_CONFIGURATION),
            (BOUNTY_AIM_DRIVES),
            (EXPERIMENTAL_MOBILITY_3)
        ]])
    );
    cache
}

fn sync(peer: &mut Peer, dispatch: &ScriptDispatch, id: u16) -> Pickle {
    peer.command(
        dispatch,
        "doCmdInt3",
        vec![
            Value::Int16(id as i16),
            Value::Int16(100),
            Value::Int64(0),
            Value::Int64(0),
            Value::Int64(0),
        ],
    );
    peer.until(dispatch, |p| p.has_resource(id));
    peer.resource(id)
}

fn row(table: &Pickle, id: i32) -> Pickle {
    let Pickle::Dict(rows) = table else {
        panic!("missing table");
    };
    rows[&Pickle::from(id)].clone()
}

fn send_layout(
    peer: &mut Peer,
    dispatch: &ScriptDispatch,
    id: i16,
    command: i16,
    args: &[i32],
    result: i16,
) {
    peer.command(
        dispatch,
        "doCmdIntArr",
        vec![
            Value::Int16(id),
            Value::Int16(command),
            Value::Seq(args.iter().copied().map(Value::Int32).collect()),
        ],
    );
    peer.until(dispatch, |p| p.responses.contains(&(id, result)));
}
