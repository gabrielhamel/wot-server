use super::{Peer, Pickle, Value, dict};
use wot_server::access::mercury::app::dispatch::ScriptDispatch;

pub const M14_41: i32 = 673;
pub const SMALL_REPAIR_KIT: i32 = (4 << 8) | 0xfb;

pub fn buy_with_consumables(peer: &mut Peer, dispatch: &ScriptDispatch, before: &Pickle) -> Pickle {
    use super::{ammunition_flow::update, depot_flow::send_array};
    let Pickle::I64(revision) = dict(before, "rev") else {
        panic!("Account revision")
    };
    let before_stats = dict(before, "stats");
    let Pickle::I64(credits) = dict(&before_stats, "credits") else {
        panic!("Credits")
    };
    for (request, layout, result) in [
        (519, vec![1, SMALL_REPAIR_KIT], -2),
        (520, vec![2, SMALL_REPAIR_KIT, 2], -2),
        (521, vec![2, i32::MIN, 1], -2),
        (522, vec![4, SMALL_REPAIR_KIT, 1, SMALL_REPAIR_KIT, 1], -1),
        (523, vec![2, SMALL_REPAIR_KIT, 1], 0),
    ] {
        let mut args = vec![1, M14_41, 17, 0, -1];
        args.extend(layout);
        send_array(peer, dispatch, request, 301, &args, result);
    }
    let change = update(peer, revision + 1);
    assert_eq!(dict(&change, "prevRev"), Pickle::from(revision));
    let stats = dict(&change, "stats");
    assert_eq!(
        dict(&stats, "credits"),
        Pickle::from(credits - 4_100 - 1_704 - 3_000)
    );
    assert_eq!(dict(&stats, "slots"), dict(&before_stats, "slots"));
    let Pickle::Dict(inventory) = dict(&change, "inventory") else {
        panic!("New vehicle inventory")
    };
    let vehicles = &inventory[&Pickle::from(1)];
    assert_eq!(
        dict(vehicles, "shells"),
        wot_server::pickle!({3: [522154, 228, 522410, 0, 522666, 12]}),
    );
    assert_eq!(
        dict(vehicles, "eqs"),
        wot_server::pickle!({3: [(SMALL_REPAIR_KIT)]})
    );
    assert_eq!(
        dict(vehicles, "eqsLayout"),
        wot_server::pickle!({3: [[(SMALL_REPAIR_KIT)]]}),
    );
    let Pickle::Dict(crews) = dict(vehicles, "crew") else {
        panic!("New vehicle crew")
    };
    let Pickle::List(crew) = &crews[&Pickle::from(3)] else {
        panic!("M14/41 crew")
    };
    assert_eq!(crew.len(), 4);
    let Pickle::Dict(tankmen) = dict(&inventory[&Pickle::from(8)], "compDescr") else {
        panic!("Recruited tankmen")
    };
    assert!(crew.iter().all(|id| tankmen.contains_key(id)));
    send_array(
        peer,
        dispatch,
        525,
        301,
        &[1, M14_41, 17, 0, -1, 2, SMALL_REPAIR_KIT, 1],
        -1,
    );
    peer.command(
        dispatch,
        "doCmdInt3",
        vec![
            Value::Int16(524),
            Value::Int16(100),
            Value::Int64(0),
            Value::Int64(0),
            Value::Int64(0),
        ],
    );
    peer.until(dispatch, |p| p.has_resource(524));
    let cache = peer.resource(524);
    assert_eq!(dict(&cache, "rev"), Pickle::from(revision + 1));
    cache
}

pub fn buy_slot(
    low_balance_peer: &mut Peer,
    buyer: &mut Peer,
    dispatch: &ScriptDispatch,
    previous: &Pickle,
    shop: &Pickle,
) -> Pickle {
    let Pickle::I64(shop_rev) = dict(shop, "rev") else {
        panic!("Shop revision")
    };
    let buy = |peer: &mut Peer, id, revision, arg1, arg2| {
        peer.command(
            dispatch,
            "doCmdInt3",
            vec![
                Value::Int16(id),
                Value::Int16(205),
                Value::Int64(revision),
                Value::Int64(arg1),
                Value::Int64(arg2),
            ],
        );
    };
    buy(low_balance_peer, 513, shop_rev, 0, 0);
    low_balance_peer.until(dispatch, |p| p.responses.contains(&(513, -1)));
    for (id, revision, arg1, arg2, result) in [
        (514, shop_rev, 1, 0, -2),
        (515, shop_rev, 0, -1, -2),
        (516, shop_rev - 1, 0, 0, -4),
        (517, shop_rev, 0, 0, 0),
    ] {
        buy(buyer, id, revision, arg1, arg2);
        buyer.until(dispatch, |p| p.responses.contains(&(id, result)));
    }
    let Pickle::I64(revision) = dict(previous, "rev") else {
        panic!("Account revision")
    };
    let before = dict(previous, "stats");
    let Pickle::I64(credits) = dict(&before, "credits") else {
        panic!("Credits")
    };
    let Pickle::I64(slots) = dict(&before, "slots") else {
        panic!("Vehicle slots")
    };
    let update = super::ammunition_flow::update(buyer, revision + 1);
    assert_eq!(dict(&update, "prevRev"), Pickle::I64(revision));
    let stats = dict(&update, "stats");
    assert_eq!(dict(&stats, "credits"), Pickle::I64(credits - 250_000));
    assert_eq!(dict(&stats, "slots"), Pickle::I64(slots + 1));
    assert_eq!(dict(&stats, "gold"), dict(&before, "gold"));
    buyer.command(
        dispatch,
        "doCmdInt3",
        vec![
            Value::Int16(518),
            Value::Int16(100),
            Value::Int64(0),
            Value::Int64(0),
            Value::Int64(0),
        ],
    );
    buyer.until(dispatch, |p| p.has_resource(518));
    let cache = buyer.resource(518);
    assert_eq!(dict(&cache, "rev"), Pickle::I64(revision + 1));
    assert_eq!(dict(&cache, "inventory"), dict(previous, "inventory"));
    let stored = dict(&cache, "stats");
    for key in ["slots", "credits", "gold"] {
        assert_eq!(dict(&stored, key), dict(&stats, key));
    }
    cache
}

