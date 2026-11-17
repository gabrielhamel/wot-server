use super::{
    Peer, Pickle, Value,
    ammunition_flow::{receipt, update},
    depot_flow::{row, send_array, send_four},
    dict,
};
use wot_server::{client_gateway::mercury::app::dispatch::ScriptDispatch, pickle};

const REPAIR_KIT: i32 = (4 << 8) | 0xfb;
const LARGE_REPAIR_KIT: i32 = (5 << 8) | 0xfb;
const FIRST_AID: i32 = (2 << 8) | 0xfb;
const AUTO_EXTINGUISHER: i32 = (1 << 8) | 0xfb;
const FIREFIGHTING: i32 = (113 << 8) | 0xfb;
const CHOCOLATE: i32 = (9 << 8) | 0xfb;
const QUALITY_FUEL: i32 = (65 << 8) | 0xfb;
const EXCELLENT_FUEL: i32 = (66 << 8) | 0xfb;
const UNAVAILABLE: i32 = (900 << 8) | 0xfb;

pub fn exercise(peer: &mut Peer, dispatch: &ScriptDispatch, before: &Pickle) -> Pickle {
    let layout = |shop, vehicle, items: [i32; 3], directive| {
        let mut args = vec![shop, vehicle, 0, 0, 8];
        args.extend(
            items
                .into_iter()
                .chain([directive])
                .flat_map(|item| [item, i32::from(item != 0)]),
        );
        args
    };
    let desired = [FIRST_AID, AUTO_EXTINGUISHER, REPAIR_KIT];
    for (request, args, result) in [
        (680, layout(0, 1, desired, FIREFIGHTING), -4),
        (681, layout(1, 99, desired, FIREFIGHTING), -1),
        (
            682,
            layout(1, 1, [FIRST_AID, 0, -FIRST_AID], FIREFIGHTING),
            -1,
        ),
        (683, layout(1, 1, [CHOCOLATE, 0, 0], FIREFIGHTING), -1),
        (
            684,
            layout(1, 1, [QUALITY_FUEL, EXCELLENT_FUEL, 0], FIREFIGHTING),
            -1,
        ),
        (685, layout(1, 1, [UNAVAILABLE, 0, 0], FIREFIGHTING), -1),
        (686, layout(1, 1, desired, 0), -1),
        (687, layout(1, 1, [i32::MIN, 0, 0], FIREFIGHTING), -2),
    ] {
        send_array(peer, dispatch, request, 108, &args, result);
    }
    send_four(peer, dispatch, 688, 302, [1, UNAVAILABLE.into(), 1, 0], -1);
    send_four(peer, dispatch, 689, 302, [1, REPAIR_KIT.into(), 1, 0], 0);
    assert_eq!(
        dict(&update(peer, 21), "inventory"),
        pickle!({11: {(REPAIR_KIT): 1}})
    );

    send_array(
        peer,
        dispatch,
        690,
        108,
        &layout(1, 1, desired, FIREFIGHTING),
        0,
    );
    let mounted = update(peer, 22);
    let Pickle::I64(credits) = dict(&dict(before, "stats"), "credits") else {
        panic!("Missing credits")
    };
    assert_eq!(
        dict(&dict(&mounted, "stats"), "credits"),
        (credits - 26_000).into()
    );
    let replacement = Pickle::Tuple(vec![1.into(), "_r".into()]);
    assert_eq!(
        dict(&mounted, "inventory"),
        pickle!({1: {
            "eqs": {1: [(FIRST_AID), (AUTO_EXTINGUISHER), (REPAIR_KIT)]},
            "eqsLayout": {(replacement.clone()): [[(FIRST_AID), (AUTO_EXTINGUISHER), (REPAIR_KIT)]]}
        }, 11: {(REPAIR_KIT): None}})
    );
    assert_eq!(
        dict(&receipt(peer, 690), "eqs"),
        pickle!([
            ((FIRST_AID), (3000, 0), 1),
            ((AUTO_EXTINGUISHER), (20000, 0), 1)
        ])
    );
    send_array(
        peer,
        dispatch,
        691,
        108,
        &layout(1, 1, desired, FIREFIGHTING),
        0,
    );

    send_array(
        peer,
        dispatch,
        692,
        108,
        &[
            1,
            1,
            0,
            1,
            8,
            FIRST_AID,
            1,
            AUTO_EXTINGUISHER,
            1,
            REPAIR_KIT,
            1,
            0,
            0,
        ],
        0,
    );
    assert_eq!(
        row(&dict(&update(peer, 23), "inventory"), 11),
        pickle!({(FIREFIGHTING): 1})
    );
    send_four(peer, dispatch, 693, 107, [1, 8, 0, 0], 0);
    assert_eq!(
        dict(&update(peer, 24), "inventory"),
        pickle!({1: {"settings": {1: 134}}})
    );

    let current_shells = row(&dict(&row(&dict(before, "inventory"), 1), "shells"), 1);
    let Pickle::List(shells) = current_shells else {
        panic!("Missing shells")
    };
    let Pickle::I64(shell) = shells[0] else {
        panic!("Missing shell ID")
    };
    let shell = i32::try_from(shell).unwrap();
    let combined = |shell, count| {
        [
            1, 1, 2, shell, count, 0, 6, REPAIR_KIT, 1, 0, 0, FIRST_AID, 1,
        ]
    };
    send_array(peer, dispatch, 694, 108, &combined(shell, i32::MAX), -1);
    send_array(peer, dispatch, 695, 108, &combined(shell, 0), 0);
    let combined = update(peer, 25);
    assert_eq!(dict(&combined, "prevRev"), 24.into());
    let inventory = dict(&combined, "inventory");
    assert_eq!(row(&inventory, 11), pickle!({(AUTO_EXTINGUISHER): 1}));
    let Pickle::List(unloaded) = row(&dict(&row(&inventory, 1), "shells"), 1) else {
        panic!("Missing updated shells")
    };
    assert!(
        unloaded
            .as_chunks::<2>()
            .0
            .iter()
            .all(|pair| pair[1] == 0.into())
    );
    assert_eq!(
        row(&dict(&row(&inventory, 1), "eqs"), 1),
        pickle!([(REPAIR_KIT), 0, (FIRST_AID)])
    );
    assert_eq!(dict(&combined, "stats"), dict(&mounted, "stats"));

    send_array(
        peer,
        dispatch,
        696,
        108,
        &layout(1, 1, [REPAIR_KIT, AUTO_EXTINGUISHER, FIRST_AID], 0),
        0,
    );
    assert_eq!(
        row(&dict(&update(peer, 26), "inventory"), 11),
        pickle!({(AUTO_EXTINGUISHER): None})
    );
    assert_eq!(dict(&receipt(peer, 696), "eqs"), pickle!([]));

    let screenshot_layout = [FIRST_AID, AUTO_EXTINGUISHER, LARGE_REPAIR_KIT];
    send_array(
        peer,
        dispatch,
        698,
        108,
        &layout(1, 3, screenshot_layout, 0),
        0,
    );
    let isu = update(peer, 27);
    let vehicles = row(&dict(&isu, "inventory"), 1);
    assert_eq!(
        row(&dict(&vehicles, "eqs"), 3),
        pickle!([(FIRST_AID), (AUTO_EXTINGUISHER), (LARGE_REPAIR_KIT)])
    );
    assert_eq!(
        dict(&dict(&isu, "stats"), "credits"),
        (credits - 69_000).into()
    );

    peer.command(
        dispatch,
        "doCmdInt3",
        vec![
            Value::Int16(697),
            Value::Int16(100),
            Value::Int64(0),
            Value::Int64(0),
            Value::Int64(0),
        ],
    );
    peer.until(dispatch, |p| p.has_resource(697));
    let cache = peer.resource(697);
    let vehicles = row(&dict(&cache, "inventory"), 1);
    assert_eq!(dict(&cache, "rev"), 27.into());
    assert_eq!(row(&dict(&vehicles, "settings"), 1), 134.into());
    assert_eq!(
        row(&dict(&vehicles, "eqsLayout"), 1),
        pickle!([[(REPAIR_KIT), (AUTO_EXTINGUISHER), (FIRST_AID)]])
    );
    cache
}
