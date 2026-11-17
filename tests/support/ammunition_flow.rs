use super::{Peer, Pickle, Value, dict};
use wot_server::player_sessions::mercury::{
    app::dispatch::ScriptDispatch,
    script::{PythonValue, StringValue},
};

pub fn exercise(peer: &mut Peer, dispatch: &ScriptDispatch) -> Pickle {
    let initial = peer.resource(226);
    let Pickle::List(flat) = shells(&initial) else {
        panic!("Shell list")
    };
    let mut layout: Vec<i32> = flat
        .iter()
        .map(|value| match value {
            Pickle::I64(n) => i32::try_from(*n).unwrap(),
            _ => panic!("Shell ID or count"),
        })
        .collect();
    let source = (0..layout.len())
        .step_by(2)
        .find(|&i| layout[i + 1] >= 2)
        .unwrap();
    let other = (0..layout.len()).step_by(2).find(|&i| i != source).unwrap();
    let shell_id = i64::from(layout[source]);

    let send = |peer: &mut Peer, id, shop_rev, vehicle, layout: &[i32]| {
        let args = [shop_rev, vehicle, layout.len() as i32]
            .into_iter()
            .chain(layout.iter().copied())
            .chain([0, 0])
            .map(Value::Int32)
            .collect();
        peer.command(
            dispatch,
            "doCmdIntArr",
            vec![Value::Int16(id), Value::Int16(108), Value::Seq(args)],
        );
    };
    for (id, shop_rev, vehicle, bad_layout, code) in [
        (228, 0, 42, layout.clone(), -4),
        (229, 1, 42, vec![i32::MIN, 1], -2),
        (230, 1, 42, vec![layout[source]], -2),
        (231, 1, 42, vec![layout[source], -1], -2),
        (232, 1, 42, vec![layout[source], i32::MAX], -1),
        (233, 1, 42, vec![layout[source], 0, -layout[source], 0], -1),
        (234, 1, 42, vec![1, 0], -1),
        (235, 1, 1, layout.clone(), -1),
    ] {
        send(peer, id, shop_rev, vehicle, &bad_layout);
        peer.until(dispatch, |p| p.responses.contains(&(id, code)));
    }

    layout[source + 1] -= 2;
    send(peer, 236, 1, 42, &layout);
    peer.until(dispatch, |p| p.responses.contains(&(236, 0)));
    let unload = update(peer, 5);
    assert_eq!(dict(&unload, "prevRev"), Pickle::I64(4));
    assert_eq!(
        shells(&unload),
        Pickle::List(layout.iter().copied().map(Pickle::from).collect())
    );
    assert_eq!(depot(&unload), wot_server::pickle!({(shell_id): 2}));
    assert_eq!(dict(&unload, "stats"), money(&initial));
    assert_eq!(receipt(peer, 236), wot_server::pickle!({"shells": []}));

    layout[source + 1] += 2;
    send(peer, 237, 1, 42, &layout);
    peer.until(dispatch, |p| p.responses.contains(&(237, 0)));
    let reload = update(peer, 6);
    assert_eq!(depot(&reload), wot_server::pickle!({(shell_id): None}));
    assert_eq!(dict(&reload, "stats"), money(&initial));
    assert_eq!(receipt(peer, 237), wot_server::pickle!({"shells": []}));

    layout[source + 1] -= 1;
    layout[other + 1] += 1;
    send(peer, 238, 1, 42, &layout);
    peer.until(dispatch, |p| p.responses.contains(&(238, 0)));
    let purchase = update(peer, 7);
    assert_eq!(depot(&purchase), wot_server::pickle!({(shell_id): 1}));
    assert_eq!(
        shells(&purchase),
        Pickle::List(layout.iter().copied().map(Pickle::from).collect())
    );

    let shell = Pickle::from(layout[other]);
    let prices = dict(&dict(&peer.resource(222), "items"), "itemPrices");
    let Pickle::Object(prices) = prices else {
        panic!("Shop item prices")
    };
    let prices = dict(prices.state.as_ref().unwrap(), "_itemsPriceInfo");
    let item = get(&prices, shell.clone());
    let credits = optional_amount(&item, "credits");
    let gold = optional_amount(&item, "gold");
    assert!(credits + gold > 0);
    assert_eq!(
        dict(&purchase, "stats"),
        wot_server::pickle!({"credits": (1234 - credits), "gold": (1_000_000 - gold)})
    );
    assert_eq!(
        receipt(peer, 238),
        wot_server::pickle!({"shells": [((shell), (credits, gold), 1)]})
    );

    // Submitting the same target again must not consume stock, charge, or advance the revision.
    send(peer, 239, 1, 42, &layout);
    peer.until(dispatch, |p| p.responses.contains(&(239, 0)));
    assert_eq!(receipt(peer, 239), wot_server::pickle!({}));
    peer.command(
        dispatch,
        "doCmdInt3",
        vec![
            Value::Int16(240),
            Value::Int16(100),
            Value::Int64(0),
            Value::Int64(0),
            Value::Int64(0),
        ],
    );
    peer.until(dispatch, |p| p.has_resource(240));
    let cache = peer.resource(240);
    assert_eq!(dict(&cache, "rev"), Pickle::I64(7));
    assert_eq!(shells(&cache), shells(&purchase));
    assert_eq!(money(&cache), dict(&purchase, "stats"));
    assert_eq!(depot(&cache), depot(&purchase));
    let vehicle = get(&dict(&cache, "inventory"), Pickle::I64(1));
    let layout = get(&dict(&vehicle, "shellsLayout"), Pickle::I64(42));
    let Pickle::Dict(guns) = layout else {
        panic!("Turret/gun layout")
    };
    assert_eq!(
        guns.values().next(),
        Some(&Pickle::List(vec![shells(&cache)]))
    );
    cache
}

