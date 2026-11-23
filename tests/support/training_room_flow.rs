use super::{
    Peer, Pickle, ScriptDispatch, Value, ammunition_flow::pickle_value, dict, telemetry_capture,
};
use opentelemetry_sdk::trace::SpanData;
use std::time::Duration;
use wot_platform::mercury::script::StringValue;

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
            wot_platform::pickle!({0: {"maxCount": (20, 30), "maxCountTotal": 30}}),
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
            wot_platform::pickle!(("arenaVoipChannels", (index)))
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
        wot_platform::pickle!(("comment", (EDITED_DESCRIPTION)))
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
        wot_platform::pickle!(((guest_entity), 18))
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
        wot_platform::pickle!(((guest_entity), 2))
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
    let by_id: std::collections::HashMap<_, _> = traces
        .iter()
        .map(|span| {
            (
                (span.span_context.trace_id(), span.span_context.span_id()),
                span,
            )
        })
        .collect();
    let parent_of = |span: &SpanData| {
        by_id
            .get(&(span.span_context.trace_id(), span.parent_span_id))
            .copied()
    };
    let assert_player_delivery = |player_id: i64, operation, outcome, response| {
        let player_id = player_id.to_string();
        let span = traces
            .iter()
            .find(|span| {
                span.name == operation
                    && has(span, "output.outcome", outcome)
                    && parent_of(span).is_some_and(|command| {
                        command.name == "player_sessions.commands.command"
                            && parent_of(command).is_some_and(|root| {
                                root.name == "game.command" && has(root, "user.id", &player_id)
                            })
                    })
            })
            .unwrap_or_else(|| panic!("Missing trace for {operation} {outcome}"));
        assert!(
            traces.iter().any(|child| child.name == "mercury.response"
                && (child_of(child, span)
                    || parent_of(child)
                        .is_some_and(|resource| resource.name == "mercury.response.resource"
                            && child_of(resource, span)))
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

#[derive(Clone, Copy)]
pub struct BattleFixture {
    shell_damage: u16,
    shell_range: f32,
    shell_speed: f32,
    has_obstacles: bool,
    has_damage_stickers: bool,
    has_vehicle_bounds: bool,
}

pub fn battle_fixture(resources: &wot_resources::ResourceBundle) -> BattleFixture {
    use wot_platform::client_formats::pxml;

    let shells = resources
        .read("scripts/item_defs/vehicles/usa/components/shells.xml")
        .unwrap();
    let shells = pxml::from_reader(shells).unwrap();
    let damage = shells
        .get_child_value_at(&["_37mm_AP_1916_1937_T1", "damage", "armor"])
        .unwrap();
    let guns = resources
        .read("scripts/item_defs/vehicles/usa/components/guns.xml")
        .unwrap();
    let guns = pxml::from_reader(guns).unwrap();
    let shot = [
        "shared",
        "_37mm_Gun_M1916",
        "shots",
        "_37mm_AP_1916_1937_T1",
    ]
    .into_iter()
    .try_fold(guns.as_ref(), |element, name| {
        element.get_child_element(name)
    })
    .unwrap();
    let common = pxml::from_reader(
        resources
            .read("scripts/item_defs/vehicles/common/vehicle_common.xml")
            .unwrap(),
    )
    .unwrap();
    let speed_factor = common
        .get_child_value_at(&["miscParams", "projectileSpeedFactor"])
        .and_then(|value| value.as_float())
        .unwrap();
    BattleFixture {
        shell_damage: damage
            .as_float()
            .or_else(|| damage.as_vec2().map(|values| values.x))
            .unwrap()
            .round() as u16,
        shell_range: shot.get_child_float("maxDistance").unwrap(),
        shell_speed: shot.get_child_float("speed").unwrap() * speed_factor,
        has_obstacles: resources.has_file("spaces/07_lakeville/collision.bin"),
        has_vehicle_bounds: resources
            .has_file("vehicles/american/A01_T1_Cunningham/collision_client/Chassis.bounds"),
        has_damage_stickers: resources
            .has_file("scripts/item_defs/vehicles/common/damage_stickers.xml"),
    }
}

// Lakeville Encounter has a clear firing line along the lakeside road.
const BATTLE_ARENA_TYPE: i32 = (1 << 16) | 5;
pub const BATTLE_TERRAIN: &str = "spaces/07_lakeville/terrain";

pub fn join_during_battle(peer: &mut Peer, script: &ScriptDispatch, room_id: i64, host_id: i64) {
    invite::sync(peer, script);
    invite::assert_received(peer, script, 0, room_id, host_id, invite::MESSAGE);
    let (total, rooms) = training_list(peer, script, 0, 50);
    assert_eq!(total, 1);
    assert!(matches!(&rooms[..], [Pickle::Tuple(room)] if room[1] == Pickle::I64(room_id)));
    for (request, join) in [(5390, 417), (5391, 400)] {
        let before = peer.methods.len();
        if join == 417 {
            invite::accept(peer, script, room_id);
        } else {
            training_command(peer, script, 202, join, room_id);
        }
        peer.until(script, |p| {
            p.methods[before..].iter().any(|(name, args)| {
                if name != "updatePrebattle" || args[0] != Value::UInt8(1) {
                    return false;
                }
                matches!(pickle_value(&args[1]), Pickle::List(roster)
                    if roster.len() == 3 && roster.iter().filter(|entry|
                        matches!(entry, Pickle::Tuple(fields) if fields[5] == Pickle::I64(8))
                    ).count() == 2)
            })
        });
        assert_eq!(peer.resets, 0);
        training_command(peer, script, request, 401, 0);
        peer.until(script, |p| p.responses.contains(&(request, 0)));
    }
}

#[allow(clippy::too_many_arguments)]
pub fn exercise_battle(
    host: &mut Peer,
    host_vehicle: i64,
    guest: &mut Peer,
    guest_vehicle: i64,
    script: &ScriptDispatch,
    fixture: BattleFixture,
    visitor_account: i64,
    visit_in_battle: impl FnOnce(i64),
) {
    let shell_damage = fixture.shell_damage;
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
            Value::Int32(BATTLE_ARENA_TYPE),
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
    invite::send(host, script, &[visitor_account], invite::MESSAGE);
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
    let (preparing, resume) = telemetry_capture::pause_next_arena_preparation();
    command(host, 5110, 411, [2, 0, 0]);
    preparing
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    command(host, 5111, 412, [2, 0, 0]);
    host.until(script, |p| p.responses.contains(&(5111, 0)));
    assert!(!host.responses.iter().any(|(request, _)| *request == 5110));
    drop(resume);
    host.until(script, |p| p.responses.contains(&(5110, -1)));
    assert_eq!(host.resets, 0);
    command(host, 5112, 411, [1, 0, 0]);
    host.until(script, |p| p.responses.contains(&(5112, 0)));

    command(host, 5109, 411, [2, 0, 0]);
    host.until(script, |p| p.resets == 1);
    guest.until(script, |p| p.resets == 1);
    assert!(host.responses.contains(&(5109, 0)));
    assert!(
        host.methods[before_confirmations..]
            .iter()
            .any(|(name, args)| name == "updatePrebattle"
                && args[0].as_i64() == Some(6)
                && pickle_value(&args[1]) == wot_platform::pickle!((2, 2)))
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
    visit_in_battle(room_id);

    let host_cell = host.cell.as_ref().unwrap();
    let guest_cell = guest.cell.as_ref().unwrap();
    assert_eq!(host.entered_arena, guest.entered_arena);
    assert_ne!(i64::from(host_cell.space_id), room_id);
    assert_eq!(
        host.entered_arena,
        Some((u64::from(host_cell.space_id), BATTLE_ARENA_TYPE))
    );
    assert!(
        host_cell.position.y > 1.0 && guest_cell.position.y > 1.0,
        "Lakeville spawns must be elevated above world zero: {:?} and {:?}",
        host_cell.position,
        guest_cell.position
    );
    let initial_host = host_cell.position;
    let initial_guest = guest_cell.position;
    let geometry = host.geometry.as_ref().unwrap();
    assert_eq!(geometry.space_id, guest_cell.space_id);
    assert_eq!(geometry.path, guest.geometry.as_ref().unwrap().path);
    assert_ne!(host_cell.vehicle_id, guest_cell.vehicle_id);
    assert_ne!(host_cell.position, guest_cell.position);
    let avatar_schema = script.entity_from_name("Avatar").unwrap().1;
    let vehicle_type = script.entity_from_name("Vehicle").unwrap().0;
    for (peer, cell, team, display_type) in [(&*host, host_cell, 1, 1), (&*guest, guest_cell, 2, 2)]
    {
        use wot_platform::mercury::net::codec::Codec;
        let mut blob = cell.properties.as_slice();
        let Value::Dict(properties) = Value::read(&mut blob, &avatar_schema.cell_data_ty).unwrap()
        else {
            panic!("avatar cell properties");
        };
        assert_eq!(properties["team"].as_i64(), Some(team));
        assert_eq!(
            properties["customizationDisplayType"],
            Value::UInt8(display_type)
        );
        let Value::UInt64(aux) = properties["ownVehicleAuxPhysicsData"] else {
            panic!("expected packed hull orientation");
        };
        assert_aux_orientation(aux, cell.direction);
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
        for cell in [host_cell, guest_cell] {
            let vehicle = peer
                .arena_entities
                .iter()
                .find(|entity| entity.entity_id == cell.vehicle_id)
                .unwrap();
            assert_eq!(vehicle.position, cell.position);
            assert_eq!(vehicle.direction, cell.direction);
        }
    }

    for cell in [host_cell, guest_cell] {
        let vehicle_properties = |peer: &Peer| {
            peer.arena_entities
                .iter()
                .find(|entity| entity.entity_id == cell.vehicle_id)
                .unwrap()
                .properties
                .clone()
        };
        assert_eq!(vehicle_properties(host), vehicle_properties(guest));
    }

    let period = |p: &Peer| {
        p.methods
            .iter()
            .any(|(name, args)| name == "updateArena" && args[0].as_i64() == Some(3))
    };
    host.command(script, "setClientReady", vec![]);
    let host_ready_ack = host.last_sent_reliable + 1;
    host.until(script, |p| {
        p.ack_only_through
            .is_some_and(|ack| ack.wrapping_cmp(host_ready_ack).is_ge())
    });
    host.until(script, |p| p.avatar_position_frames >= 2);
    guest.command(script, "setClientReady", vec![]);
    host.until(script, period);
    guest.until(script, period);
    guest.until(script, |p| p.avatar_position_frames >= 2);
    for peer in [&*host, &*guest] {
        assert_roster_ready(script, &peer.nrl_updates);
    }
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
        matches!(wot_platform::client_formats::pickle::from_reader(flate2::read::ZlibDecoder::new(data.as_slice())),
            Ok(Pickle::Tuple(fields)) if fields.first() == Some(&Pickle::I64(3)))
    })
    };
    host.until(script, in_battle);
    guest.until(script, in_battle);
    for peer in [&mut *host, &mut *guest] {
        let first_tick = peer.arena_tick.unwrap();
        peer.until(script, |p| {
            p.arena_tick
                .is_some_and(|tick| (3..=127).contains(&tick.wrapping_sub(first_tick)))
        });
        assert_eq!(peer.positions[&first], initial_host);
        assert_eq!(peer.positions[&second], initial_guest);
    }
    host.cell_command(script, "vehicle_moveWith", vec![Value::UInt8(0x80)]);
    host.cell_command(script, "vehicle_moveWith", vec![Value::UInt8(1)]);
    guest.cell_command(script, "vehicle_moveWith", vec![Value::UInt8(1)]);
    for peer in [&mut *host, &mut *guest] {
        peer.until(script, |p| {
            p.positions.get(&first).is_some_and(|p| *p != initial_host)
                && p.positions
                    .get(&second)
                    .is_some_and(|p| *p != initial_guest)
        });
        assert!(peer.positions[&first].y > 1.0 && peer.positions[&second].y > 1.0);
    }
    guest.until(script, |p| p.engine_modes.get(&first) == Some(&[3, 1]));
    host.until(script, |p| {
        p.hull_aim
            .is_some_and(|aux| (aux >> 40) as u8 > 85 && (aux >> 48) as u8 > 85)
    });
    host.cell_command(script, "vehicle_moveWith", vec![Value::UInt8(64)]);
    guest.cell_command(script, "vehicle_moveWith", vec![Value::UInt8(64)]);
    host.vehicle_command(
        script,
        second,
        "trackRelativePointWithGun",
        vec![Value::Vector3(glam::Vec3::Y)],
    );
    let centered = host.positions[&second] + glam::Vec3::Y * 1.35;
    let across = (centered - host.positions[&first])
        .cross(glam::Vec3::Y)
        .normalize();
    let target = centered + across * 0.25;
    let offset = target - host.positions[&first];
    host.cell_command(script, "setServerMarker", vec![Value::UInt8(1)]);
    host.vehicle_command(
        script,
        first,
        "trackRelativePointWithGun",
        vec![Value::Vector3(offset)],
    );
    host.until(script, |peer| {
        gun_markers(peer, script)
            .last()
            .is_some_and(|(origin, velocity, _)| {
                (target - *origin).cross(velocity.normalize()).length() < 0.06
            })
    });
    host.cell_command(script, "setServerMarker", vec![Value::UInt8(0)]);
    host.cell_command(
        script,
        "vehicle_changeSetting",
        vec![Value::UInt8(1), Value::Int32(-1)],
    );
    host.cell_command(script, "vehicle_shoot", vec![]);
    host.cell_command(script, "vehicle_shoot", vec![]);
    host.until(script, |peer| {
        peer.methods
            .iter()
            .any(|(name, _)| name == "stopTracer" || name == "explodeProjectile")
    });
    assert!(
        host.methods.iter().any(|(name, _)| name == "stopTracer"),
        "direct shot tracer and scene impact: {:?}",
        host.methods
            .iter()
            .filter(|(name, _)| name == "showTracer" || name == "explodeProjectile")
            .collect::<Vec<_>>()
    );
    let received_shot = |p: &Peer| {
        p.methods.iter().any(|(name, _)| name == "stopTracer")
            && p.methods.iter().any(|(name, _)| name == "onBattleEvents")
            && p.vehicle_methods.iter().any(|(target, name, args)| {
                *target == second
                    && name == "onHealthChanged"
                    && args[0].as_i64() < args[1].as_i64()
            })
    };
    host.until(script, received_shot);
    guest.until(script, received_shot);
    let (_, shot_results) = host
        .methods
        .iter()
        .find(|(name, _)| name == "showShotResults")
        .expect("shooter received the result");
    let [Value::Seq(results)] = shot_results.as_slice() else {
        panic!("expected shot results array");
    };
    let [Value::Dict(hit)] = results.as_slice() else {
        panic!("expected one target in the shot results");
    };
    assert_eq!(hit["vehicleID"].as_i64(), Some(i64::from(second)));
    assert_eq!(hit["hitFlags"].as_i64(), Some(0x0010_0010));
    assert_eq!(hit["gunInstallationIndex"].as_i64(), Some(0));
    let (_, _, health) = guest
        .vehicle_methods
        .iter()
        .find(|(target, name, _)| *target == second && name == "onHealthChanged")
        .unwrap();
    let rolled_damage = health[1].as_i64().unwrap() - health[0].as_i64().unwrap();
    let [min, max] = [0.75, 1.25].map(|scale| (f32::from(shell_damage) * scale).round() as i64);
    assert!(
        (min..=max).contains(&rolled_damage),
        "rolled {rolled_damage}, expected {min}..={max}"
    );
    assert!(
        !guest
            .methods
            .iter()
            .any(|(name, _)| name == "showShotResults")
    );

    for peer in [&*host, &*guest] {
        let (_, tracer) = peer
            .methods
            .iter()
            .find(|(name, _)| name == "showTracer")
            .unwrap();
        let (_, endpoint) = peer
            .methods
            .iter()
            .find(|(name, _)| name == "stopTracer")
            .unwrap();
        assert_eq!(tracer[0].as_i64(), Some(i64::from(first)));
        assert_eq!(tracer[1], endpoint[0]);
        let (
            Value::Vector3(origin),
            Value::Vector3(velocity),
            Value::Float32(range),
            Value::Vector3(end),
        ) = (&tracer[7], &tracer[8], &tracer[10], &endpoint[1])
        else {
            panic!("expected tracer geometry");
        };
        assert_eq!(*range, fixture.shell_range);
        assert!((velocity.length() - fixture.shell_speed).abs() < 0.01);
        let ray = velocity.normalize();
        let offset = *end - *origin;
        assert!(offset.cross(ray).length() < 0.01);
        assert!((target - *origin).cross(ray).length() < 0.10);
        assert!((centered - *origin).cross(ray).length() > 0.15);
        assert!((0.0..=*range).contains(&offset.dot(ray)));

        let (_, _, impact) = peer
            .vehicle_methods
            .iter()
            .find(|(target, name, _)| *target == second && name == "showDamageFromShot")
            .expect("target impact");
        assert_eq!(impact[0].as_i64(), Some(i64::from(first)));
        assert_eq!(impact[4].as_i64(), Some(rolled_damage));
        let Value::Seq(points) = &impact[1] else {
            panic!("expected impact points");
        };
        let [Value::Dict(point)] = points.as_slice() else {
            panic!("expected one point for the penetrating impact effect");
        };
        assert_eq!(point["networkID"].as_i64(), Some(0));
        let Value::UInt64(segment) = point["segment"] else {
            panic!("expected packed hull segment");
        };
        let [effect, part, ax, ay, az, bx, by, bz] = segment.to_le_bytes();
        assert_eq!(effect, 4);
        assert!(matches!(part, 1 | 2));
        assert_ne!([ax, ay, az], [bx, by, bz]);
        let params = point["params"].as_i64().unwrap();
        assert_eq!(params & 0xf, i64::from(effect));
        assert_eq!((params >> 4) & 0xf, tracer[5].as_i64().unwrap());
        assert_eq!(tracer[5], Value::UInt8(2));
        assert_eq!(tracer[6], Value::Float32(37.0));
        assert_eq!((params >> 8) & 0xffff, 370);
        assert_eq!(impact[2], tracer[3]);
        assert_eq!(impact[3], tracer[4]);
        if fixture.has_damage_stickers {
            let Value::Seq(stickers) = &peer.damage_stickers[&second] else {
                panic!("expected target damage sticker array");
            };
            let [Value::Dict(sticker)] = stickers.as_slice() else {
                panic!("expected one persistent damage sticker");
            };
            assert_eq!(sticker["networkID"], point["networkID"]);
            let Value::UInt64(sticker_segment) = sticker["segment"] else {
                panic!("expected packed sticker segment");
            };
            assert_eq!(sticker_segment >> 8, segment >> 8);
            assert_eq!(
                sticker["params"].as_i64().unwrap(),
                params | (tracer[4].as_i64().unwrap() << 24)
            );
        }
        let (_, _, health) = peer
            .vehicle_methods
            .iter()
            .find(|(target, name, _)| *target == second && name == "onHealthChanged")
            .unwrap();
        assert_eq!(
            impact[5].as_i64(),
            Some(rolled_damage * 255 / health[1].as_i64().unwrap())
        );
    }
    for (peer, event_type, other_vehicle) in [(&*host, 7, second), (&*guest, 10, first)] {
        let (_, events) = peer
            .methods
            .iter()
            .find(|(name, _)| name == "onBattleEvents")
            .unwrap();
        let [Value::Seq(events)] = events.as_slice() else {
            panic!("expected battle events array");
        };
        let [Value::Dict(event)] = events.as_slice() else {
            panic!("expected one damage event");
        };
        assert_eq!(event["eventType"].as_i64(), Some(event_type));
        assert_eq!(event["targetID"].as_i64(), Some(i64::from(other_vehicle)));
        assert_eq!(event["count"].as_i64(), Some(1));
        let Value::UInt64(details) = event["details"] else {
            panic!("expected packed damage details");
        };
        assert_eq!(details >> 25 & 0xffff, rolled_damage as u64);
        assert_eq!(details >> 17 & 0xff, 0);
        assert_eq!(details >> 9 & 0x7f, if event_type == 10 { 1 } else { 127 });
    }
    guest.until(script, |p| {
        p.methods
            .iter()
            .any(|(name, _)| name == "showOwnVehicleHitDirection")
    });
    let (_, direction) = guest
        .methods
        .iter()
        .find(|(name, _)| name == "showOwnVehicleHitDirection")
        .unwrap();
    let (_, _, health) = guest
        .vehicle_methods
        .iter()
        .find(|(target, name, _)| *target == second && name == "onHealthChanged")
        .unwrap();
    assert_eq!(
        health[1].as_i64().unwrap() - health[0].as_i64().unwrap(),
        rolled_damage
    );
    assert_eq!(direction[1].as_i64(), Some(i64::from(first)));
    assert_eq!(
        direction[2].as_i64(),
        Some(health[1].as_i64().unwrap() - health[0].as_i64().unwrap())
    );
    assert_eq!(direction[6].as_i64(), Some(i64::from(second)));
    let Value::Float32(yaw) = direction[0] else {
        panic!("Expected hit bearing in radians");
    };
    let towards_target = guest.positions[&second] - guest.positions[&first];
    let expected_yaw = towards_target.x.atan2(towards_target.z);
    assert!((yaw - expected_yaw).cos() > 0.95);
    assert!(
        !host
            .methods
            .iter()
            .any(|(name, _)| name == "showOwnVehicleHitDirection")
    );

    let schema = script.entity_from_name("OwnVehicle").unwrap().1;
    let (reload_index, reload) = schema
        .properties
        .iter()
        .enumerate()
        .find(|(_, property)| &*property.name == "vehicleGunReloadTime")
        .unwrap();
    let node = (1026 + reload_index as u16).to_be_bytes();
    let reload_times = |peer: &Peer| {
        use wot_platform::mercury::net::codec::Codec;
        peer.nrl_updates
            .iter()
            .filter_map(|record| {
                let (length, mut body) = record
                    .strip_prefix(&node)?
                    .strip_prefix(&[0])?
                    .split_first()?;
                assert_eq!(usize::from(*length), body.len());
                let Value::Dict(reload) = Value::read(&mut body, &reload.ty).unwrap() else {
                    return None;
                };
                let Value::Float32(time) = reload["timeLeft"] else {
                    panic!("Expected gun reload time");
                };
                Some(time)
            })
            .collect::<Vec<_>>()
    };
    host.until(script, |p| {
        let times = reload_times(p);
        times.last() == Some(&0.0) && times.iter().any(|&time| time > 0.0)
    });

    assert!(
        guest
            .vehicle_methods
            .iter()
            .any(|(target, name, _)| *target == first && name == "showShooting")
    );
    let facing = host.cell.as_ref().unwrap().direction.z;
    assert_gun_marker(host, guest, script, first, facing, fixture.shell_speed);
    let reload_count = reload_times(host).len();
    assert_ground_impact(host, guest, script);
    if fixture.has_obstacles {
        std::thread::scope(|scope| {
            let (aiming, aimed) = std::sync::mpsc::channel::<()>();
            let observer = &mut *guest;
            scope.spawn(move || {
                observer.until(script, |_| {
                    !matches!(aimed.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty))
                });
            });
            host.until(script, |peer| {
                let times = reload_times(peer);
                times[reload_count..].contains(&0.0)
            });
            let stone_wall = glam::Vec3::new(-50.5, 9.0, 359.0);
            let offset = stone_wall - host.positions[&first];
            host.vehicle_command(
                script,
                first,
                "trackRelativePointWithGun",
                vec![Value::Vector3(offset)],
            );
            host.until(script, |peer| {
                gun_markers(peer, script)
                    .last()
                    .is_some_and(|(origin, velocity, _)| {
                        let target = stone_wall - *origin;
                        let direction = velocity.normalize();
                        target.dot(direction) > 0.0 && target.cross(direction).length() < 0.15
                    })
            });
            drop(aiming);
        });
        assert_scene_impact(host, guest, script, 2, 20.0..80.0);
    }
    host.cell_command(
        script,
        "vehicle_stopTrackingWithGun",
        vec![Value::Float32(0.0), Value::Float32(-0.6)],
    );
    host.until(script, |peer| reload_times(peer).last() == Some(&0.0));
    host.until(script, |peer| {
        gun_markers(peer, script)
            .last()
            .is_some_and(|(_, velocity, _)| velocity.y > 60.0)
    });
    assert_range_expiry(host, guest, script, fixture.shell_range);

    let forward = glam::Vec3::new(facing.sin(), 0.0, facing.cos());
    let stopped = host.positions[&first];
    host.cell_command(script, "vehicle_moveWith", vec![Value::UInt8(2)]);
    host.until(script, |p| {
        (p.positions[&first] - stopped).dot(forward) < -0.1 && p.hull_aim.is_some()
    });
    assert_aux_orientation(host.hull_aim.unwrap(), host.vehicle_directions[&first]);
    let before_reverse_turn = host.vehicle_directions[&first].z;
    host.cell_command(script, "vehicle_moveWith", vec![Value::UInt8(2 | 8)]);
    host.until(script, |p| {
        (p.vehicle_directions[&first].z - before_reverse_turn).sin() < -0.05
            && p.hull_aim
                .is_some_and(|aux| (aux >> 40) as u8 != (aux >> 48) as u8)
    });
    guest.until(script, |p| {
        (p.vehicle_directions[&first].z - before_reverse_turn).sin() < -0.05
            && p.engine_modes.get(&first) == Some(&[3, 10])
    });
    host.cell_command(script, "vehicle_moveWith", vec![Value::UInt8(0)]);
    host.until(script, |p| {
        p.hull_aim
            .is_some_and(|aux| (aux >> 40) as u8 == 85 && (aux >> 48) as u8 == 85)
    });
    guest.until(script, |p| p.engine_modes.get(&first) == Some(&[1, 0]));
    host.cell_command(script, "vehicle_moveWith", vec![Value::UInt8(1)]);
    let stationary = host.positions[&first];
    host.until(script, |p| p.positions[&first].distance(stationary) > 0.5);
    let outcome_start = host.methods.len();
    guest.command(script, "leaveArena", vec![]);
    guest.until(script, |p| p.resets == 2);
    host.until(script, |p| {
        p.methods[outcome_start..].iter().any(|(name, args)| {
            name == "onRoundFinished" && args == &[Value::Int8(1), Value::UInt8(1)]
        })
    });
    host.until(script, |p| {
        p.methods[outcome_start..].iter().any(|(name, args)| {
            if name != "updateArena" || args[0].as_i64() != Some(3) {
                return false;
            }
            let Value::String(StringValue::Raw(data)) = &args[1] else {
                return false;
            };
            matches!(wot_platform::client_formats::pickle::from_reader(
                flate2::read::ZlibDecoder::new(data.as_slice())),
                Ok(Pickle::Tuple(fields)) if fields.first() == Some(&Pickle::I64(4))
                    && fields.last() == Some(&wot_platform::pickle!((1, 1))))
        })
    });
    assert_eq!(host.resets, 1);
    let position_at_result = host.positions[&first];
    host.until(script, |p| {
        p.resets == 2 || p.positions[&first].distance(position_at_result) > 0.05
    });
    assert_eq!(
        host.resets, 1,
        "Vehicle must coast after the outcome is sent"
    );
    host.until(script, |p| p.resets == 2);
    host.enable_entities();
    guest.enable_entities();
    host.until(script, |p| p.created && p.shown);
    guest.until(script, |p| p.created && p.shown);
    let (total, rooms) = training_list(host, script, 0, 50);
    assert_eq!(total, 1);
    assert!(matches!(&rooms[..], [Pickle::Tuple(room)] if room[1] == Pickle::I64(room_id)));
}

