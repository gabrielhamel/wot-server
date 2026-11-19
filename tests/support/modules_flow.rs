use super::{Peer, Pickle, Value, dict};
use wot_server::player_sessions::test_support::mercury::app::dispatch::ScriptDispatch;

pub fn exercise(peer: &mut Peer, dispatch: &ScriptDispatch) -> Pickle {
    let command = |peer: &mut Peer, id, cmd, a, b, c| {
        peer.command(
            dispatch,
            "doCmdInt3",
            vec![
                Value::Int16(id),
                Value::Int16(cmd),
                Value::Int64(a),
                Value::Int64(b),
                Value::Int64(c),
            ],
        );
    };
    for (id, vehicle, index) in [
        (401, 3089, -1),
        (402, 3089, i64::MAX),
        (403, i64::MAX, 0),
        (404, 3089, 1),
    ] {
        command(peer, id, 201, vehicle, index, 0);
        peer.until(dispatch, |p| {
            p.responses.iter().any(|&(r, code)| r == id && code < 0)
        });
    }
    command(peer, 405, 201, 3089, 0, 0);
    peer.until(dispatch, |p| p.responses.contains(&(405, 0)));
    let send_equip = |peer: &mut Peer, id, rev| {
        peer.command(
            dispatch,
            "doCmdIntArr",
            vec![
                Value::Int16(id),
                Value::Int16(308),
                Value::Seq(
                    [rev, 515604, 1, 0, 0, 0]
                        .into_iter()
                        .map(Value::Int32)
                        .collect(),
                ),
            ],
        );
    };
    send_equip(peer, 406, 0);
    peer.until(dispatch, |p| p.responses.contains(&(406, -4)));
    send_equip(peer, 407, 1);
    peer.until(dispatch, |p| p.responses.contains(&(407, 0)));
    command(peer, 408, 100, 0, 0, 0);
    peer.until(dispatch, |p| p.has_resource(408));
    let upgraded = peer.resource(408);
    let stats = dict(&upgraded, "stats");
    assert_eq!(dict(&stats, "freeXP"), Pickle::I64(1_000_000 - 35));
    let Pickle::Set(unlocks) = dict(&stats, "unlocks") else {
        panic!("unlocks")
    };
    assert!(unlocks.contains(&Pickle::from(515604)));
    let Pickle::Dict(inventory) = dict(&upgraded, "inventory") else {
        panic!("inventory")
    };
    let vehicle = &inventory[&Pickle::from(1)];
    let Pickle::Dict(descriptors) = dict(vehicle, "compDescr") else {
        panic!("descriptors")
    };
    let Pickle::Bytes(descr) = &descriptors[&Pickle::from(1)] else {
        panic!("descriptor")
    };
    let Pickle::Dict(shell_tables) = dict(vehicle, "shells") else {
        panic!("shell tables")
    };
    let Pickle::List(shells) = &shell_tables[&Pickle::from(1)] else {
        panic!("shell layout")
    };
    let Pickle::I64(shell) = shells[0] else {
        panic!("shell ID")
    };
    for (id, count, code) in [(416, i32::MAX, -1), (417, 1, 0)] {
        peer.command(
            dispatch,
            "doCmdIntArr",
            vec![
                Value::Int16(id),
                Value::Int16(108),
                Value::Seq(
                    [1, 1, 2, i32::try_from(shell).unwrap(), count, 0, 0]
                        .into_iter()
                        .map(Value::Int32)
                        .collect(),
                ),
            ],
        );
        peer.until(dispatch, |p| p.responses.contains(&(id, code)));
    }
    // Switching back uses the previous gun returned to the depot.
    let stock = inventory[&Pickle::from(4)].clone();
    let Pickle::Dict(stock) = stock else {
        panic!("gun depot")
    };
    let (&Pickle::I64(old_gun), _) = stock.iter().next().unwrap() else {
        panic!("stock gun")
    };
    assert_ne!(old_gun, 515604);
    assert!(!descr.is_empty());
    command(peer, 409, 101, 1, old_gun, 0);
    peer.until(dispatch, |p| p.responses.contains(&(409, 0)));
    command(peer, 410, 101, 1, 515604, 0);
    peer.until(dispatch, |p| p.responses.contains(&(410, 0)));
    for (id, count, result) in [(411, -1, -1), (412, 101, -1), (413, 2, 0)] {
        peer.command(
            dispatch,
            "doCmdInt4",
            vec![
                Value::Int16(id),
                Value::Int16(302),
                Value::Int64(1),
                Value::Int64(old_gun),
                Value::Int64(count),
                Value::Int64(0),
            ],
        );
        peer.until(dispatch, |p| {
            p.responses
                .iter()
                .any(|&(r, c)| r == id && if result == 0 { c == 0 } else { c < 0 })
        });
    }
    command(peer, 414, 100, 0, 0, 0);
    peer.until(dispatch, |p| p.has_resource(414));
    peer.resource(414)
}