fn get(value: &Pickle, key: Pickle) -> Pickle {
    let Pickle::Dict(map) = value else {
        panic!("Dictionary")
    };
    map[&key].clone()
}

fn shells(cache_or_diff: &Pickle) -> Pickle {
    let vehicle = get(&dict(cache_or_diff, "inventory"), Pickle::I64(1));
    get(&dict(&vehicle, "shells"), Pickle::I64(42))
}

fn depot(cache_or_diff: &Pickle) -> Pickle {
    get(&dict(cache_or_diff, "inventory"), Pickle::I64(10))
}

fn money(cache: &Pickle) -> Pickle {
    let stats = dict(cache, "stats");
    wot_server::pickle!({"credits": (dict(&stats, "credits")), "gold": (dict(&stats, "gold"))})
}

fn optional_amount(value: &Pickle, currency: &str) -> i64 {
    let Pickle::Dict(map) = value else {
        panic!("Currency dictionary")
    };
    match map.get(&Pickle::from(currency)) {
        Some(Pickle::I64(n)) => *n,
        None => 0,
        _ => panic!("Currency amount"),
    }
}

pub(super) fn update(peer: &Peer, revision: i64) -> Pickle {
    peer.methods
        .iter()
        .filter_map(|(name, args)| {
            if !name.ends_with("update") {
                return None;
            }
            let value = pickle_value(&args[0]);
            (dict(&value, "rev") == Pickle::I64(revision)).then_some(value)
        })
        .next()
        .expect("Revision update precedes command response")
}

pub(super) fn receipt(peer: &Peer, request_id: i64) -> Pickle {
    let (_, args) = peer
        .methods
        .iter()
        .find(|(name, args)| {
            name.ends_with("onCmdResponseExt") && args[0].as_i64() == Some(request_id)
        })
        .expect("Extended command response");
    pickle_value(&args[3])
}

fn pickle_value(value: &Value) -> Pickle {
    match value {
        Value::Python(PythonValue::Decoded(value)) | Value::String(StringValue::Python(value)) => {
            value.clone()
        }
        Value::Python(PythonValue::Raw(bytes)) | Value::String(StringValue::Raw(bytes)) => {
            wot_server::platform::client_formats::pickle::from_bytes(bytes).unwrap()
        }
        other => panic!("Expected pickle, got {other:?}"),
    }
}