fn assert_range_expiry(host: &mut Peer, guest: &mut Peer, script: &ScriptDispatch, range: f32) {
    let start = [host.methods.len(), guest.methods.len()];
    host.cell_command(script, "vehicle_shoot", vec![]);
    for (peer, since) in [(host, start[0]), (guest, start[1])] {
        peer.until(script, |p| {
            p.methods[since..]
                .iter()
                .any(|(name, _)| name == "showTracer")
        });
        let tracer = peer.methods[since..]
            .iter()
            .find(|(name, _)| name == "showTracer")
            .unwrap();
        assert_eq!(tracer.1[10], Value::Float32(range));
        let shot_id = tracer.1[1].clone();
        let tick = peer.arena_tick.unwrap();
        let Value::Vector3(velocity) = tracer.1[8] else {
            panic!("Expected tracer velocity");
        };
        let flight = ((range / velocity.length()) * 10.0).ceil() as u8;
        peer.until(script, |p| {
            p.arena_tick
                .is_some_and(|now| (flight..=127).contains(&now.wrapping_sub(tick)))
        });
        assert!(!peer.methods[since..].iter().any(|(name, args)| {
            matches!(name.as_str(), "stopTracer" | "explodeProjectile") && args[0] == shot_id
        }));
    }
}

fn assert_ground_impact(host: &mut Peer, guest: &mut Peer, script: &ScriptDispatch) {
    host.cell_command(script, "setServerMarker", vec![Value::UInt8(1)]);
    host.cell_command(
        script,
        "vehicle_stopTrackingWithGun",
        vec![Value::Float32(-0.25), Value::Float32(0.25)],
    );
    host.until(script, |peer| {
        gun_markers(peer, script)
            .last()
            .is_some_and(|(_, velocity, _)| velocity.y < -35.0)
    });
    assert_scene_impact(host, guest, script, 0, 0.0..50.0);
}

