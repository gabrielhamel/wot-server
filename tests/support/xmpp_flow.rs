use crate::garage_peer::Peer;
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    time::Duration,
};
use wot_server::platform::client_formats::pickle;
use wot_server::player_sessions::test_support::mercury::app::dispatch::ScriptDispatch;
use wot_server::player_sessions::test_support::mercury::script::{PythonValue, StringValue, Value};

struct Chat {
    stream: TcpStream,
    buffer: String,
}
impl Chat {
    fn connect(address: SocketAddr, owner: i64, token: &str) -> Self {
        let stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(15)))
            .unwrap();
        let mut peer = Self {
            stream,
            buffer: String::new(),
        };
        peer.write("<stream:stream to='wot-server.local' xmlns='jabber:client' xmlns:stream='http://etherx.jabber.org/streams' version='1.0'>");
        assert!(peer.until("</stream:features>").contains("PLAIN"));
        let credential = STANDARD.encode(format!("\0{owner}\0{token}"));
        peer.write(&format!(
            "<auth xmlns='urn:ietf:params:xml:ns:xmpp-sasl' mechanism='PLAIN'>{credential}</auth>"
        ));
        assert!(peer.until("/>").contains("success"));
        peer.write("<stream:stream to='wot-server.local' xmlns='jabber:client' xmlns:stream='http://etherx.jabber.org/streams' version='1.0'>");
        peer.until("</stream:features>");
        peer.write("<iq type='set' id='bind'><bind xmlns='urn:ietf:params:xml:ns:xmpp-bind'><resource>wot</resource></bind></iq>");
        assert!(
            peer.until("</iq>")
                .contains(&format!("{owner}@wot-server.local/wot"))
        );
        peer.write("<iq type='set' id='session'><session xmlns='urn:ietf:params:xml:ns:xmpp-session'/></iq>");
        assert!(peer.until("</iq>").contains("type='result'"));
        peer.write("<iq type='get' id='roster'><query xmlns='http://wargaming.net/xmpp#filter-roster'/></iq>");
        let roster = peer.until("</iq>");
        assert!(
            roster.contains("type='result'") && roster.contains("#filter-roster"),
            "{roster}"
        );
        peer.write("<iq type='get' id='blocks'><blocklist xmlns='urn:xmpp:blocking'/></iq>");
        assert!(peer.until("</iq>").contains("blocklist"));
        peer.write("<iq type='get' id='notes'><query xmlns='http://wargaming.net/xmpp#storage'><list name='contact-notes'/></query></iq>");
        assert!(peer.until("</iq>").contains("contact-notes"));
        peer.write(&format!("<presence/><presence><wgexts-client xmlns='http://wargaming.net/xmpp#client'><game-host>cluster-{owner}.local:20016</game-host></wgexts-client></presence>"));
        peer
    }
    fn write(&mut self, xml: &str) {
        self.stream.write_all(xml.as_bytes()).unwrap();
    }
    fn close(mut self) {
        self.write("</stream:stream>");
        std::io::copy(&mut self.stream, &mut std::io::sink())
            .expect("XMPP stream should close after the closing tag");
    }
    #[track_caller]
    fn until(&mut self, marker: &str) -> String {
        loop {
            if let Some(end) = self.buffer.find(marker) {
                return self.buffer.drain(..end + marker.len()).collect();
            }
            let mut bytes = [0; 8192];
            let length = self.stream.read(&mut bytes).unwrap_or_else(|error| {
                panic!("XMPP read failed while waiting for {marker}: {error}")
            });
            assert_ne!(
                length, 0,
                "XMPP disconnected while waiting for {marker}; received {}",
                self.buffer
            );
            self.buffer
                .push_str(std::str::from_utf8(&bytes[..length]).unwrap());
            assert!(self.buffer.len() < 1024 * 1024);
        }
    }
    fn history(&mut self, peer: i64, id: &str) -> String {
        self.write(&format!("<iq type='get' id='{id}'><query xmlns='http://wargaming.net/xmpp#private-message-history' with='{peer}@wot-server.local' limit='100'/></iq>"));
        self.write("<iq type='get' id='after-history'><ping xmlns='urn:xmpp:ping'/></iq>");
        let response = self.until("id='after-history' from='wot-server.local'></iq>");
        let (_, history) = response
            .split_once(&format!(
                "<iq type='result' id='{id}' from='wot-server.local'></iq>"
            ))
            .unwrap_or_else(|| panic!("Missing history IQ response: {response}"));
        assert_eq!(
            history
                .matches(&format!("request-id='{id}' final='true'"))
                .count(),
            1
        );
        assert!(!history.contains("type='error'"), "{response}");
        for item in history
            .split("<message ")
            .skip(1)
            .filter(|item| item.contains("<private-history "))
        {
            assert!(item.contains("from='wot-server.local'"), "{item}");
        }
        response
    }
    fn message(&mut self, peer: i64, text: &str) {
        self.write(&format!(
            "<message type='chat' to='{peer}@wot-server.local'><body>{text}</body></message>"
        ));
    }

    fn before_ping(&mut self) -> String {
        self.write("<iq type='get' id='barrier'><ping xmlns='urn:xmpp:ping'/></iq>");
        self.until("id='barrier' from='wot-server.local'></iq>")
    }
}

