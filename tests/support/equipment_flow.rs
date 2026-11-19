use super::{Peer, Pickle, Value, dict};
use wot_server::{pickle, player_sessions::test_support::mercury::app::dispatch::ScriptDispatch};

const AIMING: i32 = (101 << 8) | 0xf9;
const RAMMER: i32 = (81 << 8) | 0xf9;
const WRONG_CLASS_RAMMER: i32 = (82 << 8) | 0xf9;
const IMPROVED_CONFIGURATION: i32 = (43 << 8) | 0xf9;
const IMPROVED_OPTICS: i32 = (46 << 8) | 0xf9;
const IMPROVED_RAMMER: i32 = (45 << 8) | 0xf9;
const BOUNTY_AIM_DRIVES: i32 = (50 << 8) | 0xf9;
const BOUNTY_AIM_DRIVES_UPGRADED: i32 = (51 << 8) | 0xf9;
const BOUNTY_TURBO: i32 = (115 << 8) | 0xf9;
const BOUNTY_TURBO_UPGRADED: i32 = (116 << 8) | 0xf9;
const EXPERIMENTAL_MOBILITY_1: i32 = (153 << 8) | 0xf9;
const EXPERIMENTAL_MOBILITY_2: i32 = (154 << 8) | 0xf9;
const EXPERIMENTAL_MOBILITY_3: i32 = (155 << 8) | 0xf9;
const EXPERIMENTAL_ACCURACY_1: i32 = (159 << 8) | 0xf9;
const EXPERIMENTAL_ACCURACY_2: i32 = (160 << 8) | 0xf9;
const EXPERIMENTAL_ACCURACY_3: i32 = (161 << 8) | 0xf9;
const ISU_130: i32 = 3;

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
        (BOUNTY_AIM_DRIVES, 1),
        (BOUNTY_TURBO, 1),
        (EXPERIMENTAL_MOBILITY_1, 2),
        (EXPERIMENTAL_MOBILITY_2, 1),
        (EXPERIMENTAL_MOBILITY_3, 1),
        (EXPERIMENTAL_ACCURACY_3, 9),
        (EXPERIMENTAL_ACCURACY_1, 1),
    ]
    .into_iter()
    .collect()
}