#[track_caller]
fn assert_scene_impact(
    host: &mut Peer,
    guest: &mut Peer,
    script: &ScriptDispatch,
    material: u8,
    expected_distance: std::ops::Range<f32>,
) {
    host.cell_command(script, "vehicle_shoot", vec![]);
    for peer in [host, guest] {
        let received = peer.methods.len();
        peer.until(script, |peer| {
            peer.methods[received..]
                .iter()
                .any(|(name, _)| name == "explodeProjectile")
        });
        let messages = &peer.methods[received..];
        let explosion_index = messages
            .iter()
            .position(|(name, _)| name == "explodeProjectile")
            .unwrap();
        let (_, impact) = &messages[explosion_index];
        let (_, tracer) = peer.methods[..received + explosion_index]
            .iter()
            .rev()
            .find(|(name, _)| name == "showTracer")
            .expect("tracer sent before explosion");
        assert_eq!(impact[0], tracer[1]);
        assert_eq!(impact[1..3], tracer[3..5]);
        assert_eq!(impact[3], Value::UInt8(material));
        assert_eq!(impact[4..6], tracer[5..7]);
        let (Value::Vector3(endpoint), Value::Vector3(origin), Value::Vector3(velocity)) =
            (&impact[6], &tracer[7], &tracer[8])
        else {
            panic!("expected impact geometry");
        };
        assert!(expected_distance.contains(&endpoint.distance(*origin)));
        assert!((*endpoint - *origin).normalize().dot(velocity.normalize()) > 0.999);
        assert!(matches!(&impact[9], Value::Seq(destructibles) if destructibles.is_empty()));
        assert!(
            !messages
                .iter()
                .any(|(name, _)| name == "stopTracer" || name == "showShotResults")
        );
    }
}

