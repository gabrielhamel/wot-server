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
    let mut settings = pickle_value(&messages[1].1[1]);
    for (key, value) in [
        ("type", Pickle::I64(2)),
        ("arenaGuiType", Pickle::I64(2)),
        ("arenaTypeID", Pickle::I64(83)),
        ("roundLength", Pickle::I64(900)),
        ("arenaVoipChannels", Pickle::I64(0)),
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
    let Pickle::I64(player_entity) = host[0] else {
        panic!("Missing player entity ID")
    };
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

    let assign = |peer: &mut Peer, id, target, slot, group, result| {
        peer.command(
            script,
            "doCmdInt3",
            vec![
                Value::Int16(id),
                Value::Int16(404),
                Value::Int64(target),
                Value::Int64(slot),
                Value::Int64(group),
            ],
        );
        peer.until(script, |peer| peer.responses.contains(&(id, result)));
    };
    let assert_move = |peer: &Peer, before, next, vehicle: &Pickle| {
        let [(method, args), (ack, _)] = &peer.methods[before..] else {
            panic!("Expected roster update followed by acknowledgement")
        };
        assert_eq!(method, "updatePrebattle");
        assert_eq!(args[0], Value::UInt8(1));
        let Pickle::List(rows) = pickle_value(&args[1]) else {
            panic!("Missing roster snapshot")
        };
        let [Pickle::Tuple(player)] = rows.as_slice() else {
            panic!("Expected exactly one roster entry")
        };
        assert_eq!(player[0], Pickle::I64(player_entity));
        assert_eq!(player[2], Pickle::I64(account_id));
        assert_eq!(player[4], Pickle::I64(next));
        assert_eq!(
            player[5],
            Pickle::I64(if *vehicle == Pickle::I64(0) { 1 } else { 4 })
        );
        assert_eq!(&player[7], vehicle);
        assert_eq!(player[13], Pickle::I64(0xfe7f));
        assert_eq!(ack, "onCmdResponse");
    };
    let before = peer.methods.len();
    assign(peer, 3260, player_entity + 1, 1, 0, -1);
    assign(peer, 3261, player_entity, 16, 0, -1);
    assign(peer, 3262, player_entity, 1, 1, -1);
    assert!(
        peer.methods[before..]
            .iter()
            .all(|(method, _)| method == "onCmdResponse")
    );
    let before = peer.methods.len();
    assign(peer, 3263, player_entity, 1, 0, 0);
    assert_move(peer, before, 1, &ready[3]);
    let before = peer.methods.len();
    assign(peer, 3264, player_entity, 1, 0, 0);
    assert_move(peer, before, 1, &ready[3]);

    exercise_settings(peer, script, &mut settings);

    let assert_ready_vehicle = |peer: &Peer, before, expected: Option<&Pickle>| {
        let [(update, args), (response, _)] = &peer.methods[before..] else {
            panic!("Expected player-state update followed by acknowledgement")
        };
        assert_eq!(update, "updatePrebattle");
        assert_eq!(args[0], Value::UInt8(4));
        let Pickle::Tuple(state) = pickle_value(&args[1]) else {
            panic!("Missing player state")
        };
        assert_eq!(
            &state[..3],
            &[Pickle::I64(player_entity), Pickle::I64(1), Pickle::I64(4)]
        );
        assert_eq!(response, "onCmdResponse");
        if let Some(expected) = expected {
            assert_eq!(&state[3], expected);
        } else {
            assert!(
                matches!(&state[3], Pickle::Bytes(descriptor) if descriptor.starts_with(&[1, 255]))
            );
        }
        state[3].clone()
    };
    let before = peer.methods.len();
    request(peer, 3290, 402, -5000);
    peer.until(script, |peer| {
        peer.responses.iter().any(|(id, _)| *id == 3290)
    });
    assert!(
        peer.responses.contains(&(3290, 0)),
        "Observer response was {:?}",
        peer.methods.last()
    );
    let observer = assert_ready_vehicle(peer, before, None);

    let before = peer.methods.len();
    create(peer, 83, "duplicate");
    peer.until(script, |peer| peer.methods.len() >= before + 4);
    assert_eq!(peer.methods[before].0, "onPrebattleJoined");
    assert_eq!(peer.methods[before].1[0].as_i64(), Some(room_id));
    assert_eq!(pickle_value(&peer.methods[before + 1].1[1]), settings);
    let Pickle::List(roster) = pickle_value(&peer.methods[before + 3].1[1]) else {
        panic!("Missing replayed roster")
    };
    let [Pickle::Tuple(host)] = roster.as_slice() else {
        panic!("Expected one host")
    };
    assert_eq!(host[4], Pickle::I64(1));
    assert_eq!(host[5], Pickle::I64(4));
    assert_eq!(host[7], observer);

    for (id, slot) in [(3292, 2), (3293, 1)] {
        let before = peer.methods.len();
        assign(peer, id, player_entity, slot, 0, 0);
        assert_move(peer, before, slot, &observer);
    }
    let before = peer.methods.len();
    request(peer, 3294, 405, 1);
    peer.until(script, |peer| peer.responses.contains(&(3294, -1)));
    assert_eq!(peer.methods.len(), before + 1);
    for (id, slot) in [(3295, 2), (3296, 1)] {
        let before = peer.methods.len();
        request(peer, id, 405, 0);
        peer.until(script, |peer| peer.responses.contains(&(id, 0)));
        assert_move(peer, before, slot, &observer);
    }

    let before = peer.methods.len();
    request(peer, 3291, 402, vehicle_id);
    peer.until(script, |peer| peer.responses.contains(&(3291, 0)));
    assert_ready_vehicle(peer, before, Some(&ready[3]));

    let before = peer.methods.len();
    assign(peer, 3265, player_entity, 2, 0, 0);
    assert_move(peer, before, 2, &ready[3]);

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
    assert_eq!(not_ready[1], Pickle::I64(2));
    assert_eq!(not_ready[3], Pickle::I64(0));

    let before = peer.methods.len();
    assign(peer, 3266, player_entity, 18, 0, 0);
    assert_move(peer, before, 18, &Pickle::I64(0));
    let before = peer.methods.len();
    request(peer, 3297, 405, 0);
    peer.until(script, |peer| peer.responses.contains(&(3297, 0)));
    assert_move(peer, before, 18, &Pickle::I64(0));

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

fn exercise_settings(peer: &mut Peer, script: &ScriptDispatch, settings: &mut Pickle) {
    let change = |peer: &mut Peer, id, command, value| {
        peer.command(
            script,
            "doCmdInt3",
            vec![
                Value::Int16(id),
                Value::Int16(command),
                Value::Int64(value),
                Value::Int64(0),
                Value::Int64(0),
            ],
        );
    };
    let comment = |peer: &mut Peer, id, value: String| {
        peer.command(
            script,
            "doCmdStr",
            vec![
                Value::Int16(id),
                Value::Int16(409),
                Value::String(StringValue::String(value)),
            ],
        );
    };
    let before = peer.methods.len();
    change(peer, 3270, 406, i64::MAX);
    change(peer, 3271, 407, 901);
    change(peer, 3272, 408, 2);
    comment(peer, 3273, "x".repeat(401));
    peer.until(script, |peer| {
        (3270..=3273).all(|id| peer.responses.contains(&(id, -1)))
    });
    assert!(
        peer.methods[before..]
            .iter()
            .all(|(method, _)| method == "onCmdResponse")
    );

    let before = peer.methods.len();
    change(peer, 3274, 408, 1);
    comment(peer, 3275, "soirée training fixture".into());
    change(peer, 3276, 406, 1);
    change(peer, 3277, 407, 660);
    peer.until(script, |peer| {
        (3274..=3277).all(|id| peer.responses.contains(&(id, 0)))
    });
    let expected = [
        ("isOpened", Pickle::Bool(true)),
        ("comment", Pickle::from("soirée training fixture")),
        ("arenaTypeID", Pickle::I64(1)),
        ("roundLength", Pickle::I64(660)),
    ];
    let (message_pairs, remaining) = peer.methods[before..].as_chunks::<2>();
    assert_eq!(message_pairs.len(), expected.len());
    assert!(remaining.is_empty());
    let Pickle::Dict(settings) = settings else {
        panic!("Missing room settings dictionary")
    };
    for ([(update, args), (response, _)], (key, value)) in message_pairs.iter().zip(expected) {
        assert_eq!(update, "updatePrebattle");
        assert_eq!(args[0], Value::UInt8(8));
        assert_eq!(
            pickle_value(&args[1]),
            Pickle::Tuple(vec![Pickle::from(key), value.clone()])
        );
        assert_eq!(response, "onCmdResponse");
        settings.insert(Pickle::from(key), value);
    }

    let before = peer.methods.len();
    change(peer, 3278, 407, 660);
    peer.until(script, |peer| peer.responses.contains(&(3278, 0)));
    assert_eq!(peer.methods.len(), before + 1);

    let before = peer.methods.len();
    change(peer, 3280, 410, -1);
    change(peer, 3281, 410, 2);
    change(peer, 3282, 410, 0);
    peer.until(script, |peer| {
        [(3280, -1), (3281, -1), (3282, 0)]
            .iter()
            .all(|response| peer.responses.contains(response))
    });
    assert_eq!(peer.methods.len(), before + 3);
    assert!(
        peer.methods[before..]
            .iter()
            .all(|(method, _)| method == "onCmdResponse")
    );

    for (request_id, index) in [(3283, 1_i64), (3284, 0), (3285, 1)] {
        let before = peer.methods.len();
        change(peer, request_id, 410, index);
        peer.until(script, |peer| peer.responses.contains(&(request_id, 0)));
        let [(update, args), (response, _)] = &peer.methods[before..] else {
            panic!("Expected voice mode update followed by acknowledgement")
        };
        assert_eq!(update, "updatePrebattle");
        assert_eq!(args[0], Value::UInt8(8));
        assert_eq!(
            pickle_value(&args[1]),
            wot_server::pickle!(("arenaVoipChannels", (index)))
        );
        assert_eq!(response, "onCmdResponse");
    }
    settings.insert(Pickle::from("arenaVoipChannels"), Pickle::I64(1));
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
        let observer = assert_delivery(
            "player_sessions.set_training_ready",
            "observer",
            "updatePrebattle",
        );
        assert!(has(observer, "input.inventory_id", "-5000"));
        for method in ["accounts.training_observer_descriptor", "mercury.response"] {
            assert!(traces.iter().any(|span| span.name == method
                && child_of(span, observer)
                && (method != "mercury.response"
                    || (has(span, "rpc.method", "onCmdResponse")
                        && has(span, "output.delivery", "sent")))));
        }
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
        let assigned = assert_delivery(
            "player_sessions.assign_training_roster",
            "assigned",
            "updatePrebattle",
        );
        assert!(has(assigned, "input.roster.slot", "1"));
        assert!(has(assigned, "output.roster.previous_slot", "17"));
        assert!(traces.iter().any(|span| span.name == "mercury.response"
            && child_of(span, assigned)
            && has(span, "rpc.method", "onCmdResponse")
            && has(span, "output.delivery", "sent")));
        assert_delivery(
            "player_sessions.assign_training_roster",
            "invalid_request",
            "onCmdResponse",
        );
        assert_delivery(
            "player_sessions.assign_training_roster",
            "unchanged",
            "updatePrebattle",
        );
        let swapped = assert_delivery(
            "player_sessions.swap_training_teams",
            "swapped",
            "updatePrebattle",
        );
        assert!(has(swapped, "output.roster.previous_slot", "1"));
        assert!(has(swapped, "output.roster.slot", "2"));
        assert!(traces.iter().any(|span| span.name == "mercury.response"
            && child_of(span, swapped)
            && has(span, "rpc.method", "onCmdResponse")
            && has(span, "output.delivery", "sent")));
        assert_delivery(
            "player_sessions.swap_training_teams",
            "invalid_request",
            "onCmdResponse",
        );
        assert_delivery(
            "player_sessions.swap_training_teams",
            "unchanged",
            "updatePrebattle",
        );
        let changed = assert_delivery(
            "player_sessions.change_training_setting",
            "changed",
            "updatePrebattle",
        );
        assert!(has(changed, "input.command_id", "408"));
        assert!(traces.iter().any(|span| span.name == "mercury.response"
            && child_of(span, changed)
            && has(span, "rpc.method", "onCmdResponse")
            && has(span, "output.delivery", "sent")));
        assert_delivery(
            "player_sessions.change_training_setting",
            "invalid_request",
            "onCmdResponse",
        );
        assert_delivery(
            "player_sessions.change_training_setting",
            "unchanged",
            "onCmdResponse",
        );
        for (id, outcome, response_code) in [
            ("3281", "invalid_request", "-1"),
            ("3282", "unchanged", "0"),
            ("3283", "changed", "0"),
        ] {
            let voice = traces
                .iter()
                .find(|span| {
                    span.name == "player_sessions.change_training_setting"
                        && has(span, "input.command_id", "410")
                        && has(span, "input.request_id", id)
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
                .unwrap();
            assert!(traces.iter().any(|span| span.name == "mercury.response"
                && child_of(span, voice)
                && has(span, "rpc.method", "onCmdResponse")
                && has(span, "output.response_code", response_code)
                && has(span, "output.delivery", "sent")));
            assert_eq!(
                traces.iter().any(|span| span.name == "mercury.response"
                    && child_of(span, voice)
                    && has(span, "rpc.method", "updatePrebattle")
                    && has(span, "output.delivery", "sent")),
                outcome == "changed"
            );
        }
    }
}
