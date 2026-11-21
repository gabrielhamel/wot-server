use super::{
    Peer, Pickle, ScriptDispatch, Value, ammunition_flow::pickle_value, dict, telemetry_capture,
};
use opentelemetry_sdk::trace::SpanData;
use wot_server::player_sessions::test_support::mercury::script::StringValue;

#[path = "training_chat_flow.rs"]
mod chat;
#[path = "training_invite_flow.rs"]
mod invite;

const ROOM_DESCRIPTION: &str = "Studzianki practice";
const EDITED_DESCRIPTION: &str = "Entraînement en équipe";

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
    chat::assert_open(peer, script, before);
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
        ("destroyIfCreatorOut", Pickle::Bool(true)),
        ("arenaGuiType", Pickle::I64(2)),
        ("bonusType", Pickle::I64(2)),
        (
            "limits",
            wot_server::pickle!({0: {"maxCount": (20, 30), "maxCountTotal": 30}}),
        ),
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
    create(peer, 83, ROOM_DESCRIPTION);
    chat::assert_open(peer, script, before);
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

pub fn exercise_shared(
    host: &mut Peer,
    guest: &mut Peer,
    script: &ScriptDispatch,
    host_account: i64,
    guest_account: i64,
    other_guest: &mut Peer,
    other_account: i64,
) {
    invite::sync(host, script);
    invite::sync(guest, script);
    invite::sync(other_guest, script);
    let (total, entries) = training_list(guest, script, 0, 50);
    assert_eq!(total, 1);
    let [Pickle::Tuple(entry)] = entries.as_slice() else {
        panic!("Expected the host's invitation-only room");
    };
    let [Pickle::F64(_), Pickle::I64(room_id), Pickle::Dict(info)] = entry.as_slice() else {
        panic!("Invalid room list entry: {entry:?}");
    };
    let room_id = *room_id;
    assert_eq!(info[&Pickle::I64(2)], Pickle::Bool(false));
    assert_eq!(info[&Pickle::I64(5)], Pickle::I64(1));
    assert_eq!(info[&Pickle::I64(9)], Pickle::from(ROOM_DESCRIPTION));
    assert_eq!(info[&Pickle::I64(11)], Pickle::I64(83));
    assert_eq!(info[&Pickle::I64(18)], Pickle::I64(host_account));
    assert_eq!(info[&Pickle::I64(20)], Pickle::I64(0));
    assert_eq!(training_list(guest, script, 1, 50), (1, vec![]));

    training_command(guest, script, 202, 400, room_id);
    guest.until(script, |peer| {
        peer.methods
            .iter()
            .any(|(name, args)| name == "onPrebattleJoinFailure" && args == &[Value::UInt8(10)])
    });

    chat::assert_outside_room(guest, script, 4801);

    invite::assert_accept_rejected(guest, script, room_id, 10);
    let guest_before = guest.methods.len();
    let other_before = other_guest.methods.len();
    invite::send(host, script, &[guest_account], &"x".repeat(401));
    invite::send(
        host,
        script,
        &[guest_account, other_account, guest_account],
        invite::MESSAGE,
    );
    invite::assert_received(
        guest,
        script,
        guest_before,
        room_id,
        host_account,
        invite::MESSAGE,
    );
    invite::assert_received(
        other_guest,
        script,
        other_before,
        room_id,
        host_account,
        invite::MESSAGE,
    );
    invite::decline(other_guest, script, room_id);
    invite::assert_accept_rejected(other_guest, script, room_id, 10);

    let before = host.methods.len();
    let guest_before = guest.methods.len();
    invite::accept(guest, script, room_id);
    chat::assert_open(guest, script, guest_before);
    let joined = guest_before + 1;
    assert_eq!(guest.methods[joined].0, "onPrebattleJoined");
    assert_eq!(guest.methods[joined].1[0].as_i64(), Some(room_id));
    host.until(script, |peer| peer.methods.len() > before);
    let roster = |peer: &Peer, index| {
        let (method, args): &(String, Vec<Value>) = &peer.methods[index];
        assert_eq!(method, "updatePrebattle");
        assert_eq!(args[0], Value::UInt8(1));
        let Pickle::List(entries) = pickle_value(&args[1]) else {
            panic!("Expected full roster");
        };
        entries
    };
    let shared = roster(guest, joined + 3);
    let (method, args) = &host.methods[before];
    assert_eq!(method, "updatePrebattle");
    assert_eq!(args[0], Value::UInt8(2));
    assert!(shared.contains(&pickle_value(&args[1])));
    assert_eq!(shared.len(), 2);
    assert!(
        shared
            .iter()
            .any(|entry| matches!(entry, Pickle::Tuple(fields)
        if fields[2] == Pickle::I64(guest_account) && fields[13] == Pickle::I64(0x60)))
    );
    let guest_before = guest.methods.len();
    training_command(host, script, 3310, 408, 1);
    host.until(script, |peer| peer.responses.contains(&(3310, 0)));
    guest.until(script, |peer| peer.methods.len() > guest_before);
    let player_entity = |account: i64| {
        shared
            .iter()
            .find_map(|entry| match entry {
                Pickle::Tuple(fields) if fields[2] == Pickle::I64(account) => match fields[0] {
                    Pickle::I64(id) => Some(id),
                    _ => None,
                },
                _ => None,
            })
            .unwrap()
    };
    let host_entity = player_entity(host_account);
    let guest_entity = player_entity(guest_account);
    for (request_id, entity, destination, host_slot, guest_slot) in [
        (3320, guest_entity, 1, 17, 1),
        (3321, host_entity, 2, 2, 1),
        (3322, host_entity, 1, 1, 1),
        (3323, guest_entity, 2, 1, 2),
        (3324, guest_entity, 18, 1, 18),
    ] {
        let host_before = host.methods.len();
        let guest_before = guest.methods.len();
        host.command(
            script,
            "doCmdInt3",
            vec![
                Value::Int16(request_id),
                Value::Int16(404),
                Value::Int64(entity),
                Value::Int64(destination),
                Value::Int64(0),
            ],
        );
        host.until(script, |peer| peer.responses.contains(&(request_id, 0)));
        guest.until(script, |peer| peer.methods.len() > guest_before);
        let updated = roster(host, host_before);
        assert_eq!(updated, roster(guest, guest_before));
        assert_eq!(updated.len(), 2);
        for (entity, slot) in [(host_entity, host_slot), (guest_entity, guest_slot)] {
            assert!(
                updated
                    .iter()
                    .any(|entry| matches!(entry, Pickle::Tuple(fields)
                if fields[0] == Pickle::I64(entity) && fields[4] == Pickle::I64(slot)))
            );
        }
    }
    let (total, entries) = training_list(guest, script, 0, 50);
    assert_eq!(total, 1);
    let [Pickle::Tuple(entry)] = entries.as_slice() else {
        panic!("Expected the public room");
    };
    let Pickle::Dict(info) = &entry[2] else {
        panic!("Missing room info");
    };
    assert_eq!(info[&Pickle::I64(2)], Pickle::Bool(true));
    assert_eq!(info[&Pickle::I64(5)], Pickle::I64(2));

    let guest_before = guest.methods.len();
    host.command(
        script,
        "doCmdStr",
        vec![
            Value::Int16(3330),
            Value::Int16(409),
            Value::String(StringValue::String(EDITED_DESCRIPTION.into())),
        ],
    );
    host.until(script, |peer| peer.responses.contains(&(3330, 0)));
    guest.until(script, |peer| peer.methods.len() > guest_before);
    let (method, args) = &guest.methods[guest_before];
    assert_eq!(method, "updatePrebattle");
    assert_eq!(args[0], Value::UInt8(8));
    assert_eq!(
        pickle_value(&args[1]),
        wot_server::pickle!(("comment", (EDITED_DESCRIPTION)))
    );
    let (total, entries) = training_list(guest, script, 0, 50);
    assert_eq!(total, 1);
    let [Pickle::Tuple(entry)] = entries.as_slice() else {
        panic!("Expected the updated room");
    };
    let Pickle::Dict(info) = &entry[2] else {
        panic!("Missing updated room info");
    };
    assert_eq!(info[&Pickle::I64(9)], Pickle::from(EDITED_DESCRIPTION));

    training_command(guest, script, 3311, 408, 0);
    guest.until(script, |peer| peer.responses.contains(&(3311, -1)));
    let before = host.methods.len();
    training_command(guest, script, 3312, 402, -5000);
    guest.until(script, |peer| peer.responses.contains(&(3312, 0)));
    host.until(script, |peer| peer.methods.len() > before);
    assert_eq!(host.methods[before].1[0], Value::UInt8(4));
    chat::exchange(host, guest, script, host_account, guest_account);

    let before = host.methods.len();
    let guest_before = guest.methods.len();
    training_command(guest, script, 3313, 401, 0);
    guest.until(script, |peer| peer.responses.contains(&(3313, 0)));
    host.until(script, |peer| peer.methods.len() > before);
    let (method, args) = &host.methods[before];
    assert_eq!(method, "updatePrebattle");
    assert_eq!(args[0], Value::UInt8(3));
    assert_eq!(
        pickle_value(&args[1]),
        wot_server::pickle!(((guest_entity), 18))
    );
    chat::assert_closed(guest, script, guest_before);
    let guest_before = guest.methods.len();
    chat::send_after_guest_leaves(host, script);
    chat::assert_outside_room(guest, script, 4806);
    chat::assert_no_broadcast(guest, guest_before);
    let guest_before = guest.methods.len();
    training_command(guest, script, 202, 400, room_id);
    chat::assert_open(guest, script, guest_before);

    training_command(guest, script, 3331, 414, host_entity);
    guest.until(script, |peer| peer.responses.contains(&(3331, -1)));
    training_command(host, script, 3332, 414, host_entity);
    training_command(host, script, 3333, 414, i64::MAX);
    host.until(script, |peer| peer.responses.contains(&(3333, -1)));
    assert!(host.responses.contains(&(3332, -1)));

    let guest_before = guest.methods.len();
    host.command(
        script,
        "doCmdInt3",
        vec![
            Value::Int16(3334),
            Value::Int16(404),
            Value::Int64(guest_entity),
            Value::Int64(2),
            Value::Int64(0),
        ],
    );
    host.until(script, |peer| peer.responses.contains(&(3334, 0)));
    guest.until(script, |peer| peer.methods.len() > guest_before);
    assert!(
        roster(guest, guest_before)
            .iter()
            .any(|entry| matches!(entry, Pickle::Tuple(fields)
        if fields[0] == Pickle::I64(guest_entity) && fields[4] == Pickle::I64(2)))
    );

    let host_before = host.methods.len();
    let guest_before = guest.methods.len();
    training_command(host, script, 3335, 414, guest_entity);
    host.until(script, |peer| peer.responses.contains(&(3335, 0)));
    let (method, args) = &host.methods[host_before];
    assert_eq!(method, "updatePrebattle");
    assert_eq!(args[0], Value::UInt8(3));
    assert_eq!(
        pickle_value(&args[1]),
        wot_server::pickle!(((guest_entity), 2))
    );
    chat::assert_closed(guest, script, guest_before);
    assert_eq!(
        guest.methods[guest_before],
        ("onKickedFromPrebattle".into(), vec![Value::UInt8(10)])
    );
    chat::assert_outside_room(guest, script, 4808);
    let (total, entries) = training_list(guest, script, 0, 50);
    assert_eq!(total, 1);
    let [Pickle::Tuple(entry)] = entries.as_slice() else {
        panic!("Expected the room to remain available");
    };
    let Pickle::Dict(info) = &entry[2] else {
        panic!("Missing room info");
    };
    assert_eq!(info[&Pickle::I64(5)], Pickle::I64(1));

    let guest_before = guest.methods.len();
    training_command(guest, script, 202, 400, room_id);
    chat::assert_open(guest, script, guest_before);

    let host_before = host.methods.len();
    let guest_before = guest.methods.len();
    training_command(host, script, 3314, 401, 0);
    host.until(script, |peer| peer.responses.contains(&(3314, 0)));
    guest.until(script, |peer| {
        peer.methods[guest_before..]
            .iter()
            .any(|(name, _)| name == "onPrebattleLeft")
    });
    chat::assert_closed(host, script, host_before);
    chat::assert_closed(guest, script, guest_before);
    assert_empty_list(guest, script);
    let guest_before = guest.methods.len();
    training_command(guest, script, 202, 400, room_id);
    guest.until(script, |peer| peer.methods.len() > guest_before);
    assert_eq!(
        guest.methods[guest_before],
        ("onPrebattleJoinFailure".into(), vec![Value::UInt8(2)])
    );

    // Leave a shared room active so the garage test can verify host session replacement.
    let before = host.methods.len();
    host.command(
        script,
        "accountPrebattle_createTraining",
        vec![
            Value::Int32(83),
            Value::Int32(900),
            Value::UInt8(1),
            Value::String(StringValue::String(String::new())),
        ],
    );
    chat::assert_open(host, script, before);
    let id = host.methods[before].1[0].as_i64().unwrap();
    let guest_before = guest.methods.len();
    training_command(guest, script, 202, 400, id);
    chat::assert_open(guest, script, guest_before);
    let other_before = other_guest.methods.len();
    other_guest.command(
        script,
        "accountPrebattle_createTraining",
        vec![
            Value::Int32(83),
            Value::Int32(900),
            Value::UInt8(0),
            Value::String(StringValue::String(String::new())),
        ],
    );
    chat::assert_open(other_guest, script, other_before);
    let other_room = other_guest.methods[other_before].1[0].as_i64().unwrap();
    let guest_before = guest.methods.len();
    invite::send(other_guest, script, &[guest_account], "");
    invite::assert_received(guest, script, guest_before, other_room, other_account, "");
    let guest_before = guest.methods.len();
    training_command(other_guest, script, 3315, 401, 0);
    other_guest.until(script, |peer| peer.responses.contains(&(3315, 0)));
    invite::assert_expired(guest, script, guest_before, other_room);
    invite::assert_accept_rejected(guest, script, other_room, 2);
}

