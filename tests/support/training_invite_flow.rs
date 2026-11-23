use super::{Peer, Pickle, ScriptDispatch, Value, dict, pickle_value};
use wot_platform::mercury::script::StringValue;

pub const MESSAGE: &str = "Rendez-vous à Studzianki";
const PERIPHERY: i64 = 250;

pub fn sync(peer: &mut Peer, script: &ScriptDispatch) {
    peer.command(script, "doCmdInt3", command(219, 100, 0, 0));
    peer.until(script, |peer| peer.has_resource(219));
}

pub fn send(host: &mut Peer, script: &ScriptDispatch, ids: &[i64], message: &str) {
    host.command(
        script,
        "accountPrebattle_sendPrebattleInvites",
        vec![
            Value::Seq(ids.iter().copied().map(Value::Int64).collect()),
            Value::String(StringValue::String(message.into())),
        ],
    );
}

pub fn assert_received(
    peer: &mut Peer,
    script: &ScriptDispatch,
    since: usize,
    room_id: i64,
    host_id: i64,
    message: &str,
) {
    let invite = wait_state(peer, script, since, room_id, 1);
    assert_eq!(dict(&invite, "type"), Pickle::I64(2));
    assert_eq!(dict(&invite, "creatorDBID"), Pickle::I64(host_id));
    assert_eq!(dict(&invite, "comment"), Pickle::from(message));
    assert!(matches!(dict(&invite, "createTime"), Pickle::F64(sent) if sent > 0.0));
}

pub fn accept(peer: &mut Peer, script: &ScriptDispatch, room_id: i64) {
    let since = peer.methods.len();
    peer.command(script, "doCmdInt3", command(202, 417, room_id, PERIPHERY));
    wait_state(peer, script, since, room_id, 2);
}

pub fn decline(peer: &mut Peer, script: &ScriptDispatch, room_id: i64) {
    let since = peer.methods.len();
    peer.command(script, "doCmdInt3", command(202, 418, room_id, PERIPHERY));
    wait_state(peer, script, since, room_id, 3);
}

pub fn assert_accept_rejected(peer: &mut Peer, script: &ScriptDispatch, room_id: i64, reason: u8) {
    let since = peer.methods.len();
    peer.command(script, "doCmdInt3", command(202, 417, room_id, PERIPHERY));
    peer.until(script, |peer| {
        peer.methods[since..]
            .iter()
            .any(|(name, args)| name == "onPrebattleJoinFailure" && args == &[Value::UInt8(reason)])
    });
}

pub fn assert_expired(peer: &mut Peer, script: &ScriptDispatch, since: usize, room_id: i64) {
    wait_state(peer, script, since, room_id, 4);
}

fn wait_state(
    peer: &mut Peer,
    script: &ScriptDispatch,
    since: usize,
    room_id: i64,
    state: i64,
) -> Pickle {
    let expected_key = wot_platform::pickle!(((room_id), (PERIPHERY)));
    let revision = dict(&peer.resource(219), "rev");
    let delta = |peer: &Peer| {
        peer.methods[since..].iter().find_map(|(method, args)| {
            if method != "update" {
                return None;
            }
            let Pickle::Dict(update) = pickle_value(&args[0]) else {
                return None;
            };
            let Pickle::Dict(invites) = update.get(&Pickle::from("prebattleInvites"))? else {
                return None;
            };
            let invite = invites.get(&expected_key)?;
            if dict(invite, "state") != Pickle::I64(state) {
                return None;
            }
            assert_eq!(update[&Pickle::from("rev")], revision);
            assert_eq!(update[&Pickle::from("prevRev")], revision);
            Some(invite.clone())
        })
    };
    peer.until(script, |peer| delta(peer).is_some());
    delta(peer).unwrap()
}

fn command(request: i16, cmd: i16, first: i64, second: i64) -> Vec<Value> {
    vec![
        Value::Int16(request),
        Value::Int16(cmd),
        Value::Int64(first),
        Value::Int64(second),
        Value::Int64(0),
    ]
}