fn assert_aux_orientation(aux: u64, direction: glam::Vec3) {
    use std::f32::consts::{PI, TAU};
    let yaw = ((aux & 32767) as f32 / 32768.0) * TAU - PI;
    let pitch = (((aux >> 15) & 4095) as f32 / 4095.0) * PI - PI * 0.5;
    let roll = (((aux >> 27) & 8191) as f32 / 8192.0) * TAU - PI;
    assert!((yaw - direction.z).cos() > 0.999_999);
    assert!((pitch - direction.y).abs() < 0.002);
    assert!((roll - direction.x).cos() > 0.999_999);
}

fn assert_gun_marker(
    host: &mut Peer,
    guest: &mut Peer,
    script: &ScriptDispatch,
    vehicle: u32,
    facing: f32,
    speed: f32,
) {
    use std::f32::consts::{PI, TAU};
    let (marker_type, _) = script.entity_from_name("GunMarkerComponent").unwrap();
    let marker_type = script.component_nrl_id(marker_type).unwrap();
    let component = host.arena_roster.as_ref().unwrap();
    assert_eq!(
        &component[..7],
        &[8, 0, 4, 0, 2, marker_type as u8, (marker_type >> 8) as u8]
    );
    let markers = |peer: &Peer| gun_markers(peer, script);
    let aim = |peer: &mut Peer, yaw| {
        peer.cell_command(
            script,
            "vehicle_stopTrackingWithGun",
            vec![Value::Float32(yaw), Value::Float32(0.0)],
        );
        let yaw_bits = (((yaw + PI) / TAU * 1024.0).round() as u16) & 1023;
        move |p: &Peer| {
            p.gun_angles
                .get(&vehicle)
                .is_some_and(|value| value >> 6 == yaw_bits)
        }
    };

    host.cell_command(script, "setServerMarker", vec![Value::UInt8(1)]);
    host.until(script, |p| !markers(p).is_empty());
    let yaw = 0.25;
    let reached = aim(host, yaw);
    host.until(script, |p| {
        reached(p)
            && markers(p).last().is_some_and(|(_, velocity, _)| {
                let direction = p.vehicle_directions[&vehicle];
                let expected =
                    glam::Quat::from_euler(glam::EulerRot::YXZ, facing, direction.y, direction.x)
                        * glam::Quat::from_rotation_y(yaw)
                        * glam::Vec3::Z;
                velocity.normalize().dot(expected) > 0.999_999
            })
    });
    guest.until(script, reached);
    assert_eq!(host.gun_angles[&vehicle], guest.gun_angles[&vehicle]);
    let (origin, velocity, dispersion) = *markers(host).last().unwrap();
    assert!(origin.distance(host.positions[&vehicle]) < 5.0);
    assert!((velocity.length() - speed).abs() < 0.01 && dispersion > 0.0);
    assert!(markers(guest).is_empty());

    let marker_count = markers(host).len();
    host.cell_command(script, "setServerMarker", vec![Value::UInt8(0)]);
    let reached = aim(host, -yaw);
    host.until(script, reached);
    guest.until(script, reached);
    assert_eq!(markers(host).len(), marker_count);
}