fn training_command(peer: &mut Peer, script: &ScriptDispatch, id: i16, command: i16, value: i64) {
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
}

fn training_list(
    peer: &mut Peer,
    script: &ScriptDispatch,
    start: i32,
    end: i32,
) -> (i64, Vec<Pickle>) {
    peer.clear_resource(200);
    peer.command(
        script,
        "doCmdIntArr",
        vec![
            Value::Int16(200),
            Value::Int16(520),
            Value::Seq(
                [2, 7, 0, start, end]
                    .into_iter()
                    .map(Value::Int32)
                    .collect(),
            ),
        ],
    );
    peer.until(script, |peer| peer.has_resource(200));
    let Pickle::Tuple(result) = peer.resource(200) else {
        panic!("Expected a Training Rooms list");
    };
    let [Pickle::I64(2), Pickle::I64(total), Pickle::List(entries)] = result.as_slice() else {
        panic!("Invalid Training Rooms list: {result:?}");
    };
    (*total, entries.clone())
}

pub fn assert_empty_list(peer: &mut Peer, script: &ScriptDispatch) {
    assert_eq!(training_list(peer, script, 0, 50), (0, vec![]));
}

pub fn assert_room_gone(peer: &mut Peer, script: &ScriptDispatch, vehicle_id: i64) {
    let before = peer.methods.len();
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
        !peer.methods[before..]
            .iter()
            .any(|(name, _)| name == "onPrebattleJoined")
    );
}