fn assert_peer_replay(replay: &str, peer_id: i64, peer_name: &str) {
    let messages: Vec<_> = replay.split("<message ").skip(1).collect();
    assert!(!messages.is_empty(), "{replay}");
    for stanza in messages {
        assert!(
            stanza.contains(&format!("from='{peer_id}@wot-server.local'"))
                && stanza.contains(&format!("<userid>{peer_id}</userid>"))
                && stanza.contains(&format!("<nickname>{peer_name}</nickname>")),
            "{stanza}"
        );
    }
}
fn token(peer: &mut Peer, script: &ScriptDispatch, request: u16) -> String {
    peer.command(
        script,
        "requestToken",
        vec![Value::UInt16(request), Value::UInt8(1)],
    );
    peer.until(script, |p| {
        p.methods.iter().any(|(name, args)| {
            name.ends_with("onTokenReceived") && args[0].as_i64() == Some(i64::from(request))
        })
    });
    let (_, args) = peer
        .methods
        .iter()
        .find(|(name, args)| {
            name.ends_with("onTokenReceived") && args[0].as_i64() == Some(i64::from(request))
        })
        .unwrap();
    let data = match &args[2] {
        Value::Python(PythonValue::Decoded(pickle::Value::Dict(data)))
        | Value::String(StringValue::Python(pickle::Value::Dict(data))) => data,
        _ => panic!("Expected chat token dictionary"),
    };
    let pickle::Value::Bytes(bytes) = &data[&pickle::Value::Bytes(b"token".to_vec())] else {
        panic!()
    };
    String::from_utf8(bytes.clone()).unwrap()
}

pub fn exercise_game_disconnect(
    game: &mut Peer,
    script: &ScriptDispatch,
    address: SocketAddr,
    account: i64,
) {
    let token = token(game, script, 4003);
    let mut chat = Chat::connect(address, account, &token);
    chat.before_ping();
    let chat_peer = chat.stream.local_addr().unwrap();
    game.disconnect();
    chat.stream.read_to_end(&mut Vec::new()).unwrap();
    crate::telemetry_capture::assert_xmpp_disconnect(chat_peer, account, "session_revoked");

    let mut malformed = TcpStream::connect(address).unwrap();
    malformed
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let malformed_peer = malformed.local_addr().unwrap();
    malformed.write_all(b"</unexpected>").unwrap();
    malformed.read_to_end(&mut Vec::new()).unwrap();
    crate::telemetry_capture::assert_xmpp_connection_failure(
        malformed_peer,
        "Unexpected XMPP closing element",
    );
}