fn gun_markers(peer: &Peer, script: &ScriptDispatch) -> Vec<(glam::Vec3, glam::Vec3, f32)> {
    use wot_platform::mercury::net::codec::Codec;
    let (_, schema) = script.entity_from_name("GunMarkerComponent").unwrap();
    let (index, property) = schema
        .properties
        .iter()
        .enumerate()
        .find(|(_, property)| &*property.name == "gunMarker")
        .unwrap();
    let node = (2049 + index as u16).to_be_bytes();
    peer.nrl_updates
        .iter()
        .filter_map(|record| {
            let mut body = record.strip_prefix(&node)?;
            let Value::Dict(marker) = Value::read(&mut body, &property.ty).unwrap() else {
                panic!("expected a gun marker dictionary");
            };
            assert!(body.is_empty());
            let (Value::Vector3(origin), Value::Vector3(velocity), Value::Float32(dispersion)) = (
                &marker["gunPosition"],
                &marker["shotVector"],
                &marker["dispersion"],
            ) else {
                panic!("expected launch position, velocity and dispersion");
            };
            Some((*origin, *velocity, *dispersion))
        })
        .collect()
}

fn assert_roster_ready(script: &ScriptDispatch, updates: &[Vec<u8>]) {
    use wot_platform::mercury::script::{Ty, TyKind};
    fn unalias(ty: &Ty) -> &TyKind {
        match ty.kind() {
            TyKind::Alias(inner) => unalias(inner),
            kind => kind,
        }
    }
    let schema = script.entity_from_name("AvatarVehiclesInfo").unwrap().1;
    let (property, roster) = schema
        .properties
        .iter()
        .enumerate()
        .find(|(_, p)| &*p.name == "vehiclesInfo")
        .unwrap();
    let TyKind::Array(rows) = unalias(&roster.ty) else {
        panic!("roster array");
    };
    let TyKind::Dict(fields) = unalias(&rows.ty) else {
        panic!("roster dictionary");
    };
    let bits = |count: usize| usize::BITS - count.saturating_sub(1).leading_zeros();
    let mut ready = std::collections::BTreeSet::new();
    let mut committed = std::collections::BTreeSet::new();
    let node = (4 + property as u16).to_be_bytes();
    for record in updates.iter().filter(|data| data.starts_with(&node)) {
        assert_eq!(record[2], 2);
        let path = &record[4..]; // This path and its value fit in a one-byte length.
        let mut bit = 0usize;
        let mut read = |width: u32| {
            (0..width).fold(0usize, |value, _| {
                let next = (path[bit / 8] >> (7 - bit % 8)) & 1;
                bit += 1;
                (value << 1) | usize::from(next)
            })
        };
        assert_eq!(read(1), 1);
        assert_eq!(read(bits(schema.properties.len())), property);
        assert_eq!(read(1), 1);
        let row = read(1);
        assert_eq!(read(1), 0);
        let field = read(bits(fields.properties.len()));
        let leaf = &path[bit.div_ceil(8)..];
        match &*fields.properties[field].name {
            "isAvatarReady" => {
                assert_eq!(leaf, [1]);
                ready.insert(row);
            }
            "__generation" => {
                assert_eq!(leaf, 1u32.to_le_bytes());
                assert!(ready.contains(&row));
                committed.insert(row);
            }
            name => panic!("Unexpected roster field {name}"),
        }
    }
    assert_eq!(committed, [0, 1].into_iter().collect());
}