pub fn exercise(peer: &mut Peer, dispatch: &ScriptDispatch, previous: &Pickle) -> Pickle {
    let buy = |peer: &mut Peer, id, args: &[i32]| {
        peer.command(
            dispatch,
            "doCmdIntArr",
            vec![
                Value::Int16(id),
                Value::Int16(301),
                Value::Seq(args.iter().copied().map(Value::Int32).collect()),
            ],
        );
    };
    for (id, args, code) in [
        (501, vec![1, 2065, 17, 0, -1], -2),
        (502, vec![1, 2065, 17, 0, -1, 0, 999], -2),
        (503, vec![1, 2065, 17, 0, -1, 1], -2),
        (504, vec![1, 2065, 17, 0, 5, 0], -2),
        (505, vec![1, 2065, 2, 0, -1, 0], -2),
        (506, vec![0, 2065, 17, 0, -1, 0], -4),
        (507, vec![1, 2065, 17, 0, -1, 0], -1),
    ] {
        buy(peer, id, &args);
        peer.until(dispatch, |p| p.responses.contains(&(id, code)));
    }
    peer.command(
        dispatch,
        "doCmdInt3",
        vec![
            Value::Int16(508),
            Value::Int16(201),
            Value::Int64(3089),
            Value::Int64(1),
            Value::Int64(0),
        ],
    );
    peer.until(dispatch, |p| p.responses.contains(&(508, 0)));
    // Duplicate research must not charge XP or advance the revision.
    peer.command(
        dispatch,
        "doCmdInt3",
        vec![
            Value::Int16(509),
            Value::Int16(201),
            Value::Int64(3089),
            Value::Int64(1),
            Value::Int64(0),
        ],
    );
    peer.until(dispatch, |p| p.responses.contains(&(509, -1)));
    buy(peer, 510, &[1, 2065, 17, 0, -1, 0]);
    peer.until(dispatch, |p| p.responses.contains(&(510, 0)));
    let update = super::ammunition_flow::update(peer, 9);
    let Pickle::Dict(delta) = dict(&update, "inventory") else {
        panic!("inventory update")
    };
    let Pickle::Dict(descriptors) = dict(&delta[&Pickle::from(1)], "compDescr") else {
        panic!("descriptor update")
    };
    assert!(descriptors.contains_key(&Pickle::from(2)));
    assert!(!descriptors.contains_key(&Pickle::from(1)));
    buy(peer, 511, &[1, 2065, 17, 0, -1, 0]);
    peer.until(dispatch, |p| p.responses.contains(&(511, -1)));
    peer.command(
        dispatch,
        "doCmdInt3",
        vec![
            Value::Int16(512),
            Value::Int16(100),
            Value::Int64(0),
            Value::Int64(0),
            Value::Int64(0),
        ],
    );
    peer.until(dispatch, |p| p.has_resource(512));
    let next = peer.resource(512);
    assert_eq!(dict(&next, "rev"), Pickle::from(9));
    let stats = dict(&next, "stats");
    assert_eq!(dict(&stats, "freeXP"), Pickle::from(1_000_000 - 35 - 275));
    let Pickle::I64(credits) = dict(&dict(previous, "stats"), "credits") else {
        panic!("credits")
    };
    assert_eq!(
        dict(&stats, "credits"),
        Pickle::from(credits - 3_000 - 1_560)
    );
    let Pickle::Set(unlocks) = dict(&stats, "unlocks") else {
        panic!("unlocks")
    };
    assert!(unlocks.contains(&Pickle::from(2065)));
    let Pickle::Dict(inventory) = dict(&next, "inventory") else {
        panic!("inventory")
    };
    let vehicles = &inventory[&Pickle::from(1)];
    let Pickle::Dict(descriptors) = dict(vehicles, "compDescr") else {
        panic!("descriptors")
    };
    assert_eq!(descriptors.len(), 2);
    let Pickle::Bytes(pz) = &descriptors[&Pickle::from(2)] else {
        panic!("PzII descriptor")
    };
    assert_eq!(pz[0], 0x11);
    assert_eq!(pz[1], 8);
    let Pickle::Dict(crews) = dict(vehicles, "crew") else {
        panic!("crew")
    };
    let Pickle::List(crew) = &crews[&Pickle::from(2)] else {
        panic!("PzII crew")
    };
    assert_eq!(crew.len(), 3);
    let Pickle::Dict(shells) = dict(vehicles, "shells") else {
        panic!("shells")
    };
    assert_eq!(
        shells[&Pickle::from(2)],
        wot_server::pickle!([119066, 520, 119322, 0])
    );
    let Pickle::Dict(tankmen) = dict(&inventory[&Pickle::from(8)], "compDescr") else {
        panic!("tankmen")
    };
    assert!(crew.iter().all(|id| tankmen.contains_key(id)));
    next
}
