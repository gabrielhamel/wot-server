use crate::garage_peer::Peer;
use std::collections::BTreeMap;
use wot_server::client_formats::pickle;
use wot_server::client_gateway::mercury::app::dispatch::ScriptDispatch;
use wot_server::client_gateway::mercury::script::{PythonValue, StringValue, Value};

fn text(s: &str) -> Value {
    Value::String(StringValue::String(s.into()))
}
fn chat(
    peer: &mut Peer,
    script: &ScriptDispatch,
    request: i64,
    command: u8,
    channel: u32,
    target: i64,
    message: &str,
) {
    peer.command(
        script,
        "chatCommandFromClient",
        vec![
            Value::Int64(request),
            Value::UInt8(command),
            Value::Int32(channel as i32),
            Value::Int64(target),
            Value::Int16(0),
            text(message),
            text(""),
        ],
    );
}
fn event(peer: &Peer, request: i64, action: i64) -> Option<&BTreeMap<std::sync::Arc<str>, Value>> {
    peer.methods
        .iter()
        .rev()
        .filter(|(name, _)| name.ends_with("onChatAction"))
        .find_map(|(_, args)| {
            let Value::Dict(data) = &args[0] else {
                return None;
            };
            (data["requestID"].as_i64() == Some(request) && data["action"].as_i64() == Some(action))
                .then_some(data)
        })
}
fn python(value: &Value) -> pickle::Value {
    match value {
        Value::Python(PythonValue::Decoded(v)) | Value::String(StringValue::Python(v)) => v.clone(),
        Value::Python(PythonValue::Raw(v)) | Value::String(StringValue::Raw(v)) => {
            pickle::from_bytes(v).unwrap()
        }
        _ => panic!("Expected Python value: {value:?}"),
    }
}
fn wait_event(peer: &mut Peer, script: &ScriptDispatch, request: i64, action: i64) {
    peer.until(script, |p| event(p, request, action).is_some());
    assert_eq!(
        event(peer, request, action).unwrap()["actionResponse"].as_i64(),
        Some(0),
        "request={request} action={action}"
    );
}
fn assert_no_room_events(peer: &mut Peer, script: &ScriptDispatch) {
    let stats = peer
        .methods
        .iter()
        .filter(|(name, _)| name.ends_with("receiveServerStats"))
        .count();
    peer.command(
        script,
        "doCmdInt3",
        vec![
            Value::Int16(2000),
            Value::Int16(501),
            Value::Int64(0),
            Value::Int64(0),
            Value::Int64(0),
        ],
    );
    peer.until(script, |p| {
        p.methods
            .iter()
            .filter(|(name, _)| name.ends_with("receiveServerStats"))
            .count()
            > stats
    });
    for (name, args) in &peer.methods {
        if !name.ends_with("onChatAction") {
            continue;
        }
        let Value::Dict(data) = &args[0] else {
            panic!()
        };
        assert!(
            !matches!(data["action"].as_i64(), Some(1 | 4 | 6 | 7 | 8)),
            "Unexpected remote room event: {data:?}"
        );
    }
}

fn pause() {
    std::thread::sleep(std::time::Duration::from_millis(120));
}

