use super::{
    Peer, Pickle, ScriptDispatch, Value, ammunition_flow::pickle_value, dict, telemetry_capture,
};
use opentelemetry_sdk::trace::SpanData;
use wot_server::player_sessions::mercury::script::StringValue;

pub fn exercise(
    peer: &mut Peer,
    script: &ScriptDispatch,
    account_id: i64,
    vehicle_id: i64,
    available: bool,
) {
    let create = |peer: &mut Peer, arena, comment: &str| {
        peer.command(
            script,
            "accountPrebattle_createTraining",
            vec![
                Value::Int32(arena),
                Value::Int32(900),
                Value::UInt8(0),
                Value::String(StringValue::String(comment.into())),
            ],
        );
    };
    let failure = if available { 6 } else { 14 };
    create(peer, i32::MAX, "");
    peer.until(script, |peer| {
        peer.methods.iter().any(|(name, args)| {
            name == "onPrebattleJoinFailure" && args == &[Value::UInt8(failure)]
        })
    });
    if !available {
        return;
    }

    let before = peer.methods.len();
    create(peer, 83, "training-fixture-comment");
    peer.until(script, |peer| peer.methods.len() >= before + 4);
    let messages = &peer.methods[before..before + 4];
    assert_eq!(messages[0].0, "onPrebattleJoined");
    let room_id = messages[0].1[0].as_i64().unwrap();
    assert!(room_id > 0);
    for ((name, args), update) in messages[1..].iter().zip([7, 10, 1]) {
        assert_eq!(name, "updatePrebattle");
        assert_eq!(args[0], Value::UInt8(update));
    }
    let settings = pickle_value(&messages[1].1[1]);
    for (key, value) in [
        ("type", Pickle::I64(2)),
        ("arenaGuiType", Pickle::I64(2)),
        ("arenaTypeID", Pickle::I64(83)),
        ("roundLength", Pickle::I64(900)),
        ("isOpened", Pickle::Bool(false)),
        ("comment", Pickle::from("training-fixture-comment")),
        ("creatorDBID", Pickle::I64(account_id)),
    ] {
        assert_eq!(dict(&settings, key), value);
    }
    let Pickle::List(roster) = pickle_value(&messages[3].1[1]) else {
        panic!("Missing host roster")
    };
    let [Pickle::Tuple(host)] = roster.as_slice() else {
        panic!("Expected one host")
    };
    assert_eq!(host.len(), 14);
    assert_eq!(host[2], Pickle::I64(account_id));
    assert_eq!(host[5], Pickle::I64(1));
    assert_eq!(host[13], Pickle::I64(0xfe7f));

    let request = |peer: &mut Peer, id, cmd, arg| {
        peer.command(
            script,
            "doCmdInt3",
            vec![
                Value::Int16(id),
                Value::Int16(cmd),
                Value::Int64(arg),
                Value::Int64(0),
                Value::Int64(0),
            ],
        );
    };
    request(peer, 3250, 402, i64::MAX);
    peer.until(script, |peer| peer.responses.contains(&(3250, -1)));
    request(peer, 3251, 402, vehicle_id);
    peer.until(script, |peer| {
        peer.responses.iter().any(|(id, _)| *id == 3251)
    });
    assert!(
        peer.responses.contains(&(3251, 0)),
        "Ready response was {:?}",
        peer.methods.last()
    );
    let (_, state) = peer
        .methods
        .iter()
        .find(|(name, args)| name == "updatePrebattle" && args[0] == Value::UInt8(4))
        .unwrap();
    let Pickle::Tuple(ready) = pickle_value(&state[1]) else {
        panic!("Missing ready state")
    };
    assert_eq!(ready[2], Pickle::I64(4));
    assert!(matches!(&ready[3], Pickle::Bytes(descriptor) if !descriptor.is_empty()));

    let before = peer.methods.len();
    create(peer, 83, "duplicate");
    peer.until(script, |peer| peer.methods.len() >= before + 4);
    assert_eq!(peer.methods[before].0, "onPrebattleJoined");
    assert_eq!(peer.methods[before].1[0].as_i64(), Some(room_id));
    assert_eq!(pickle_value(&peer.methods[before + 1].1[1]), settings);

    request(peer, 3254, 403, 1);
    peer.until(script, |peer| peer.responses.contains(&(3254, 0)));
    let (_, state) = peer
        .methods
        .iter()
        .rev()
        .find(|(name, args)| name == "updatePrebattle" && args[0] == Value::UInt8(4))
        .unwrap();
    let Pickle::Tuple(not_ready) = pickle_value(&state[1]) else {
        panic!("Missing not-ready state")
    };
    assert_eq!(not_ready[2], Pickle::I64(1));
    assert_eq!(not_ready[3], Pickle::I64(0));

    request(peer, 3252, 401, 0);
    peer.until(script, |peer| peer.responses.contains(&(3252, 0)));
    assert!(
        peer.methods
            .iter()
            .any(|(name, _)| name == "onPrebattleLeft")
    );
    request(peer, 3253, 402, vehicle_id);
    peer.until(script, |peer| peer.responses.contains(&(3253, -1)));
    let before = peer.methods.len();
    create(peer, 83, "");
    peer.until(script, |peer| peer.methods.len() >= before + 4);
    assert_eq!(peer.methods[before].0, "onPrebattleJoined");
    assert!(peer.methods[before].1[0].as_i64().unwrap() > room_id);
}