pub fn assert_battle_traces(host: i64, guest: i64, fixture: BattleFixture) {
    let shell_damage = fixture.shell_damage;
    let traces = telemetry_capture::exporter().get_finished_spans().unwrap();
    let attribute = |span: &SpanData, key: &str| {
        span.attributes
            .iter()
            .find(|attribute| attribute.key.as_str() == key)
            .map(|attribute| attribute.value.to_string())
    };
    let has = |span: &SpanData, key, value: &str| {
        span.attributes
            .iter()
            .any(|attr| attr.key.as_str() == key && attr.value.to_string() == value)
    };
    let child_of = |child: &SpanData, parent: &SpanData| {
        child.parent_span_id == parent.span_context.span_id()
            && child.span_context.trace_id() == parent.span_context.trace_id()
    };
    assert!(traces.iter().any(|mobility| {
        mobility.name == "accounts.battle_mobility"
            && has(mobility, "ddd.bounded_context.name", "accounts")
            && attribute(mobility, "input.inventory_id").is_some()
            && (!fixture.has_vehicle_bounds
                || has(
                    mobility,
                    "output.mobility.collision_bounds_available",
                    "true",
                ))
            && [
                "output.mobility.power_watts_per_kg",
                "output.mobility.forward_speed_mps",
                "output.mobility.reverse_speed_mps",
            ]
            .into_iter()
            .all(|key| {
                attribute(mobility, key)
                    .and_then(|v| v.parse::<f64>().ok())
                    .is_some_and(|v| v.is_finite() && v > 0.0)
            })
            && traces.iter().any(|loadout| {
                loadout.name == "accounts.battle_vehicle" && child_of(mobility, loadout)
            })
    }));
    assert!(traces.iter().any(|update| {
        has(update, "input.replication.kind", "hull_aim")
            && has(update, "output.delivered.count", "1")
            && traces.iter().any(|drive| {
                has(drive, "rpc.method", "vehicle_moveWith") && child_of(update, drive)
            })
            && traces.iter().any(|delivery| {
                delivery.name == "mercury.arena_delivery"
                    && child_of(delivery, update)
                    && has(delivery, "output.delivery", "sent")
            })
    }));
    assert!(traces.iter().any(|drive| {
        drive.name == "battles.control"
            && has(drive, "input.drive.flags", "1")
            && traces.iter().any(|step| {
                step.name == "battles.drive_vehicle"
                    && child_of(step, drive)
                    && has(step, "input.drive.airborne", "false")
                    && has(step, "output.drive.track_locked", "false")
                    && step
                        .attributes
                        .iter()
                        .any(|attr| attr.key.as_str() == "output.drive.speed_mps")
                    && step
                        .attributes
                        .iter()
                        .any(|attr| attr.key.as_str() == "input.ground.medium_contacts")
            })
            && traces.iter().any(|update| {
                child_of(update, drive)
                    && has(update, "input.replication.kind", "engine_mode")
                    && has(update, "output.delivered.count", "2")
                    && traces.iter().any(|delivery| {
                        delivery.name == "mercury.arena_delivery"
                            && child_of(delivery, update)
                            && has(delivery, "output.delivery", "sent")
                    })
            })
    }));
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

    let marker_request = traces
        .iter()
        .find(|span| {
            span.name == "game.cell_command"
                && has(span, "rpc.method", "setServerMarker")
                && traces.iter().any(|control| {
                    child_of(control, span) && has(control, "input.aim.server_marker", "true")
                })
        })
        .expect("server marker request");
    let marker_control = traces
        .iter()
        .find(|span| child_of(span, marker_request) && has(span, "input.aim.server_marker", "true"))
        .unwrap();
    let marker = traces
        .iter()
        .find(|span| {
            child_of(span, marker_control) && has(span, "input.replication.kind", "gun_marker")
        })
        .unwrap();
    assert!(has(marker, "output.delivered.count", "1"));
    assert!(
        traces
            .iter()
            .any(|span| span.name == "mercury.arena_delivery"
                && child_of(span, marker)
                && has(span, "output.delivery", "sent"))
    );
    assert!(traces.iter().any(|control| {
        control.name == "battles.control"
            && has(control, "input.aim.kind", "hull_angles")
            && traces
                .iter()
                .any(|request| request.name == "game.cell_command" && child_of(control, request))
            && ["gun_angles", "gun_marker"].into_iter().all(|kind| {
                traces.iter().any(|replication| {
                    child_of(replication, control)
                        && has(replication, "input.replication.kind", kind)
                        && traces.iter().any(|delivery| {
                            delivery.name == "mercury.arena_delivery"
                                && child_of(delivery, replication)
                                && has(delivery, "output.delivery", "sent")
                        })
                })
            })
    }));

    let hit = traces
        .iter()
        .find(|s| s.name == "battles.shot" && has(s, "output.outcome", "penetrated"))
        .expect("training hit");
    assert!(has(hit, "ddd.bounded_context.name", "battles"));
    assert!(has(hit, "input.shell.damage", &shell_damage.to_string()));
    let damage = attribute(hit, "output.damage")
        .unwrap()
        .parse::<u16>()
        .unwrap();
    let [min, max] = [0.75, 1.25].map(|scale| (f32::from(shell_damage) * scale).round() as u16);
    assert!((min..=max).contains(&damage));
    let armor = traces
        .iter()
        .find(|span| span.name == "battles.resolve_armor" && child_of(span, hit))
        .expect("armor resolution belongs to the shot trace");
    assert!(has(armor, "ddd.bounded_context.name", "battles"));
    assert!(has(armor, "output.outcome", "penetrated"));
    assert!(has(armor, "output.damage", &damage.to_string()));
    for field in [
        "output.part.index",
        "output.material.kind",
        "output.plate.thickness_mm",
        "output.plate.effective_mm",
        "output.penetration.remaining_mm",
    ] {
        assert!(attribute(armor, field).is_some(), "missing {field}");
    }
    assert!(has(hit, "output.damage_log.shell_type", "1"));
    let control = traces
        .iter()
        .find(|s| s.name == "battles.control" && child_of(hit, s))
        .unwrap();
    let shooter = attribute(control, "input.participant.id").expect("shooting participant ID");
    assert!(has(hit, "input.participant.id", &shooter));
    assert!(
        traces
            .iter()
            .any(|s| s.name == "game.cell_command" && child_of(control, s))
    );
    let flight = attribute(hit, "output.projectile.flight_seconds")
        .unwrap()
        .parse::<f64>()
        .unwrap();
    assert!(flight > 0.0);
    let earliest_impact = hit.start_time + Duration::from_secs_f64((flight - 0.01).max(0.0));
    for impact in traces
        .iter()
        .filter(|span| child_of(span, hit) && has(span, "rpc.method", "stopTracer"))
    {
        assert!(impact.start_time >= earliest_impact);
    }
    if fixture.has_damage_stickers {
        assert!(has(hit, "output.damage_stickers.count", "1"));
        let sticker_updates: Vec<_> = traces
            .iter()
            .filter(|span| child_of(span, hit) && span.name == "battles.damage_stickers")
            .collect();
        assert_eq!(sticker_updates.len(), 2);
        for update in sticker_updates {
            assert!(has(update, "input.damage_stickers.count", "1"));
            assert!(update.start_time >= earliest_impact);
            assert!(traces.iter().any(|delivery| {
                child_of(delivery, update)
                    && delivery.name == "mercury.arena_delivery"
                    && has(delivery, "output.delivery", "sent")
            }));
        }
    }
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
    for (span_name, method, recipients) in [
        ("mercury.arena_vehicle_method", "showDamageFromShot", 2),
        ("mercury.response", "showTracer", 2),
        ("mercury.response", "stopTracer", 2),
        ("mercury.response", "showShotResults", 1),
        ("mercury.response", "onBattleEvents", 2),
        ("mercury.response", "showOwnVehicleHitDirection", 1),
    ] {
        assert_eq!(
            traces
                .iter()
                .filter(|span| span.name == span_name
                    && child_of(span, hit)
                    && has(span, "rpc.method", method)
                    && has(span, "output.delivery", "sent"))
                .count(),
            recipients,
            "{method}"
        );
    }
    assert!(
        hit.attributes
            .iter()
            .any(|attribute| attribute.key.as_str() == "output.hit_direction.radians")
    );
    let impact_outcomes = [
        Some("ground_impact"),
        fixture.has_obstacles.then_some("obstacle_impact"),
    ];
    for outcome in impact_outcomes.into_iter().flatten() {
        let impact = traces
            .iter()
            .find(|s| s.name == "battles.shot" && has(s, "output.outcome", outcome))
            .expect("scene impact traced");
        assert!(has(impact, "input.shell.kind", "2"));
        assert!(has(impact, "input.shell.caliber_mm", "37"));
        assert!(
            impact.attributes.iter().any(|attribute| {
                attribute.key.as_str() == "output.scene_impact.distance_metres"
            })
        );
        if outcome == "obstacle_impact" {
            assert!(has(impact, "output.obstacle.material_kind", "111"));
            assert!(traces.iter().any(|span| {
                span.name == "battles.load_obstacles"
                    && has(span, "ddd.bounded_context.name", "battles")
                    && span
                        .attributes
                        .iter()
                        .any(|attribute| attribute.key.as_str() == "output.obstacles.instances")
            }));
        }
        assert!(traces.iter().any(|control| {
            control.name == "battles.control"
                && child_of(impact, control)
                && traces.iter().any(|request| {
                    request.name == "game.cell_command" && child_of(control, request)
                })
        }));
        assert_eq!(
            traces
                .iter()
                .filter(|span| {
                    span.name == "mercury.response"
                        && child_of(span, impact)
                        && has(span, "rpc.method", "explodeProjectile")
                        && has(span, "output.delivery", "sent")
                })
                .count(),
            2
        );
    }
    let reload = traces
        .iter()
        .find(|span| {
            span.name == "battles.replication"
                && child_of(span, hit)
                && has(span, "input.replication.kind", "ammunition")
        })
        .expect("reload completion");
    assert!(
        traces
            .iter()
            .any(|span| span.name == "mercury.arena_delivery"
                && child_of(span, reload)
                && has(span, "output.delivery", "sent"))
    );
    assert!(traces.iter().any(|span| span.name == "battles.control"
        && has(span, "input.ammunition.setting", "1")
        && has(span, "input.ammunition.shell", "-1")
        && has(span, "output.outcome", "shell_unavailable")));
    let movement = traces
        .iter()
        .find(|s| s.name == "battles.replication" && has(s, "input.replication.kind", "position"))
        .expect("movement replication");
    assert!(has(movement, "output.delivered.count", "2"));
    assert!(has(movement, "ddd.bounded_context.name", "battles"));
    assert!(movement.attributes.iter().any(|attribute| {
        attribute.key.as_str() == "input.arena.tick"
            && matches!(attribute.value, opentelemetry::Value::I64(tick) if tick > 0)
    }));
    assert!(traces.iter().any(|s| s.name == "battles.control"
        && child_of(movement, s)
        && has(s, "input.drive.flags", "1")));
    assert!(traces.iter().any(|s| s.name == "battles.control"
        && has(s, "input.drive.flags", "10")
        && traces.iter().any(|replication| child_of(replication, s)
            && has(replication, "input.replication.kind", "position")
            && has(replication, "output.delivered.count", "2"))));
    for outcome in ["reloading", "invalid_flags", "entity_unavailable"] {
        assert!(
            traces
                .iter()
                .any(|s| s.name == "game.cell_command" && has(s, "output.outcome", outcome)),
            "missing {outcome}"
        );
    }

    assert!(
        traces
            .iter()
            .any(|span| span.name == "mercury.acknowledge_arena_packet"
                && has(span, "output.delivery", "sent")
                && span.attributes.iter().any(|attribute| {
                    attribute.key.as_str() == "output.packet.sequence"
                        && matches!(attribute.value, opentelemetry::Value::I64(0..=0x0fff_ffff))
                }))
    );
    let readiness: Vec<_> = traces
        .iter()
        .filter(|span| span.name == "battles.roster_ready" && has(span, "input.ready", "true"))
        .collect();
    assert_eq!(readiness.len(), 2);
    for span in readiness {
        assert!(has(span, "output.recipients.count", "2"));
        assert!(traces.iter().any(|root| root.name == "game.command"
            && child_of(span, root)
            && has(root, "rpc.method", "setClientReady")));
        assert_eq!(
            traces
                .iter()
                .filter(|delivery| delivery.name == "mercury.arena_delivery"
                    && child_of(delivery, span)
                    && has(delivery, "input.element_id", "38")
                    && has(delivery, "output.delivery", "sent"))
                .count(),
            4
        );
    }

    let countdown = traces
        .iter()
        .find(|s| s.name == "battles.period" && has(s, "output.phase", "countdown"))
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

    let finished = traces
        .iter()
        .find(|s| {
            s.name == "battles.period"
                && has(s, "output.phase", "finished")
                && has(s, "output.battle.winner_team", "1")
                && has(s, "output.battle.finish_reason", "1")
        })
        .expect("training battle outcome");
    for method in ["onRoundFinished", "updateArena"] {
        assert!(traces.iter().any(|s| s.name == "mercury.response"
            && child_of(s, finished)
            && has(s, "rpc.method", method)
            && has(s, "output.delivery", "sent")));
    }
    assert!(traces.iter().any(|s| s.name == "battles.replication"
        && child_of(s, finished)
        && has(s, "input.replication.kind", "position")
        && has(s, "output.delivered.count", "1")));
    assert!(
        traces
            .iter()
            .any(|s| s.name == "player_sessions.return_from_battle"
                && child_of(s, finished)
                && has(s, "output.outcome", "garage_reset"))
    );
    let pending = traces
        .iter()
        .find(|s| {
            s.name == "player_sessions.send_training_invites"
                && has(s, "output.outcome", "queued")
                && has(s, "output.invites.queued_count", "1")
        })
        .expect("offline invitation retained");
    let delivered = traces
        .iter()
        .find(|s| {
            s.name == "player_sessions.deliver_pending_training_invites"
                && has(s, "output.invites.delivered_count", "1")
        })
        .expect("offline invitation delivered");
    assert!(
        delivered
            .links
            .iter()
            .any(|link| link.span_context == pending.span_context)
    );
    let range = fixture.shell_range.to_string();
    let miss = traces
        .iter()
        .find(|s| {
            s.name == "battles.shot"
                && has(s, "output.outcome", "miss")
                && has(s, "input.shell.max_distance_metres", &range)
        })
        .expect("range expiry trace");
    assert_eq!(
        traces
            .iter()
            .filter(|s| s.name == "mercury.response"
                && child_of(s, miss)
                && has(s, "rpc.method", "showTracer")
                && has(s, "output.delivery", "sent"))
            .count(),
        2
    );

    let leave = traces
        .iter()
        .find(|s| {
            s.name == "game.command"
                && has(s, "rpc.method", "leaveArena")
                && has(s, "user.id", &guest.to_string())
        })
        .expect("guest leave request");
    let garage_return = traces
        .iter()
        .find(|s| s.name == "player_sessions.return_from_battle" && child_of(s, leave))
        .expect("garage return in the leave request trace");
    assert!(has(garage_return, "output.outcome", "garage_reset"));
    let departed = traces
        .iter()
        .find(|span| span.name == "battles.remove_participant" && child_of(span, leave))
        .expect("participant removal by the battle worker in the leave request trace");
    assert!(departed.end_time <= garage_return.start_time);
    assert!(has(departed, "ddd.bounded_context.name", "battles"));
    assert!(has(departed, "output.players.count", "1"));
    let participant =
        attribute(departed, "input.participant.id").expect("departing participant ID");
    let avatar = attribute(leave, "input.entity_id").expect("departing Avatar ID");
    let battle = attribute(departed, "output.battle.id").expect("departed battle ID");
    assert!(traces.iter().any(|span| span.name == "battles.enter"
        && has(span, "input.battle.id", &battle)
        && has(span, "input.entity_id", &avatar)
        && has(span, "input.participant.id", &participant)
        && has(span, "output.outcome", "entered")));
    assert!(traces.iter().any(|control| {
        control.name == "battles.control"
            && has(control, "output.outcome", "invalid_flags")
            && traces.iter().any(|request| {
                request.name == "game.cell_command"
                    && child_of(control, request)
                    && has(request, "output.outcome", "invalid_flags")
            })
            && traces.iter().any(|entry| {
                entry.name == "battles.enter"
                    && ["input.battle.id", "input.entity_id", "input.participant.id"]
                        .iter()
                        .all(|key| {
                            attribute(control, key).is_some_and(|value| has(entry, key, &value))
                        })
            })
    }));
    for account in [host, guest] {
        let sync = traces
            .iter()
            .find(|s| {
                s.name == "game.command"
                    && has(s, "user.id", &account.to_string())
                    && has(s, "input.request_id", "221")
                    && has(s, "input.command_id", "100")
            })
            .expect("post-battle garage cache request");
        assert!(traces.iter().any(|s| s.name == "mercury.response.resource"
            && s.span_context.trace_id() == sync.span_context.trace_id()
            && has(s, "input.request_id", "221")
            && has(s, "output.delivery", "sent")));
    }

    for (request, account, outcome, code) in [
        ("5100", guest, "not_host", "-1"),
        ("5101", host, "team_not_ready", "-1"),
        ("5109", host, "launched", "0"),
        ("5110", host, "preparation_stale", "-1"),
        ("5111", host, "team_unlocked", "0"),
        ("5112", host, "team_ready", "0"),
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
            let arena = traces
                .iter()
                .find(|s| s.name == "battles.prepare" && child_of(s, prepare))
                .expect("battle preparation inside the Training launch trace");
            assert!(has(arena, "ddd.bounded_context.name", "battles"));
            assert!(has(arena, "output.outcome", "prepared"));
            assert!(has(arena, "output.players.count", "2"));
            let response = traces
                .iter()
                .find(|s| s.name == "mercury.response" && child_of(s, launch))
                .unwrap();
            let terrain = traces
                .iter()
                .find(|s| s.name == "battles.load_terrain" && child_of(s, arena))
                .expect("terrain loading in the battle preparation trace");
            assert!(terrain.end_time <= response.start_time);
            let battle_id = arena
                .attributes
                .iter()
                .find(|attribute| attribute.key.as_str() == "output.battle.id")
                .expect("allocated battle ID")
                .value
                .to_string();
            let activation = traces
                .iter()
                .find(|s| s.name == "battles.activate" && child_of(s, launch))
                .expect("battle worker activation inside the Training launch trace");
            assert!(has(activation, "input.battle.id", &battle_id));
            assert!(has(activation, "input.players.count", "2"));
            assert!(has(activation, "output.outcome", "activated"));
            let entries = traces
                .iter()
                .filter(|s| s.name == "battles.enter" && child_of(s, arena))
                .collect::<Vec<_>>();
            assert_eq!(entries.len(), 2);
            for entry in entries {
                assert!(has(entry, "output.outcome", "entered"));
                assert!(has(entry, "ddd.bounded_context.name", "battles"));
                assert!(has(entry, "input.battle.id", &battle_id));
                let display_type = if has(entry, "input.team", "1") {
                    "1"
                } else {
                    "2"
                };
                assert!(has(entry, "input.customization.display_type", display_type));
                assert!(has(entry, "output.avatar_control", "server"));
                for element in ["6", "65"] {
                    assert!(traces.iter().any(|s| s.name == "mercury.arena_delivery"
                        && child_of(s, entry)
                        && has(s, "input.element_id", element)
                        && has(s, "output.delivery", "sent")));
                }
            }
        }
    }
}