pub fn assert_traces(account_id: i64, guest_account_id: i64, available: bool) {
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
    let assert_player_delivery = |player_id: i64, operation, outcome, response| {
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
                                    && has(root, "user.id", &player_id.to_string())
                            })
                    })
            })
            .unwrap_or_else(|| panic!("Missing trace for {operation} {outcome}"));
        assert!(
            traces.iter().any(|child| child.name == "mercury.response"
                && (child_of(child, span)
                    || traces
                        .iter()
                        .any(|resource| resource.name == "mercury.response.resource"
                            && child_of(resource, span)
                            && child_of(child, resource)))
                && has(child, "rpc.method", response)
                && has(child, "output.delivery", "sent")),
            "Missing delivery of {response} for {outcome}"
        );
        span
    };
    let assert_delivery = |operation, outcome, response| {
        assert_player_delivery(account_id, operation, outcome, response)
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
        let sent = traces
            .iter()
            .find(|span| {
                span.name == "player_sessions.send_training_invites"
                    && has(span, "output.outcome", "sent")
                    && has(span, "output.recipients.count", "2")
            })
            .unwrap();
        assert!(has(sent, "input.recipients.count", "3"));
        assert!(has(
            sent,
            "input.string.bytes",
            &invite::MESSAGE.len().to_string()
        ));
        assert!(
            traces
                .iter()
                .any(|command| command.name == "player_sessions.commands.command"
                    && child_of(sent, command)
                    && traces.iter().any(|root| root.name == "game.command"
                        && child_of(command, root)
                        && has(root, "user.id", &account_id.to_string())))
        );
        assert_eq!(
            traces
                .iter()
                .filter(
                    |span| span.name == "player_sessions.publish_training_invite"
                        && child_of(span, sent)
                        && has(span, "input.invite.state", "1")
                        && traces
                            .iter()
                            .any(|response| response.name == "mercury.response"
                                && child_of(response, span)
                                && has(response, "rpc.method", "update")
                                && has(response, "output.delivery", "sent"))
                )
                .count(),
            2
        );
        assert!(
            !traces
                .iter()
                .filter(|span| span.span_context.trace_id() == sent.span_context.trace_id())
                .flat_map(|span| &span.attributes)
                .any(|attr| attr.value.to_string().contains(invite::MESSAGE))
        );
        assert!(
            traces
                .iter()
                .any(|span| span.name == "player_sessions.send_training_invites"
                    && has(span, "output.outcome", "invalid_request")
                    && has(span, "input.string.bytes", "401")
                    && !traces.iter().any(|response| child_of(response, span)))
        );
        assert_player_delivery(
            guest_account_id,
            "player_sessions.join_training_room",
            "invitation_unavailable",
            "onPrebattleJoinFailure",
        );
        let is_chat_delivery = |span: &SpanData| {
            span.name == "mercury.response"
                && has(span, "rpc.method", "messenger_onActionByServer_chat2")
                && has(span, "output.delivery", "sent")
        };
        let opened = traces
            .iter()
            .find(|span| {
                span.name == "player_sessions.training_chat_membership"
                    && child_of(span, create)
                    && has(span, "input.chat.joining", "true")
            })
            .unwrap();
        assert!(
            traces
                .iter()
                .any(|response| child_of(response, opened) && is_chat_delivery(response))
        );
        for (request, outcome, sender, response_count) in [
            ("4803", "broadcast", account_id, 3),
            ("4801", "not_in_room", guest_account_id, 1),
            ("4802", "invalid_message", guest_account_id, 1),
        ] {
            let broadcast = traces
                .iter()
                .find(|span| {
                    span.name == "player_sessions.broadcast_training_chat"
                        && has(span, "input.request_id", request)
                        && has(span, "output.outcome", outcome)
                })
                .unwrap();
            let social = traces
                .iter()
                .find(|span| {
                    span.name == "player_sessions.social.social_command"
                        && child_of(broadcast, span)
                })
                .unwrap();
            let command = traces
                .iter()
                .find(|span| {
                    span.name == "player_sessions.commands.command" && child_of(social, span)
                })
                .unwrap();
            assert!(traces.iter().any(|span| span.name == "game.command"
                && has(span, "user.id", &sender.to_string())
                && child_of(command, span)));
            assert_eq!(
                traces
                    .iter()
                    .filter(|span| child_of(span, broadcast) && is_chat_delivery(span))
                    .count(),
                response_count
            );
            if outcome == "broadcast" {
                assert!(has(broadcast, "output.recipients.count", "2"));
                assert!(has(
                    broadcast,
                    "input.text.bytes",
                    &chat::HOST_TEXT.len().to_string()
                ));
                assert!(
                    !broadcast
                        .attributes
                        .iter()
                        .chain(&social.attributes)
                        .any(|attr| attr.value.to_string().contains(chat::HOST_TEXT))
                );
            }
        }
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
            "deleted",
            "onPrebattleLeft",
        );
        let joined = assert_player_delivery(
            guest_account_id,
            "player_sessions.join_training_room",
            "joined",
            "onPrebattleJoined",
        );
        assert!(has(joined, "output.players.count", "2"));
        for outcome in ["invitation_only", "room_unavailable"] {
            assert_player_delivery(
                guest_account_id,
                "player_sessions.join_training_room",
                outcome,
                "onPrebattleJoinFailure",
            );
        }
        assert_player_delivery(
            guest_account_id,
            "player_sessions.change_training_setting",
            "host_required",
            "onCmdResponse",
        );
        assert_player_delivery(
            guest_account_id,
            "player_sessions.leave_training_room",
            "left",
            "onPrebattleLeft",
        );
        let listed = assert_player_delivery(
            guest_account_id,
            "player_sessions.list_training_rooms",
            "listed",
            "onCmdResponse",
        );
        assert!(has(listed, "output.rooms.count", "1"));
        assert!(has(listed, "output.rooms.total", "1"));
        assert!(
            traces
                .iter()
                .any(|span| span.name == "mercury.response.resource"
                    && child_of(span, listed)
                    && traces.iter().any(|child| child.name == "mercury.response"
                        && child_of(child, span)
                        && has(child, "output.delivery", "sent")))
        );
        let kicked = assert_delivery(
            "player_sessions.kick_training_player",
            "kicked",
            "onKickedFromPrebattle",
        );
        assert!(has(kicked, "input.request_id", "3335"));
        assert!(traces.iter().any(|span| span.name == "mercury.response"
            && child_of(span, kicked)
            && has(span, "rpc.method", "onCmdResponse")
            && has(span, "output.response_code", "0")
            && has(span, "output.delivery", "sent")));
        let removal = traces
            .iter()
            .find(|span| {
                span.name == "player_sessions.remove_training_member"
                    && child_of(span, kicked)
                    && has(span, "output.players.count", "1")
            })
            .unwrap();
        assert!(traces.iter().any(|span| span.name == "mercury.response"
            && child_of(span, removal)
            && has(span, "rpc.method", "updatePrebattle")
            && has(span, "output.delivery", "sent")));
        let chat_closed = traces
            .iter()
            .find(|span| {
                span.name == "player_sessions.training_chat_membership"
                    && child_of(span, kicked)
                    && has(span, "input.chat.joining", "false")
            })
            .unwrap();
        assert!(
            traces
                .iter()
                .any(|span| child_of(span, chat_closed) && is_chat_delivery(span))
        );
        assert_player_delivery(
            guest_account_id,
            "player_sessions.kick_training_player",
            "host_required",
            "onCmdResponse",
        );
        assert_delivery(
            "player_sessions.kick_training_player",
            "invalid_target",
            "onCmdResponse",
        );
        let deleted = traces
            .iter()
            .find(|span| {
                span.name == "player_sessions.leave_training_room"
                    && has(span, "input.request_id", "3314")
            })
            .unwrap();
        let cleanup = traces
            .iter()
            .find(|span| {
                span.name == "player_sessions.remove_training_member" && child_of(span, deleted)
            })
            .unwrap();
        assert!(has(cleanup, "output.outcome", "deleted"));
        let closed = traces
            .iter()
            .find(|span| {
                span.name == "player_sessions.training_chat_membership"
                    && child_of(span, cleanup)
                    && has(span, "input.chat.joining", "false")
            })
            .unwrap();
        assert!(
            traces
                .iter()
                .any(|response| child_of(response, closed) && is_chat_delivery(response))
        );
        assert!(has(cleanup, "output.players.count", "0"));
        assert!(traces.iter().any(|span| span.name == "mercury.response"
            && child_of(span, cleanup)
            && has(span, "rpc.method", "onPrebattleLeft")
            && has(span, "output.delivery", "sent")));
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
        for (request_id, operation, outcome) in [
            ("3291", "player_sessions.set_training_ready", "ready"),
            ("3322", "player_sessions.assign_training_roster", "assigned"),
            ("3330", "player_sessions.change_training_setting", "changed"),
        ] {
            let operation = traces
                .iter()
                .find(|span| {
                    span.name == operation
                        && has(span, "input.request_id", request_id)
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
                && child_of(span, operation)
                && has(span, "rpc.method", "updatePrebattle")
                && has(span, "output.delivery", "sent")));
        }
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