pub fn assert_room_gone(peer: &mut Peer, script: &ScriptDispatch, vehicle_id: i64) {
    peer.command(
        script,
        "doCmdInt3",
        vec![
            Value::Int16(3255),
            Value::Int16(402),
            Value::Int64(vehicle_id),
            Value::Int64(0),
            Value::Int64(0),
        ],
    );
    peer.until(script, |peer| peer.responses.contains(&(3255, -1)));
    assert!(
        !peer
            .methods
            .iter()
            .any(|(name, _)| name == "onPrebattleJoined")
    );
}

pub fn assert_traces(account_id: i64, available: bool) {
    let traces = telemetry_capture::exporter().get_finished_spans().unwrap();
    let has = |span: &SpanData, key, value: &str| {
        span.attributes
            .iter()
            .any(|attr| attr.key.as_str() == key && attr.value.to_string() == value)
    };
    let child_of = |child: &SpanData, parent: &SpanData| {
        child.parent_span_id == parent.span_context.span_id()
            && child.span_context.trace_id() == parent.span_context.trace_id()
    };
    let assert_delivery = |operation, outcome, response| {
        let span = traces
            .iter()
            .find(|span| {
                span.name == operation
                    && has(span, "output.outcome", outcome)
                    && traces.iter().any(|command| {
                        command.name == "player_sessions.commands.command"
                            && child_of(span, command)
                            && traces.iter().any(|root| {
                                root.name == "game.command"
                                    && child_of(command, root)
                                    && has(root, "user.id", &account_id.to_string())
                            })
                    })
            })
            .unwrap_or_else(|| panic!("Missing trace for {operation} {outcome}"));
        assert!(
            traces.iter().any(|child| child.name == "mercury.response"
                && child_of(child, span)
                && has(child, "rpc.method", response)
                && has(child, "output.delivery", "sent")),
            "Missing delivery of {response} for {outcome}"
        );
        span
    };
    let rejection = if available {
        "invalid_request"
    } else {
        "unavailable"
    };
    assert_delivery(
        "player_sessions.create_training_room",
        rejection,
        "onPrebattleJoinFailure",
    );
    if available {
        let create = assert_delivery(
            "player_sessions.create_training_room",
            "created",
            "onPrebattleJoined",
        );
        assert!(has(create, "input.arena_type_id", "83"));
        assert!(has(create, "input.round_length.seconds", "900"));
        assert!(
            !create
                .attributes
                .iter()
                .any(|attr| attr.value.to_string().contains("training-fixture-comment"))
        );
        assert_delivery(
            "player_sessions.create_training_room",
            "already_hosting",
            "onPrebattleJoined",
        );
        let ready = assert_delivery(
            "player_sessions.set_training_ready",
            "ready",
            "updatePrebattle",
        );
        assert!(traces.iter().any(
            |span| span.name == "accounts.training_vehicle_descriptor" && child_of(span, ready)
        ));
        assert_delivery(
            "player_sessions.set_training_ready",
            "vehicle_unavailable",
            "onCmdResponse",
        );
        assert_delivery(
            "player_sessions.leave_training_room",
            "left",
            "onPrebattleLeft",
        );
    }
}