pub fn exercise(a: &mut Peer, b: &mut Peer, script: &ScriptDispatch, a_id: i64, b_id: i64) {
    for (peer, request) in [(&mut *a, 1001), (&mut *b, 1002)] {
        chat(peer, script, request, 9, 0, 0, "");
        wait_event(peer, script, request, 13);
    }
    pause();
    let args = Value::Dict(
        [
            ("int32Arg1".into(), Value::Int32(50)),
            ("int64Arg1".into(), Value::Int64(0)),
            ("floatArg1".into(), Value::Float64(0.0)),
            ("strArg1".into(), text("TestPlayerB")),
            ("strArg2".into(), text("")),
            ("int8Arg1".into(), Value::Int8(0)),
        ]
        .into(),
    );
    a.command(
        script,
        "messenger_onActionByClient_chat2",
        vec![Value::Int16(2), Value::UInt16(1003), args],
    );
    a.until(script, |p| {
        p.methods.iter().any(|(n, args)| {
            n.ends_with("messenger_onActionByServer_chat2") && args[1].as_i64() == Some(1003)
        })
    });
    let (_, args) = a
        .methods
        .iter()
        .find(|(n, args)| {
            n.ends_with("messenger_onActionByServer_chat2") && args[1].as_i64() == Some(1003)
        })
        .unwrap();
    assert_eq!(args[0].as_i64(), Some(0));
    let Value::Dict(data) = &args[2] else {
        panic!()
    };
    let pickle::Value::List(results) = python(&data["strArg1"]) else {
        panic!()
    };
    assert!(results.iter().any(|v| matches!(v,pickle::Value::Tuple(t) if t[0]==pickle::Value::Bytes(b"TestPlayerB".to_vec()) && t[1]==pickle::Value::I64(b_id))));
    pause();
    chat(a, script, 1004, 3, 0, b_id, "ForgedName");
    wait_event(a, script, 1004, 15);
    let pickle::Value::Tuple(contact) = python(&event(a, 1004, 15).unwrap()["data"]) else {
        panic!()
    };
    assert_eq!(contact[1], pickle::Value::Bytes(b"TestPlayerB".to_vec()));
    pause();
    chat(b, script, 1005, 3, 0, a_id, "TestPlayerA");
    wait_event(b, script, 1005, 15);
    pause();
    chat(a, script, 1006, 7, 0, b_id, "");
    wait_event(a, script, 1006, 7);
    assert_no_room_events(b, script);
    let room = event(a, 1006, 7).unwrap()["channel"].as_i64().unwrap() as u32;
    chat(a, script, 1100, 38, room, 0, "");
    wait_event(a, script, 1100, 8);
    assert_no_room_events(b, script);
    chat(a, script, 1101, 7, 0, b_id, "");
    wait_event(a, script, 1101, 7);
    assert_no_room_events(b, script);
    let reopened_room = event(a, 1101, 7).unwrap()["channel"].as_i64().unwrap() as u32;
    assert_ne!(
        reopened_room, room,
        "Unsent, closed conversations reclaim their room"
    );
    let room = reopened_room;
    chat(a, script, 1007, 39, room, 0, "Hello, tank commander! é");
    wait_event(a, script, 1007, 1);
    wait_event(b, script, 1007, 1);
    assert!(
        event(b, -1, 7).is_some(),
        "First message initializes recipient's channel"
    );
    assert_eq!(
        event(b, 1007, 1).unwrap()["originator"].as_i64(),
        Some(a_id)
    );
    assert_eq!(
        python(&event(b, 1007, 1).unwrap()["data"]),
        pickle::Value::Bytes("Hello, tank commander! é".as_bytes().to_vec())
    );
    pause();
    chat(b, script, 1008, 39, room, 0, "Hello back");
    wait_event(a, script, 1008, 1);
    wait_event(b, script, 1008, 1);
    chat(a, script, 1009, 36, room, 0, "");
    wait_event(a, script, 1009, 3);
    chat(a, script, 1010, 39, u32::MAX, 0, "forged room");
    a.until(script, |p| event(p, 1010, 1).is_some());
    assert_ne!(
        event(a, 1010, 1).unwrap()["actionResponse"].as_i64(),
        Some(0)
    );
    pause();
    chat(b, script, 1020, 4, 0, a_id, "TestPlayerA");
    wait_event(b, script, 1020, 14);
    std::thread::sleep(std::time::Duration::from_millis(510));
    chat(a, script, 1021, 39, room, 0, "blocked message");
    a.until(script, |p| event(p, 1021, 1).is_some());
    assert_eq!(
        event(a, 1021, 1).unwrap()["actionResponse"].as_i64(),
        Some(19)
    );
    pause();
    chat(b, script, 1022, 5, 0, a_id, "");
    wait_event(b, script, 1022, 17);
    pause();
    chat(b, script, 1023, 9, 0, 0, "");
    wait_event(b, script, 1023, 13);
    let pickle::Value::List(contacts) = python(&event(b, 1023, 13).unwrap()["data"]) else {
        panic!()
    };
    assert!(matches!(&contacts[0], pickle::Value::Tuple(t) if t[2] == pickle::Value::I64(2)));
    pause();
    chat(b, script, 1024, 6, 0, a_id, "");
    wait_event(b, script, 1024, 16);
    pause();
    chat(b, script, 1025, 3, 0, a_id, "");
    wait_event(b, script, 1025, 15);
    chat(a, script, 1026, 39, room, 0, &"x".repeat(2049));
    a.until(script, |p| event(p, 1026, 1).is_some());
    assert_eq!(
        event(a, 1026, 1).unwrap()["actionResponse"].as_i64(),
        Some(34)
    );
    std::thread::sleep(std::time::Duration::from_millis(510));
    chat(a, script, 1027, 39, room, 0, "Still serving valid messages");
    wait_event(a, script, 1027, 1);
    wait_event(b, script, 1027, 1);
    chat(b, script, 1028, 7, 0, a_id, "");
    wait_event(b, script, 1028, 7);
    a.methods.clear();
    assert_no_room_events(a, script);
    assert_eq!(
        event(b, 1028, 7).unwrap()["channel"].as_i64(),
        Some(i64::from(room))
    );
    a.methods
        .retain(|(name, _)| !name.ends_with("onChatAction"));
    b.methods
        .retain(|(name, _)| !name.ends_with("onChatAction"));
    chat(a, script, 1011, 38, room, 0, "");
    wait_event(a, script, 1011, 8);
    assert_no_room_events(b, script);
    std::thread::sleep(std::time::Duration::from_millis(510));
    chat(b, script, 1029, 39, room, 0, "Still open on this side");
    wait_event(b, script, 1029, 1);
    wait_event(a, script, 1029, 1);
    assert!(
        event(a, -1, 7).is_some(),
        "Incoming message reopens closed recipient"
    );
    assert!(!b.methods.iter().any(|(_, args)| matches!(args.first(), Some(Value::Dict(data)) if data.get("action").and_then(Value::as_i64) == Some(4))));
    pause();
    chat(a, script, 1012, 9, 0, 0, "");
    wait_event(a, script, 1012, 13);
    let pickle::Value::List(roster) = python(&event(a, 1012, 13).unwrap()["data"]) else {
        panic!()
    };
    assert_eq!(roster.len(), 1);
    pause();
    chat(a, script, 1013, 5, 0, b_id, "");
    wait_event(a, script, 1013, 17);
    pause();
    chat(a, script, 1014, 9, 0, 0, "");
    wait_event(a, script, 1014, 13);
    assert_eq!(
        python(&event(a, 1014, 13).unwrap()["data"]),
        pickle::Value::List(vec![])
    );
}

pub fn wait_presence(peer: &mut Peer, script: &ScriptDispatch, id: i64, online: bool) {
    peer.methods.clear();
    peer.until(script, |peer| {
        peer.methods.iter().any(|(name, args)| {
            if !name.ends_with("onChatAction") {
                return false;
            }
            let Value::Dict(data) = &args[0] else {
                return false;
            };
            if data["action"].as_i64() != Some(19) {
                return false;
            }
            let pickle::Value::Dict(presence) = python(&data["data"]) else {
                return false;
            };
            presence.get(&pickle::Value::I64(id)) == Some(&pickle::Value::Bool(online))
        })
    });
}