pub fn exercise_battle(
    host: &mut Peer,
    host_vehicle: i64,
    guest: &mut Peer,
    guest_vehicle: i64,
    script: &ScriptDispatch,
) {
    let command = |peer: &mut Peer, request, command, args: [i64; 3]| {
        let mut values = vec![Value::Int16(request), Value::Int16(command)];
        values.extend(args.map(Value::Int64));
        peer.command(script, "doCmdInt3", values);
    };
    let first_host_message = host.methods.len();
    host.command(
        script,
        "accountPrebattle_createTraining",
        vec![
            Value::Int32(83),
            Value::Int32(300),
            Value::UInt8(1),
            Value::String(StringValue::String(String::new())),
        ],
    );
    host.until(script, |p| {
        p.methods[first_host_message..]
            .iter()
            .any(|(name, _)| name == "onPrebattleJoined")
    });
    let room_id = host.methods[first_host_message..]
        .iter()
        .find(|(name, _)| name == "onPrebattleJoined")
        .unwrap()
        .1[0]
        .as_i64()
        .unwrap();
    let first_guest_message = guest.methods.len();
    command(guest, 202, 400, [room_id, 0, 0]);
    guest.until(script, |p| {
        p.methods[first_guest_message..]
            .iter()
            .any(|(name, _)| name == "onPrebattleJoined")
    });

    command(guest, 5100, 411, [1, 0, 0]);
    guest.until(script, |p| p.responses.contains(&(5100, -1)));
    command(host, 5101, 411, [1, 0, 0]);
    host.until(script, |p| p.responses.contains(&(5101, -1)));
    command(host, 5102, 404, [i64::from(host.player_entity_id), 1, 0]);
    command(host, 5103, 404, [i64::from(guest.player_entity_id), 2, 0]);
    command(host, 5104, 402, [host_vehicle, 0, 0]);
    command(guest, 5105, 402, [guest_vehicle, 0, 0]);
    host.until(script, |p| {
        [5102, 5103, 5104]
            .into_iter()
            .all(|id| p.responses.contains(&(id, 0)))
    });
    guest.until(script, |p| p.responses.contains(&(5105, 0)));

    let before_confirmations = host.methods.len();
    command(host, 5106, 411, [1, 0, 0]);
    host.until(script, |p| p.responses.contains(&(5106, 0)));
    assert_eq!(host.resets, 0);
    assert!(
        !host.methods[before_confirmations..]
            .iter()
            .any(|(name, args)| name == "updatePrebattle" && args[0].as_i64() == Some(6)),
        "A partial team-state update would suppress the second native command"
    );
    command(host, 5107, 412, [1, 0, 0]);
    host.until(script, |p| p.responses.contains(&(5107, 0)));
    command(host, 5108, 411, [1, 0, 0]);
    host.until(script, |p| p.responses.contains(&(5108, 0)));
    command(host, 5109, 411, [2, 0, 0]);
    host.until(script, |p| p.resets == 1);
    guest.until(script, |p| p.resets == 1);
    assert!(host.responses.contains(&(5109, 0)));
    assert!(
        host.methods[before_confirmations..]
            .iter()
            .any(|(name, args)| name == "updatePrebattle"
                && args[0].as_i64() == Some(6)
                && pickle_value(&args[1]) == wot_server::pickle!((2, 2)))
    );
    for (peer, before) in [(&*host, first_host_message), (&*guest, first_guest_message)] {
        assert!(
            peer.methods[before..]
                .iter()
                .any(|(name, _)| name == "onArenaCreated")
        );
    }
    host.enable_entities();
    guest.enable_entities();
    host.until(script, |p| p.arena_roster.is_some());
    guest.until(script, |p| p.arena_roster.is_some());

    let host_cell = host.cell.as_ref().unwrap();
    let guest_cell = guest.cell.as_ref().unwrap();
    assert_eq!(host.entered_arena, guest.entered_arena);
    assert_eq!(
        host.entered_arena,
        Some((u64::from(host_cell.space_id), 83))
    );
    let geometry = host.geometry.as_ref().unwrap();
    assert_eq!(geometry.space_id, guest_cell.space_id);
    assert_eq!(geometry.path, guest.geometry.as_ref().unwrap().path);
    assert_ne!(host_cell.vehicle_id, guest_cell.vehicle_id);
    assert_ne!(host_cell.position, guest_cell.position);
    let avatar_schema = script.entity_from_name("Avatar").unwrap().1;
    let vehicle_type = script.entity_from_name("Vehicle").unwrap().0;
    for (peer, cell, team) in [(&*host, host_cell, 1), (&*guest, guest_cell, 2)] {
        use wot_server::player_sessions::test_support::mercury::net::codec::Codec;
        let mut blob = cell.properties.as_slice();
        let Value::Dict(properties) = Value::read(&mut blob, &avatar_schema.cell_data_ty).unwrap()
        else {
            panic!("avatar cell properties");
        };
        assert_eq!(properties["team"].as_i64(), Some(team));
        assert_eq!(
            properties["playerVehicleID"].as_i64(),
            Some(i64::from(cell.vehicle_id))
        );
        assert_eq!(blob[0], avatar_schema.static_components_count);
        Value::read(&mut &blob[1..], &avatar_schema.cell_components_data_ty).unwrap();
        let ids = peer
            .arena_entities
            .iter()
            .filter(|entity| entity.type_id == vehicle_type)
            .map(|entity| entity.entity_id)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            ids,
            [host_cell.vehicle_id, guest_cell.vehicle_id]
                .into_iter()
                .collect()
        );
    }

    let period = |p: &Peer| {
        p.methods
            .iter()
            .any(|(name, args)| name == "updateArena" && args[0].as_i64() == Some(3))
    };
    host.command(script, "setClientReady", vec![]);
    guest.command(script, "setClientReady", vec![]);
    host.until(script, period);
    guest.until(script, period);
    let (first, second) = (
        host.cell.as_ref().unwrap().vehicle_id,
        guest.cell.as_ref().unwrap().vehicle_id,
    );
    guest.cell_command(script, "vehicle_shoot", vec![]);
    host.cell_command(script, "autoAim", vec![Value::Int32(0), Value::UInt8(0)]);
    let in_battle = |p: &Peer| {
        p.methods.iter().any(|(name, args)| {
        if name != "updateArena" || args[0].as_i64() != Some(3) { return false; }
        let Value::String(StringValue::Raw(data)) = &args[1] else { return false; };
        matches!(wot_server::platform::client_formats::pickle::from_reader(flate2::read::ZlibDecoder::new(data.as_slice())),
            Ok(Pickle::Tuple(fields)) if fields.first() == Some(&Pickle::I64(3)))
    })
    };
    host.until(script, in_battle);
    guest.until(script, in_battle);
    host.cell_command(script, "vehicle_moveWith", vec![Value::UInt8(0x80)]);
    host.cell_command(script, "vehicle_moveWith", vec![Value::UInt8(1)]);
    guest.cell_command(script, "vehicle_moveWith", vec![Value::UInt8(1)]);
    for peer in [&mut *host, &mut *guest] {
        peer.until(script, |p| {
            p.positions.contains_key(&first) && p.positions.contains_key(&second)
        });
        assert_ne!(
            peer.positions[&first],
            peer.cell
                .as_ref()
                .filter(|cell| cell.vehicle_id == first)
                .map_or_else(
                    || peer
                        .arena_entities
                        .iter()
                        .find(|e| e.entity_id == first)
                        .unwrap()
                        .position,
                    |cell| cell.position
                )
        );
    }
    host.cell_command(script, "vehicle_moveWith", vec![Value::UInt8(64)]);
    guest.cell_command(script, "vehicle_moveWith", vec![Value::UInt8(64)]);
    host.vehicle_command(
        script,
        second,
        "trackRelativePointWithGun",
        vec![Value::Vector3(glam::Vec3::Y)],
    );
    let offset = host.positions[&second] - host.positions[&first] + glam::Vec3::Y * 1.35;
    host.vehicle_command(
        script,
        first,
        "trackRelativePointWithGun",
        vec![Value::Vector3(offset)],
    );
    host.cell_command(script, "vehicle_shoot", vec![]);
    host.cell_command(script, "vehicle_shoot", vec![]);
    let received_shot = |p: &Peer| {
        p.methods.iter().any(|(name, _)| name == "showTracer")
            && p.vehicle_methods.iter().any(|(target, name, args)| {
                *target == second
                    && name == "onHealthChanged"
                    && args[0].as_i64() < args[1].as_i64()
            })
    };
    host.until(script, received_shot);
    guest.until(script, received_shot);
    assert!(
        guest
            .vehicle_methods
            .iter()
            .any(|(target, name, _)| *target == first && name == "showShooting")
    );
    guest.command(script, "leaveArena", vec![]);
    guest.until(script, |p| p.resets == 2);
    host.until(script, |p| p.resets == 2);
    host.enable_entities();
    guest.enable_entities();
    host.until(script, |p| p.created && p.shown);
    guest.until(script, |p| p.created && p.shown);
    assert_empty_list(host, script);
}

