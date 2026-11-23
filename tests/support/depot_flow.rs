use super::{Peer, Pickle, Value, dict};
use wot_platform::mercury::app::dispatch::ScriptDispatch;
use wot_platform::pickle;

const RAMMER: i32 = (81 << 8) | 0xf9;
const AIMING: i32 = (101 << 8) | 0xf9;
const REPAIR_KIT: i32 = (4 << 8) | 0xfb;
const T34_STOCK_GUN: i32 = (2038 << 8) | 4;

pub fn exercise(peer: &mut Peer, dispatch: &ScriptDispatch, initial: &Pickle) -> Pickle {
    let initial_inventory = dict(initial, "inventory");
    let Pickle::List(shells) = row(&dict(&row(&initial_inventory, 1), "shells"), 1) else {
        panic!("T-34 shell list");
    };
    let mut layout: Vec<i32> = shells.iter().map(integer).map(|n| n as i32).collect();
    let source = layout
        .as_chunks::<2>()
        .0
        .iter()
        .position(|&[_, count]| count >= 2)
        .unwrap();
    let shell = layout[2 * source];
    layout[2 * source + 1] -= 2;
    let unload: Vec<_> = [1, 1, layout.len() as i32]
        .into_iter()
        .chain(layout)
        .chain([0, 0])
        .collect();
    send_array(peer, dispatch, 620, 108, &unload, 0);
    assert_eq!(
        row(
            &dict(&super::ammunition_flow::update(peer, 7), "inventory"),
            10
        ),
        pickle!({(shell): 2})
    );
    send_array(peer, dispatch, 621, 102, &[1, 1, 0, 2, 1, 1, 0], 0);
    send_four(peer, dispatch, 622, 302, [1, REPAIR_KIT.into(), 2, 0], 0);
    let stocked = super::ammunition_flow::update(peer, 9);
    assert_eq!(
        row(&dict(&stocked, "inventory"), 11),
        pickle!({(REPAIR_KIT): 2})
    );
    send_four(peer, dispatch, 634, 302, [1, T34_STOCK_GUN.into(), 1, 0], 0);
    let stocked = super::ammunition_flow::update(peer, 10);
    assert_eq!(
        dict(&stocked, "inventory"),
        pickle!({4: {(T34_STOCK_GUN): 1}})
    );
    let credits = integer(&dict(&dict(&stocked, "stats"), "credits"));

    send_four(peer, dispatch, 623, 305, [0, 10, shell.into(), 1], -4);
    send_four(peer, dispatch, 624, 305, [1, 9, shell.into(), 1], -1);
    send_four(peer, dispatch, 625, 305, [1, 10, shell.into(), 0], -2);
    send_four(peer, dispatch, 626, 305, [1, 10, shell.into(), 3], -1);
    send_four(peer, dispatch, 635, 305, [1, 9, AIMING.into(), 1], -1);
    send_array(peer, dispatch, 627, 310, &[1, 10, shell], -2);
    send_array(
        peer,
        dispatch,
        628,
        310,
        &[1, 9, RAMMER, 1, 10, shell, 1, 10, shell, 2],
        -1,
    );
    send_four(peer, dispatch, 629, 305, [1, 11, REPAIR_KIT.into(), 1], 0);
    let single = super::ammunition_flow::update(peer, 11);
    assert_eq!(dict(&single, "inventory"), pickle!({11: {(REPAIR_KIT): 1}}));
    assert_eq!(
        integer(&dict(&dict(&single, "stats"), "credits")),
        credits + 1500
    );

    send_array(
        peer,
        dispatch,
        630,
        310,
        &[
            1,
            9,
            RAMMER,
            1,
            10,
            shell,
            1,
            10,
            shell,
            1,
            11,
            REPAIR_KIT,
            1,
            4,
            T34_STOCK_GUN,
            1,
        ],
        0,
    );
    let bulk = super::ammunition_flow::update(peer, 12);
    assert_eq!(
        dict(&bulk, "inventory"),
        pickle!({4: {(T34_STOCK_GUN): None}, 9: {(RAMMER): None}, 10: {(shell): None}, 11: {(REPAIR_KIT): None}})
    );
    peer.command(
        dispatch,
        "doCmdInt3",
        vec![
            Value::Int16(631),
            Value::Int16(300),
            Value::Int64(0),
            Value::Int64(0),
            Value::Int64(0),
        ],
    );
    peer.until(dispatch, |p| p.has_resource(631));
    let shop = peer.resource(631);
    let Pickle::Object(prices) = dict(&dict(&shop, "items"), "itemPrices") else {
        panic!("Shop item prices");
    };
    let prices = dict(prices.state.as_ref().unwrap(), "_itemsPriceInfo");
    let price_in_credits = |id| {
        let Pickle::Dict(price) = row(&prices, id) else {
            panic!("Missing item price");
        };
        let amount = |name| price.get(&Pickle::from(name)).map_or(0, integer);
        amount("credits") + 400 * amount("gold")
    };
    assert_eq!(
        integer(&dict(&dict(&bulk, "stats"), "credits")),
        credits
            + 3000
            + (price_in_credits(RAMMER) + 1) / 2
            + 2 * ((price_in_credits(shell) + 1) / 2)
            + (price_in_credits(T34_STOCK_GUN) + 1) / 2,
    );
    send_four(peer, dispatch, 632, 305, [1, 9, RAMMER.into(), 1], -1);
    peer.command(
        dispatch,
        "doCmdInt3",
        vec![
            Value::Int16(633),
            Value::Int16(100),
            Value::Int64(0),
            Value::Int64(0),
            Value::Int64(0),
        ],
    );
    peer.until(dispatch, |p| p.has_resource(633));
    let cache = peer.resource(633);
    let inventory = dict(&cache, "inventory");
    for kind in [4, 10, 11] {
        assert_eq!(row(&inventory, kind), pickle!({}));
    }
    assert_eq!(row(&inventory, 9), row(&initial_inventory, 9));
    assert_eq!(dict(&cache, "rev"), Pickle::from(12));
    cache
}

fn integer(value: &Pickle) -> i64 {
    let Pickle::I64(value) = value else {
        panic!("Missing integer");
    };
    *value
}

pub(super) fn row(table: &Pickle, id: i32) -> Pickle {
    let Pickle::Dict(rows) = table else {
        panic!("Missing inventory table");
    };
    rows[&Pickle::from(id)].clone()
}

pub(super) fn send_four(
    peer: &mut Peer,
    dispatch: &ScriptDispatch,
    id: i16,
    cmd: i16,
    args: [i64; 4],
    result: i16,
) {
    let args = [Value::Int16(id), Value::Int16(cmd)]
        .into_iter()
        .chain(args.map(Value::Int64))
        .collect();
    peer.command(dispatch, "doCmdInt4", args);
    peer.until(dispatch, |p| p.responses.contains(&(id, result)));
}

pub(super) fn send_array(
    peer: &mut Peer,
    dispatch: &ScriptDispatch,
    id: i16,
    cmd: i16,
    args: &[i32],
    result: i16,
) {
    peer.command(
        dispatch,
        "doCmdIntArr",
        vec![
            Value::Int16(id),
            Value::Int16(cmd),
            Value::Seq(args.iter().copied().map(Value::Int32).collect()),
        ],
    );
    peer.until(dispatch, |p| p.responses.contains(&(id, result)));
}