pub fn exercise(
    a: &mut Peer,
    b: &mut Peer,
    script: &ScriptDispatch,
    address: SocketAddr,
    a_id: i64,
    b_id: i64,
) {
    let a_token = token(a, script, 4001);
    let b_token = token(b, script, 4002);
    let mut rejected = Chat {
        stream: TcpStream::connect(address).unwrap(),
        buffer: String::new(),
    };
    rejected
        .stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    rejected.write("<stream:stream to='wot-server.local' xmlns='jabber:client' xmlns:stream='http://etherx.jabber.org/streams' version='1.0'>");
    rejected.until("</stream:features>");
    rejected.write(&format!(
        "<auth xmlns='urn:ietf:params:xml:ns:xmpp-sasl' mechanism='PLAIN'>{}</auth>",
        STANDARD.encode(format!("\0{a_id}\0{b_token}"))
    ));
    assert!(rejected.until("</failure>").contains("not-authorized"));
    rejected.close();
    let mut first = Chat::connect(address, a_id, &a_token);
    first.write("<unknown id='unsupported-stanza'/>");
    first.write("<iq type='set' id='unsupported-storage'><query xmlns='http://wargaming.net/xmpp#storage'/></iq>");
    assert!(first.until("</iq>").contains("feature-not-implemented"));
    first.write("<iq type='get' id='unsupported-query'><query xmlns='urn:wot:test:unknown'/></iq>");
    assert!(first.until("</iq>").contains("feature-not-implemented"));
    first.write("<iq type='get' id='search'><nickname-prefix-search xmlns='http://wargaming.net/xmpp#spa-resolver' prefix='TestPlayerB' limit='50'/></iq>");
    let search = first.until("</iq>");
    assert!(search.contains(&format!("id='{b_id}' nickname='TestPlayerB'")));
    first.write(&format!("<iq type='set' id='add'><query xmlns='jabber:iq:roster'><item jid='{b_id}@wot-server.local' name='ForgedName'/></query></iq>"));
    first.until("</iq>");
    let roster_push = first.until("</iq>");
    assert!(roster_push.contains("name='TestPlayerB'"));
    assert!(!roster_push.contains("ForgedName"));
    assert!(roster_push.contains("subscription='none'") && !roster_push.contains("ask="));
    first.write(&format!(
        "<presence type='subscribe' to='{b_id}@wot-server.local'/>"
    ));
    assert!(first.until("</iq>").contains("ask='subscribe'"));
    first.write("<iq type='get' id='saved-roster'><query xmlns='http://wargaming.net/xmpp#filter-roster'/></iq>");
    let roster = first.until("</iq>");
    assert!(
        roster.contains("#filter-roster")
            && roster.contains("name='TestPlayerB'")
            && roster.contains("ask='subscribe'")
    );
    let empty = first.history(b_id, "empty-history");
    assert!(empty.contains("type='chat' id='history-empty-history'"));
    assert!(empty.contains(&format!("to='{a_id}@wot-server.local/wot'")));
    assert!(!empty.contains("<body"));
    first.write(&format!("<message type='chat' from='{b_id}@wot-server.local' to='{b_id}@wot-server.local'><body>Offline &amp; durable é</body></message>"));
    first.write(&format!("<message type='chat' id='too-fast' to='{b_id}@wot-server.local'><body>Rate limited</body></message>"));
    let rejected = first.until("</message>");
    assert!(rejected.contains("id='too-fast'") && rejected.contains("resource-constraint"));
    // An IQ on the same TCP stream is an ordering barrier after persistence.
    first.before_ping();
    let mut second = Chat::connect(address, b_id, &b_token);
    second.until("type='subscribe'");
    assert!(second.until("</presence>").contains("TestPlayerA"));
    let announcement = second.until("</message>");
    assert!(announcement.contains(&format!("from='{a_id}@wot-server.local'")));
    assert!(announcement.contains("<nickname>TestPlayerA</nickname>"));
    assert!(
        announcement.contains("Offline &amp; durable é"),
        "{announcement}"
    );
    second.write(&format!(
        "<presence type='unsubscribed' to='{a_id}@wot-server.local'/>"
    ));
    let declined = first.until("</iq>");
    assert!(declined.contains("subscription='none'") && !declined.contains("ask="));
    first.write(&format!(
        "<presence type='subscribe' to='{b_id}@wot-server.local'/>"
    ));
    assert!(first.until("</iq>").contains("ask='subscribe'"));
    second.until("type='subscribe'");
    assert!(second.until("</presence>").contains("TestPlayerA"));
    second.write(&format!(
        "<presence type='subscribed' to='{a_id}@wot-server.local'/>"
    ));
    assert!(first.until("</iq>").contains("subscription='both'"));
    let online = first.until("</wgexts-client></presence>");
    assert!(online.contains(&format!("from='{b_id}@wot-server.local/wot'")));
    assert!(online.contains(&format!(
        "<game-host>cluster-{a_id}.local:20016</game-host>"
    )));
    let response = second.history(a_id, "offline-history");
    assert!(response.contains("Offline &amp; durable é"));
    let message_id = announcement
        .split_once("uuid='")
        .unwrap()
        .1
        .split_once('\'')
        .unwrap()
        .0;
    assert!(response.contains(&format!("uuid='{message_id}'")));
    let restored = announcement;
    assert!(!restored.contains("Rate limited"));
    assert!(restored.contains("Offline &amp; durable é"), "{restored}");
    assert!(restored.contains(&format!("<userid>{a_id}</userid>")));
    assert!(restored.contains("<nickname>TestPlayerA</nickname>"));
    assert!(restored.contains(&format!("from='{a_id}@wot-server.local'")));

    // Reopening a sent-only conversation still introduces the peer.
    let first_address = first.stream.local_addr().unwrap();
    first.close();
    let offline = second.until("type='unavailable'");
    assert!(offline.contains(&format!("from='{a_id}@wot-server.local/wot'")));
    let mut first = Chat::connect(address, a_id, &a_token);
    crate::telemetry_capture::assert_xmpp_disconnect(first_address, a_id, "peer_closed");
    let replay = first.before_ping();
    assert_peer_replay(&replay, b_id, "TestPlayerB");
    assert!(!replay.contains("<body"), "{replay}");
    assert!(
        first
            .history(b_id, "sent-history")
            .contains("Offline &amp; durable é")
    );

    second.message(a_id, "Online reply");
    assert!(first.until("</message>").contains("Online reply"));
    first.close();
    let offline = second.until("type='unavailable'");
    assert!(offline.contains(&format!("from='{a_id}@wot-server.local/wot'")));
    std::thread::sleep(Duration::from_millis(500));
    second.message(a_id, "Offline reply");
    assert!(!second.before_ping().contains("type='error'"));
    let mut first = Chat::connect(address, a_id, &a_token);
    // The oldest saved author is the owner; every login replay stanza must
    // still identify the peer, because the first one initializes the window.
    let replay = first.before_ping();
    assert_peer_replay(&replay, b_id, "TestPlayerB");
    assert!(replay.contains("Online reply"));
    assert!(replay.contains("Offline reply"));
    assert!(!replay.contains("Offline &amp; durable é"));

    let history = first.history(b_id, "reconnect-history");
    assert!(history.contains("Offline &amp; durable é"));
    assert!(history.contains("Online reply"));
    assert!(history.contains("Offline reply"));
    assert!(history.contains("request-id='reconnect-history' final='false'"));
    assert!(history.contains(&format!(
        "<body>Offline &amp; durable é</body><wgexts xmlns='http://wargaming.net/xmpp#v2'><userid>{a_id}</userid><nickname>TestPlayerA</nickname>"
    )));
    assert!(history.contains(&format!(
        "<body>Offline reply</body><wgexts xmlns='http://wargaming.net/xmpp#v2'><userid>{b_id}</userid><nickname>TestPlayerB</nickname>"
    )));
    second.write(&format!("<iq type='set' id='block-name'><block xmlns='urn:xmpp:blocking'><item jid='{a_id}@wot-server.local'><nickname>ForgedName</nickname></item></block></iq>"));
    assert!(second.until("</iq>").contains("type='result'"));
    let block_push = second.until("</iq>");
    assert!(block_push.contains("<nickname>TestPlayerA</nickname>"));
    assert!(!block_push.contains("ForgedName"));
    second.write("<iq type='get' id='saved-blocks'><blocklist xmlns='urn:xmpp:blocking'/></iq>");
    let blocklist = second.until("</iq>");
    assert!(blocklist.contains("<nickname>TestPlayerA</nickname>"));
    assert!(blocklist.contains(&format!("<userid>{a_id}</userid>")));

    let blocked_history = first.history(b_id, "blocked-history");
    assert!(!blocked_history.contains("<body"), "{blocked_history}");
    let blocker_history = second.history(a_id, "blocker-history");
    assert!(!blocker_history.contains("<body"), "{blocker_history}");

    first.message(b_id, "Blocked message must not persist");
    first.write("<iq type='get' id='after-blocked'><ping xmlns='urn:xmpp:ping'/></iq>");
    let sender_reply = first.until("id='after-blocked' from='wot-server.local'></iq>");
    assert!(
        !sender_reply.contains("<message type='error'"),
        "{sender_reply}"
    );
    second.write(&format!("<iq type='set' id='unblock-name'><unblock xmlns='urn:xmpp:blocking'><item jid='{a_id}@wot-server.local'/></unblock></iq>"));
    assert!(
        !second
            .until("</iq>")
            .contains("Blocked message must not persist")
    );
    assert!(
        second
            .until("</iq>")
            .contains("<nickname>TestPlayerA</nickname>")
    );
    let unblocked_history = first.history(b_id, "unblocked-history");
    assert!(unblocked_history.contains("Online reply"));
    assert!(!unblocked_history.contains("Blocked message must not persist"));

    second.write(&format!("<iq type='set' id='re-add'><query xmlns='jabber:iq:roster'><item jid='{a_id}@wot-server.local'/></query></iq>"));
    assert!(second.until("</iq>").contains("type='result'"));
    assert!(second.until("</iq>").contains("name='TestPlayerA'"));
    first.close();
    second.close();
    let mut malformed = TcpStream::connect(address).unwrap();
    malformed
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let malformed_peer = malformed.local_addr().unwrap();
    malformed
        .write_all(b"<!DOCTYPE a [<!ENTITY b 'bad'>]><a/>")
        .unwrap();
    let mut byte = [0];
    match malformed.read(&mut byte) {
        Ok(0) => {}
        Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
        other => panic!("Malformed XML connection was not rejected: {other:?}"),
    }
    // A malformed client must not prevent a new authenticated connection.
    let mut final_peer = Chat::connect(address, a_id, &a_token);
    crate::telemetry_capture::assert_xmpp_connection_failure(
        malformed_peer,
        "XMPP DTD is prohibited",
    );
    assert!(
        final_peer
            .history(b_id, "after-malformed")
            .contains("Online reply")
    );
    final_peer.write(&format!("<iq type='set' id='remove'><query xmlns='jabber:iq:roster'><item jid='{b_id}@wot-server.local' subscription='remove'/></query></iq>"));
    final_peer.until("</iq>");
    assert!(final_peer.until("</iq>").contains("subscription='remove'"));
    final_peer.close();
}