pub fn exercise_special_equipment(
    peer: &mut Peer,
    dispatch: &ScriptDispatch,
    before: &Pickle,
) -> Pickle {
    use super::{ammunition_flow::update, depot_flow::send_four};

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

pub fn exercise_disassembly(peer: &mut Peer, dispatch: &ScriptDispatch, before: &Pickle) -> Pickle {
    use super::{ammunition_flow::update, depot_flow::send_four};

    let Pickle::I64(revision) = dict(before, "rev") else {
        panic!("missing revision");
    };
    let accuracy = i64::from(EXPERIMENTAL_ACCURACY_3);
    for (request, args, result) in [
        (860, [0, 9, accuracy, 9], -4),
        (861, [1, 10, accuracy, 9], -1),
        (862, [1, 9, accuracy, 10], -1),
    ] {
        send_four(peer, dispatch, request, 305, args, result);
    }
    send_layout(
        peer,
        dispatch,
        863,
        310,
        &[
            1,
            9,
            EXPERIMENTAL_MOBILITY_1,
            1,
            9,
            EXPERIMENTAL_MOBILITY_2,
            2,
        ],
        -1,
    );
    assert_eq!(sync(peer, dispatch, 864), *before);

    send_four(peer, dispatch, 865, 305, [1, 9, accuracy, 9], 0);
    let sold = update(peer, revision + 1);
    assert_eq!(
        dict(&sold, "inventory"),
        pickle!({9: {(EXPERIMENTAL_ACCURACY_3): None}})
    );
    let stats = dict(&sold, "stats");
    assert_eq!(dict(&stats, "equipCoin"), 14_400.into());
    assert_eq!(
        dict(&stats, "credits"),
        dict(&dict(before, "stats"), "credits")
    );
    send_four(peer, dispatch, 866, 305, [1, 9, accuracy, 9], -1);
    send_layout(
        peer,
        dispatch,
        867,
        310,
        &[
            1,
            9,
            EXPERIMENTAL_MOBILITY_1,
            1,
            9,
            EXPERIMENTAL_MOBILITY_2,
            1,
        ],
        0,
    );
    let sold = update(peer, revision + 2);
    assert_eq!(
        dict(&sold, "inventory"),
        pickle!({9: {(EXPERIMENTAL_MOBILITY_1): 1, (EXPERIMENTAL_MOBILITY_2): None}})
    );
    assert_eq!(dict(&dict(&sold, "stats"), "equipCoin"), 14_900.into());

    for (request, args, result) in [
        (868, vec![1, ISU_130, EXPERIMENTAL_MOBILITY_3, 0], -2),
        (869, vec![1, 999, EXPERIMENTAL_MOBILITY_3], -1),
        (870, vec![1, ISU_130, BOUNTY_AIM_DRIVES], -1),
        (871, vec![1, ISU_130, EXPERIMENTAL_MOBILITY_3], 0),
        (872, vec![1, ISU_130, EXPERIMENTAL_MOBILITY_3], -1),
    ] {
        send_layout(peer, dispatch, request, 10115, &args, result);
    }
    let removed = update(peer, revision + 3);
    assert_eq!(dict(&dict(&removed, "stats"), "equipCoin"), 16_500.into());
    let mounted = row(&dict(&removed, "inventory"), 1);
    let key = Pickle::Tuple(vec![ISU_130.into(), "_r".into()]);
    assert_eq!(
        dict(&mounted, "devicesLayout"),
        pickle!({(key): [[(IMPROVED_CONFIGURATION), (BOUNTY_AIM_DRIVES)]]})
    );
    assert!(matches!(
        row(&dict(&mounted, "compDescr"), ISU_130),
        Pickle::Bytes(_)
    ));

    send_layout(
        peer,
        dispatch,
        873,
        102,
        &[1, ISU_130, EXPERIMENTAL_MOBILITY_1, 2, 1, 0, 0],
        0,
    );
    send_layout(peer, dispatch, 874, 102, &[1, ISU_130, 0, 2, 1, 0, 0], 0);
    let removed = update(peer, revision + 5);
    assert_eq!(dict(&dict(&removed, "stats"), "equipCoin"), 16_600.into());

    let cache = sync(peer, dispatch, 875);
    assert_eq!(dict(&cache, "rev"), (revision + 5).into());
    let inventory = dict(&cache, "inventory");
    let Pickle::Dict(depot) = row(&inventory, 9) else {
        panic!("missing equipment depot");
    };
    for item in [
        EXPERIMENTAL_ACCURACY_3,
        EXPERIMENTAL_MOBILITY_1,
        EXPERIMENTAL_MOBILITY_2,
        EXPERIMENTAL_MOBILITY_3,
    ] {
        assert!(!depot.contains_key(&item.into()));
    }
    assert_eq!(
        row(&dict(&row(&inventory, 1), "devicesLayout"), ISU_130),
        pickle!([[(IMPROVED_CONFIGURATION), (BOUNTY_AIM_DRIVES)]])
    );
    let stats = dict(&cache, "stats");
    let previous_stats = dict(before, "stats");
    for key in ["credits", "gold", "crystal"] {
        assert_eq!(dict(&stats, key), dict(&previous_stats, key));
    }
    assert_eq!(dict(&stats, "equipCoin"), 16_600.into());
    cache
}

pub fn exercise_bounty_upgrade(
    peer: &mut Peer,
    dispatch: &ScriptDispatch,
    before: &Pickle,
) -> Pickle {
    use super::ammunition_flow::update;

    assert!(peer.equipment_upgrades_enabled);
    peer.command(
        dispatch,
        "doCmdInt3",
        vec![
            Value::Int16(880),
            Value::Int16(300),
            Value::Int64(0),
            Value::Int64(0),
            Value::Int64(0),
        ],
    );
    peer.until(dispatch, |p| p.has_resource(880));
    let shop = peer.resource(880);
    for items in [
        dict(&shop, "items"),
        dict(&dict(&shop, "defaults"), "items"),
    ] {
        let Pickle::Dict(upgrades) = dict(&dict(&items, "operationPrices"), "upgrade") else {
            panic!("missing operation price table");
        };
        for (basic, upgraded) in [
            (BOUNTY_TURBO, BOUNTY_TURBO_UPGRADED),
            (BOUNTY_AIM_DRIVES, BOUNTY_AIM_DRIVES_UPGRADED),
        ] {
            let key = Pickle::Tuple(vec![basic.into(), upgraded.into()]);
            assert_eq!(upgrades.get(&key), Some(&pickle!({"credits": 3_000_000})));
        }
        for (source, target, cost) in [
            (EXPERIMENTAL_ACCURACY_1, EXPERIMENTAL_ACCURACY_2, 400),
            (EXPERIMENTAL_ACCURACY_2, EXPERIMENTAL_ACCURACY_3, 2_000),
        ] {
            let key = Pickle::Tuple(vec![source.into(), target.into()]);
            assert_eq!(upgrades.get(&key), Some(&pickle!({"equipCoin": (cost)})));
        }
        assert!(!upgrades.keys().any(|key| {
            matches!(key, Pickle::Tuple(pair) if pair.first() == Some(&Pickle::from(EXPERIMENTAL_ACCURACY_3)))
        }));
    }

    for (request, args, result) in [
        (881, [0, BOUNTY_TURBO, 0, 0, 0], -4),
        (882, [1, BOUNTY_TURBO, ISU_130, 0, 1], -2),
        (883, [1, 0, ISU_130, 1, 1], -2),
        (884, [1, 0, 999, 0, 1], -1),
        (885, [1, 0, ISU_130, 0, 2], -1),
        (886, [1, AIMING, 0, 0, 0], -1),
    ] {
        send_layout(peer, dispatch, request, 10050, &args, result);
    }
    assert_eq!(sync(peer, dispatch, 887), *before);

    let Pickle::I64(revision) = dict(before, "rev") else {
        panic!("missing revision");
    };
    let original_stats = dict(before, "stats");
    let Pickle::I64(credits) = dict(&original_stats, "credits") else {
        panic!("missing credits");
    };
    send_layout(peer, dispatch, 888, 10050, &[1, BOUNTY_TURBO, 0, 0, 0], 0);
    let depot = update(peer, revision + 1);
    assert_eq!(
        dict(&depot, "inventory"),
        pickle!({9: {(BOUNTY_TURBO): None, (BOUNTY_TURBO_UPGRADED): 1}})
    );
    assert_eq!(
        dict(&dict(&depot, "stats"), "credits"),
        (credits - 3_000_000).into()
    );
    send_layout(peer, dispatch, 889, 10050, &[1, BOUNTY_TURBO, 0, 0, 0], -1);
    send_layout(
        peer,
        dispatch,
        890,
        10050,
        &[1, BOUNTY_TURBO_UPGRADED, 0, 0, 0],
        -1,
    );

    send_layout(peer, dispatch, 891, 10050, &[1, 0, ISU_130, 0, 1], 0);
    let mounted = update(peer, revision + 2);
    let inventory = dict(&mounted, "inventory");
    let vehicle = row(&inventory, 1);
    let key = Pickle::Tuple(vec![ISU_130.into(), "_r".into()]);
    assert_eq!(
        dict(&vehicle, "devicesLayout"),
        pickle!({(key): [[(IMPROVED_CONFIGURATION), (BOUNTY_AIM_DRIVES_UPGRADED)]]})
    );
    assert!(matches!(
        row(&dict(&vehicle, "compDescr"), ISU_130),
        Pickle::Bytes(_)
    ));
    let Pickle::Dict(rows) = inventory else {
        panic!("missing inventory");
    };
    assert!(!rows.contains_key(&Pickle::from(9)));
    send_layout(peer, dispatch, 892, 10050, &[1, 0, ISU_130, 0, 1], -1);

    let cache = sync(peer, dispatch, 893);
    assert_eq!(dict(&cache, "rev"), (revision + 2).into());
    let stats = dict(&cache, "stats");
    assert_eq!(dict(&stats, "credits"), (credits - 6_000_000).into());
    for key in ["gold", "crystal", "equipCoin"] {
        assert_eq!(dict(&stats, key), dict(&original_stats, key));
    }
    let inventory = dict(&cache, "inventory");
    assert_eq!(row(&row(&inventory, 9), BOUNTY_TURBO_UPGRADED), 1.into());
    assert_eq!(
        row(&dict(&row(&inventory, 1), "devicesLayout"), ISU_130),
        pickle!([[(IMPROVED_CONFIGURATION), (BOUNTY_AIM_DRIVES_UPGRADED)]])
    );
    cache
}

pub fn exercise_experimental_upgrade(
    peer: &mut Peer,
    dispatch: &ScriptDispatch,
    before: &Pickle,
) -> Pickle {
    use super::ammunition_flow::update;

    let Pickle::I64(revision) = dict(before, "rev") else {
        panic!("missing revision");
    };
    let original_stats = dict(before, "stats");
    let Pickle::I64(components) = dict(&original_stats, "equipCoin") else {
        panic!("missing Components");
    };

    send_layout(
        peer,
        dispatch,
        894,
        10050,
        &[1, EXPERIMENTAL_ACCURACY_1, 0, 0, 0],
        0,
    );
    let depot = update(peer, revision + 1);
    assert_eq!(
        dict(&depot, "inventory"),
        pickle!({9: {(EXPERIMENTAL_ACCURACY_1): None, (EXPERIMENTAL_ACCURACY_2): 1}})
    );
    assert_eq!(
        dict(&dict(&depot, "stats"), "equipCoin"),
        (components - 400).into()
    );
    send_layout(
        peer,
        dispatch,
        895,
        10050,
        &[1, EXPERIMENTAL_ACCURACY_1, 0, 0, 0],
        -1,
    );

    send_layout(
        peer,
        dispatch,
        896,
        102,
        &[1, ISU_130, EXPERIMENTAL_ACCURACY_2, 2, 1, 0, 0],
        0,
    );
    send_layout(peer, dispatch, 897, 10050, &[1, 0, ISU_130, 0, 2], 0);
    let mounted = update(peer, revision + 3);
    let inventory = dict(&mounted, "inventory");
    let vehicle = row(&inventory, 1);
    let key = Pickle::Tuple(vec![ISU_130.into(), "_r".into()]);
    assert_eq!(
        dict(&vehicle, "devicesLayout"),
        pickle!({(key): [[(IMPROVED_CONFIGURATION), (BOUNTY_AIM_DRIVES_UPGRADED), (EXPERIMENTAL_ACCURACY_3)]]})
    );
    let Pickle::Dict(rows) = inventory else {
        panic!("missing inventory");
    };
    assert!(!rows.contains_key(&Pickle::from(9)));
    assert_eq!(
        dict(&dict(&mounted, "stats"), "equipCoin"),
        (components - 2_400).into()
    );
    send_layout(peer, dispatch, 898, 10050, &[1, 0, ISU_130, 0, 2], -1);

    let cache = sync(peer, dispatch, 899);
    assert_eq!(dict(&cache, "rev"), (revision + 3).into());
    let stats = dict(&cache, "stats");
    assert_eq!(dict(&stats, "equipCoin"), (components - 2_400).into());
    for key in ["credits", "gold", "crystal"] {
        assert_eq!(dict(&stats, key), dict(&original_stats, key));
    }
    cache
}

pub fn verify_unaffordable_upgrade(
    catalog: &wot_server::accounts::test_support::GarageCatalog,
    mut snapshot: wot_server::accounts::test_support::AccountSnapshot,
) {
    use wot_server::accounts::test_support::{
        EquipmentUpgradeSource, ProgressionCommand, ProgressionError,
    };

    snapshot.balances.credits = 2_999_999;
    snapshot.balances.equipment_components = 399;
    for item in [
        BOUNTY_TURBO,
        EXPERIMENTAL_ACCURACY_1,
        EXPERIMENTAL_ACCURACY_2,
    ] {
        snapshot.progression.depot.insert(item, 1);
        let command = ProgressionCommand::UpgradeEquipment(EquipmentUpgradeSource::Depot { item });
        assert!(matches!(
            catalog.plan_progression(snapshot.clone(), command),
            Err(ProgressionError::NotEnoughMoney)
        ));
    }
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
