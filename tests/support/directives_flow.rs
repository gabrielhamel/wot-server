use super::ammunition_flow::update;
use super::{
    Peer, Pickle, Value,
    depot_flow::{row, send_array, send_four},
    dict,
};
use wot_platform::mercury::app::dispatch::ScriptDispatch;
use wot_platform::pickle;

const FIREFIGHTING: i32 = (113 << 8) | 0xfb;
const OPTICS: i32 = (106 << 8) | 0xfb;
const OUT_OF_SHOP: i32 = (114 << 8) | 0xfb;
const REPAIR_KIT: i32 = (4 << 8) | 0xfb;

pub fn exercise(peer: &mut Peer, dispatch: &ScriptDispatch, before: &Pickle) -> Pickle {
    let layout =
        |shop, vehicle, item, count| [shop, vehicle, 0, 1, 8, 0, 0, 0, 0, 0, 0, item, count];

    for (id, args, result) in [
        (660, layout(0, 1, FIREFIGHTING, 1), -4),
        (661, layout(1, 99, FIREFIGHTING, 1), -1),
        (662, layout(1, 2, FIREFIGHTING, 1), -1),
        (663, layout(1, 1, FIREFIGHTING, i32::MAX), -2),
        (664, layout(1, 1, REPAIR_KIT, 1), -1),
        (665, layout(1, 1, OUT_OF_SHOP, 1), -1),
    ] {
        send_array(peer, dispatch, id, 108, &args, result);
    }
    send_array(
        peer,
        dispatch,
        666,
        108,
        &[1, 1, 0, 1, 2, FIREFIGHTING, 1],
        -1,
    );
    let mut injected = layout(1, 1, FIREFIGHTING, 1);
    injected[5..7].copy_from_slice(&[REPAIR_KIT, 1]);
    send_array(peer, dispatch, 667, 108, &injected, -1);
    send_four(peer, dispatch, 668, 302, [1, OUT_OF_SHOP.into(), 1, 0], -1);
    send_four(peer, dispatch, 669, 107, [2, 128, 1, 0], -1);

    send_array(peer, dispatch, 670, 108, &layout(1, 1, FIREFIGHTING, 1), 0);
    let mounted = update(peer, 13);
    let Pickle::I64(credits) = dict(&dict(before, "stats"), "credits") else {
        panic!("Missing credits")
    };
    assert_eq!(
        dict(&dict(&mounted, "stats"), "credits"),
        (credits - 20_000).into()
    );
    let replacement = Pickle::Tuple(vec![1.into(), "_r".into()]);
    assert_eq!(
        dict(&mounted, "inventory"),
        pickle!({1: {"boosters": {1: [(FIREFIGHTING)]}, "boostersLayout": {(replacement.clone()): [[(FIREFIGHTING)]]}}})
    );
    send_array(peer, dispatch, 671, 108, &layout(1, 1, FIREFIGHTING, 1), 0);

    send_four(peer, dispatch, 672, 107, [1, 128, 1, 0], 0);
    let settings = update(peer, 14);
    assert_eq!(
        dict(&settings, "inventory"),
        pickle!({1: {"settings": {1: 142}}})
    );
    send_four(peer, dispatch, 673, 302, [1, OPTICS.into(), 2, 0], 0);
    let bought = update(peer, 15);
    assert_eq!(dict(&bought, "inventory"), pickle!({11: {(OPTICS): 2}}));
    assert_eq!(dict(&dict(&bought, "stats"), "crystal"), 9984.into());

    send_array(peer, dispatch, 674, 108, &layout(1, 1, OPTICS, 1), 0);
    let swapped = update(peer, 16);
    assert_eq!(
        row(&dict(&swapped, "inventory"), 11),
        pickle!({(FIREFIGHTING): 1, (OPTICS): 1})
    );
    assert_eq!(dict(&swapped, "stats"), dict(&bought, "stats"));

    send_array(peer, dispatch, 675, 108, &layout(1, 1, 0, 0), 0);
    let removed = update(peer, 17);
    assert_eq!(
        dict(&removed, "inventory"),
        pickle!({1: {"boosters": {1: []}, "boostersLayout": {(replacement): [[]]}}, 11: {(OPTICS): 2}})
    );
    send_array(peer, dispatch, 676, 108, &layout(1, 1, FIREFIGHTING, 1), 0);
    let remounted = update(peer, 18);
    assert_eq!(
        row(&dict(&remounted, "inventory"), 11),
        pickle!({(FIREFIGHTING): None})
    );
    assert_eq!(dict(&remounted, "stats"), dict(&bought, "stats"));
    send_four(peer, dispatch, 677, 107, [1, 128, 0, 0], 0);
    assert_eq!(
        dict(&update(peer, 19), "inventory"),
        pickle!({1: {"settings": {1: 14}}})
    );
    send_four(peer, dispatch, 678, 107, [1, 128, 1, 0], 0);
    update(peer, 20);

    peer.command(
        dispatch,
        "doCmdInt3",
        vec![
            Value::Int16(679),
            Value::Int16(100),
            Value::Int64(0),
            Value::Int64(0),
            Value::Int64(0),
        ],
    );
    peer.until(dispatch, |p| p.has_resource(679));
    let cache = peer.resource(679);
    assert_eq!(dict(&cache, "rev"), 20.into());
    let vehicles = row(&dict(&cache, "inventory"), 1);
    assert_eq!(
        row(&dict(&vehicles, "boosters"), 1),
        pickle!([(FIREFIGHTING)])
    );
    assert_eq!(
        row(&dict(&vehicles, "boostersLayout"), 1),
        pickle!([[(FIREFIGHTING)]])
    );
    assert_eq!(row(&dict(&vehicles, "settings"), 1), 142.into());
    cache
}