pub fn assert_battle_traces(host: i64, guest: i64) {
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
    let unimplemented = traces
        .iter()
        .find(|s| s.name == "game.cell_command" && has(s, "output.outcome", "unimplemented"))
        .expect("unsupported arena cell operation");
    assert!(matches!(
        unimplemented.status,
        opentelemetry::trace::Status::Error { .. }
    ));
    assert!(
        unimplemented
            .events
            .iter()
            .any(|event| event.name == "Arena cell method is not implemented")
    );

    let hit = traces
        .iter()
        .find(|s| s.name == "player_sessions.training_shot" && has(s, "output.outcome", "hit"))
        .expect("training hit");
    let control = traces
        .iter()
        .find(|s| s.name == "player_sessions.training_control" && child_of(hit, s))
        .unwrap();
    assert!(
        traces
            .iter()
            .any(|s| s.name == "game.cell_command" && child_of(control, s))
    );
    assert_eq!(
        traces
            .iter()
            .filter(|s| s.name == "mercury.arena_vehicle_method"
                && child_of(s, hit)
                && has(s, "rpc.method", "onHealthChanged")
                && has(s, "output.delivery", "sent"))
            .count(),
        2
    );
    let movement = traces
        .iter()
        .find(|s| {
            s.name == "player_sessions.training_replication"
                && has(s, "input.replication.kind", "position")
        })
        .expect("movement replication");
    assert!(
        traces
            .iter()
            .any(|s| s.name == "player_sessions.training_control"
                && child_of(movement, s)
                && has(s, "input.drive.flags", "1"))
    );
    for outcome in ["reloading", "invalid_flags", "entity_unavailable"] {
        assert!(
            traces
                .iter()
                .any(|s| s.name == "game.cell_command" && has(s, "output.outcome", outcome)),
            "missing {outcome}"
        );
    }

    let countdown = traces
        .iter()
        .find(|s| {
            s.name == "player_sessions.training_battle_period"
                && has(s, "output.phase", "countdown")
        })
        .expect("countdown broadcast");
    assert!(traces.iter().any(|s| s.name == "game.command"
        && child_of(countdown, s)
        && has(s, "rpc.method", "setClientReady")));
    assert_eq!(
        traces
            .iter()
            .filter(|s| s.name == "mercury.response"
                && child_of(s, countdown)
                && has(s, "rpc.method", "updateArena")
                && has(s, "output.delivery", "sent"))
            .count(),
        2
    );

    for (request, account, outcome, code) in [
        ("5100", guest, "not_host", "-1"),
        ("5101", host, "team_not_ready", "-1"),
        ("5109", host, "launched", "0"),
    ] {
        let root = traces
            .iter()
            .find(|s| {
                s.name == "game.command"
                    && has(s, "input.request_id", request)
                    && has(s, "user.id", &account.to_string())
            })
            .expect("training launch request");
        let command = traces
            .iter()
            .find(|s| s.name == "player_sessions.commands.command" && child_of(s, root))
            .unwrap();
        let launch = traces
            .iter()
            .find(|s| s.name == "player_sessions.start_training_battle" && child_of(s, command))
            .unwrap();
        assert!(has(launch, "output.outcome", outcome));
        assert!(traces.iter().any(|s| s.name == "mercury.response"
            && child_of(s, launch)
            && has(s, "input.request_id", request)
            && has(s, "output.response_code", code)
            && has(s, "output.delivery", "sent")));
        if request == "5109" {
            let prepare = traces
                .iter()
                .find(|s| {
                    s.name == "player_sessions.prepare_training_battle" && child_of(s, launch)
                })
                .unwrap();
            let entries = traces
                .iter()
                .filter(|s| {
                    s.name == "player_sessions.enter_training_battle" && child_of(s, prepare)
                })
                .collect::<Vec<_>>();
            assert_eq!(entries.len(), 2);
            for entry in entries {
                assert!(has(entry, "output.outcome", "entered"));
                assert!(traces.iter().any(|s| s.name == "mercury.arena_delivery"
                    && child_of(s, entry)
                    && has(s, "input.element_id", "6")
                    && has(s, "output.delivery", "sent")));
            }
        }
    }
}
