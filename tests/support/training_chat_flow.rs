use super::{Peer, ScriptDispatch, Value};
use std::collections::BTreeMap;
use wot_server::{
    platform::client_formats::pickle, player_sessions::test_support::mercury::script::StringValue,
};

type Args = BTreeMap<std::sync::Arc<str>, Value>;

const CHAT_METHOD: &str = "messenger_onActionByServer_chat2";
const RECEIVED: i16 = 130;
pub const HOST_TEXT: &str = "Réunion sur la carte";

pub fn assert_open(peer: &mut Peer, script: &ScriptDispatch, since: usize) {
    let args = wait_action(peer, script, since, 127, 0);
    assert_eq!(args["int32Arg1"], Value::Int32(2));
    let Value::String(StringValue::Raw(history)) = &args["strArg1"] else {
        panic!("Expected compressed empty chat history");
    };
    assert_eq!(
        pickle::from_reader(flate2::read::ZlibDecoder::new(history.as_slice())).unwrap(),
        pickle::Value::List(vec![])
    );
}

pub fn assert_closed(peer: &mut Peer, script: &ScriptDispatch, since: usize) {
    wait_action(peer, script, since, 128, 0);
}

pub fn assert_outside_room(peer: &mut Peer, script: &ScriptDispatch, request: u16) {
    let since = peer.methods.len();
    send(peer, script, request, "Outside room");
    let args = wait_action(peer, script, since, 1, request);
    assert_eq!(args["int32Arg1"], Value::Int32(2));
}

pub fn exchange(
    host: &mut Peer,
    guest: &mut Peer,
    script: &ScriptDispatch,
    host_id: i64,
    guest_id: i64,
) {
    let since = guest.methods.len();
    send(guest, script, 4802, " ");
    let args = wait_action(guest, script, since, 1, 4802);
    assert_eq!(args["int32Arg1"], Value::Int32(8));

    let since = guest.methods.len();
    send(guest, script, 4807, &"x".repeat(513));
    let args = wait_action(guest, script, since, 1, 4807);
    assert_eq!(args["int32Arg1"], Value::Int32(8));

    let host_since = host.methods.len();
    let guest_since = guest.methods.len();
    send(host, script, 4803, HOST_TEXT);
    wait_action(host, script, host_since, 0, 4803);
    let own = wait_action(host, script, host_since, RECEIVED, 0);
    let received = wait_action(guest, script, guest_since, RECEIVED, 0);
    assert_eq!(own, received);
    assert_message(received, host_id, HOST_TEXT);

    let host_since = host.methods.len();
    let guest_since = guest.methods.len();
    let guest_text = "🟢".repeat(512);
    send(guest, script, 4804, &guest_text);
    wait_action(guest, script, guest_since, 0, 4804);
    let own = wait_action(guest, script, guest_since, RECEIVED, 0);
    let received = wait_action(host, script, host_since, RECEIVED, 0);
    assert_eq!(own, received);
    assert_message(received, guest_id, &guest_text);
}

pub fn assert_no_broadcast(peer: &Peer, since: usize) {
    assert!(action(peer, since, RECEIVED, 0).is_none());
}

pub fn send_after_guest_leaves(host: &mut Peer, script: &ScriptDispatch) {
    std::thread::sleep(std::time::Duration::from_millis(500));
    let since = host.methods.len();
    send(host, script, 4805, "Host is still in the room");
    wait_action(host, script, since, 0, 4805);
    wait_action(host, script, since, RECEIVED, 0);
}

fn assert_message(args: &Args, sender_id: i64, text: &str) {
    assert_eq!(args["int64Arg1"], Value::Int64(sender_id));
    assert_eq!(
        args["strArg1"],
        Value::String(StringValue::String(text.into()))
    );
    assert_ne!(
        args["strArg2"],
        Value::String(StringValue::String("Forged name".into()))
    );
    assert!(matches!(args["floatArg1"], Value::Float64(time) if time > 0.0));
}

fn send(peer: &mut Peer, script: &ScriptDispatch, request: u16, text: &str) {
    let args = Value::Dict(
        [
            ("int32Arg1".into(), Value::Int32(0)),
            ("int64Arg1".into(), Value::Int64(-1)),
            ("floatArg1".into(), Value::Float64(0.0)),
            (
                "strArg1".into(),
                Value::String(StringValue::String(text.into())),
            ),
            (
                "strArg2".into(),
                Value::String(StringValue::String("Forged name".into())),
            ),
            ("int8Arg1".into(), Value::Int8(0)),
        ]
        .into(),
    );
    peer.command(
        script,
        "messenger_onActionByClient_chat2",
        vec![Value::Int16(129), Value::UInt16(request), args],
    );
}

fn wait_action<'a>(
    peer: &'a mut Peer,
    script: &ScriptDispatch,
    since: usize,
    id: i16,
    request: u16,
) -> &'a Args {
    peer.until(script, |peer| action(peer, since, id, request).is_some());
    action(peer, since, id, request).unwrap()
}

fn action(peer: &Peer, since: usize, id: i16, request: u16) -> Option<&Args> {
    peer.methods[since..]
        .iter()
        .find_map(|(method, args)| match args.as_slice() {
            [
                Value::Int16(action),
                Value::UInt16(correlation),
                Value::Dict(data),
            ] if method == CHAT_METHOD && *action == id && *correlation == request => Some(data),
            _ => None,
        })
}
