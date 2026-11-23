use blowfish::{Blowfish, cipher::KeyInit};
use std::{
    io::Write,
    time::{Duration, Instant},
};
use wot_platform::mercury::app::{client::element::ResourceFragment, login::element::Ping};
use wot_platform::mercury::net::{
    bundle::{Bundle, NextElementReader},
    filter::BlowfishWriter,
    packet::{Packet, flags},
    proto::Protocol,
    socket::{decrypt_packet, encrypt_packet},
};

fn fragment(first: u32, last: u32, number: u32, data: &[u8]) -> Packet {
    let mut packet = Packet::new();
    packet.write_flags(flags::IS_FRAGMENT | flags::HAS_SEQUENCE_NUMBER);
    packet.grow(data.len()).copy_from_slice(data);
    for value in [first, last, number] {
        packet.grow(4).copy_from_slice(&value.to_le_bytes());
    }
    packet
}

fn ping_recovers(protocol: &mut Protocol) {
    let mut packet = Packet::new();
    packet.grow(2).copy_from_slice(&[2, 7]);
    let mut channel = protocol
        .accept(packet, "127.0.0.1:20000".parse().unwrap())
        .unwrap();
    let bundle = channel.next_bundle().expect("valid packet still served");
    let mut reader = bundle.element_reader();
    let Some(NextElementReader::Element(element)) = reader.next_element() else {
        panic!("missing ping")
    };
    assert_eq!(element.read_simple::<Ping>().unwrap().element.num, 7);
}

#[test]
fn full_encrypted_resource_packets_defer_acks_to_later_packets() {
    let sender = "127.0.0.1:20001".parse().unwrap();
    let receiver = "127.0.0.1:20002".parse().unwrap();
    let mut server = Protocol::new();
    let mut client = Protocol::new();
    for _ in 0..256 {
        let mut datagram = Bundle::new();
        datagram.push_empty();
        client.channel(receiver, None).prepare(&mut datagram, true);
        let packet = datagram.into_packets().next().unwrap();
        server.accept(packet, sender).unwrap().next_bundle();
    }

    let mut resource = Bundle::new();
    resource
        .element_writer()
        .write_simple(ResourceFragment {
            id: 1,
            sequence_num: 0,
            last: true,
            data: vec![0x5a; 8192],
        })
        .unwrap();
    server.channel(sender, None).prepare(&mut resource, true);

    let mut followup = Bundle::new();
    followup.push_empty();
    server.channel(sender, None).prepare(&mut followup, false);

    let cipher = Blowfish::new_from_slice(&[7; 16]).unwrap();
    let mut acked = Vec::new();
    let resource_packets = resource.len();
    for (index, packet) in resource
        .into_packets()
        .chain(followup.into_packets())
        .enumerate()
    {
        let encrypted = encrypt_packet(&packet, &cipher).unwrap();
        let clear = decrypt_packet(encrypted, &cipher).unwrap();
        let parsed = clear.read_config_locked().unwrap();
        let acks = parsed.config().single_acks();
        if index == 0 {
            assert!(!acks.is_empty());
            assert!(acks.len() < 255);
        }
        if index == resource_packets {
            assert!(
                !acks.is_empty(),
                "resource must leave ACKs for a later packet"
            );
        }
        acked.extend(acks.iter().rev().map(|sequence| sequence.get()));
    }
    assert_eq!(acked, (0..256).collect::<Vec<_>>());
    ping_recovers(&mut server);
}

#[test]
fn malformed_piggybacks_and_fragment_ranges_are_rejected_then_ping_works() {
    let addr = "127.0.0.1:20001".parse().unwrap();
    let mut protocol = Protocol::new();
    for len in 0..2 {
        let mut packet = Packet::new();
        packet.write_flags(flags::HAS_PIGGYBACKS);
        packet.grow(len).fill(0);
        packet
            .grow(2)
            .copy_from_slice(&(!(len as i16)).to_le_bytes());
        assert!(protocol.accept(packet, addr).is_err());
        ping_recovers(&mut protocol);
    }
    for packet in [
        fragment(10, 11, 12, &[]),
        fragment(10, 11, 9, &[]),
        fragment(1, 1_000_000, 1, &[]),
    ] {
        assert!(protocol.accept(packet, addr).is_err());
        assert_eq!(protocol.transport_usage().1, 0);
        ping_recovers(&mut protocol);
    }
}

#[test]
fn fragment_ranges_stay_consistent_and_assemblies_expire_without_more_packets() {
    let addr = "127.0.0.1:20001".parse().unwrap();
    let mut protocol = Protocol::new();
    assert!(
        protocol
            .accept(fragment(10, 11, 10, &[2]), addr)
            .unwrap()
            .next_bundle()
            .is_none()
    );
    assert!(
        protocol
            .accept(fragment(10, 12, 11, &[99]), addr)
            .unwrap()
            .next_bundle()
            .is_none()
    );
    assert_eq!(protocol.transport_usage().1, 2);
    let bundle = protocol
        .accept(fragment(10, 11, 11, &[7]), addr)
        .unwrap()
        .next_bundle()
        .unwrap();
    let mut reader = bundle.element_reader();
    let Some(NextElementReader::Element(element)) = reader.next_element() else {
        panic!("missing reassembled ping")
    };
    assert_eq!(element.read_simple::<Ping>().unwrap().element.num, 7);
    protocol.accept(fragment(20, 21, 20, &[]), addr).unwrap();
    protocol.maintenance(Instant::now() + Duration::from_secs(11));
    assert_eq!(protocol.transport_usage().1, 0);
    protocol.maintenance(Instant::now() + Duration::from_secs(121));
    assert_eq!(protocol.transport_usage().0, 0);
    ping_recovers(&mut protocol);
}

#[test]
fn reliable_bundle_rejects_a_non_fragment_inside_its_range() {
    let mut protocol = Protocol::new();
    let addr = "127.0.0.1:20001".parse().unwrap();
    let reliable_flags = flags::ON_CHANNEL | flags::IS_RELIABLE | flags::HAS_SEQUENCE_NUMBER;
    let mut first = fragment(0, 1, 0, &[2]);
    first.write_flags(reliable_flags | flags::IS_FRAGMENT);
    assert!(
        protocol
            .accept(first, addr)
            .unwrap()
            .next_bundle()
            .is_none()
    );

    let mut non_fragment = Packet::new();
    non_fragment.write_flags(reliable_flags);
    non_fragment.grow(4).copy_from_slice(&1u32.to_le_bytes());
    assert!(
        protocol
            .accept(non_fragment, addr)
            .unwrap()
            .next_bundle()
            .is_none()
    );
    ping_recovers(&mut protocol);
}

#[test]
fn global_fragment_and_peer_budgets_bound_storage_and_reclaim_capacity() {
    let mut protocol = Protocol::new();
    for port in 10000..11000 {
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        for first in [1, 300, 600, 900, 1200] {
            let _ = protocol.accept(fragment(first, first + 255, first, &[]), addr);
        }
    }
    let (peers, slots, packets) = protocol.transport_usage();
    assert!(peers <= 512 && slots <= 4096 && packets <= 4096);
    ping_recovers(&mut protocol);
    protocol.maintenance(Instant::now() + Duration::from_secs(121));
    assert_eq!(protocol.transport_usage(), (0, 0, 0));
    ping_recovers(&mut protocol);
}

#[test]
fn every_encrypted_padding_value_is_checked_before_setting_packet_length() {
    let cipher = Blowfish::new_from_slice(&[42; 56]).unwrap();
    for length in [8, 16, 24] {
        for padding in 0..=255u8 {
            let mut clear = vec![0; length];
            clear[length - 5..length - 1].copy_from_slice(&[0xef, 0xbe, 0xad, 0xde]);
            clear[length - 1] = padding;
            let mut encrypted = Vec::new();
            {
                let mut writer = BlowfishWriter::new(&mut encrypted, &cipher);
                writer.write_all(&clear).unwrap();
                writer.flush().unwrap();
            }
            let mut packet = Packet::new();
            packet.buf_mut()[4..4 + encrypted.len()].copy_from_slice(&encrypted);
            packet.set_len(4 + encrypted.len());
            let expected = (1..=8).contains(&padding) && length >= padding as usize + 6;
            assert_eq!(decrypt_packet(packet, &cipher).is_ok(), expected);
        }
    }
    let mut original = Packet::new();
    original.grow(2).copy_from_slice(&[2, 7]);
    let encrypted = wot_platform::mercury::net::socket::encrypt_packet(&original, &cipher).unwrap();
    assert_eq!(
        decrypt_packet(encrypted, &cipher).unwrap().slice(),
        original.slice()
    );
}

#[test]
fn bounded_generated_packets_do_not_panic_and_do_not_prevent_recovery() {
    let mut state = 0xcafe_f00d_u64;
    for round in 0..5000 {
        let mut packet = Packet::new();
        let length = 6 + round % 1467;
        for value in &mut packet.buf_mut()[..length] {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *value = state as u8;
        }
        packet.set_len(length);
        let mut protocol = Protocol::new();
        let _ = protocol.accept(packet, "127.0.0.1:20001".parse().unwrap());
        ping_recovers(&mut protocol);
    }
}

#[test]
fn deeply_nested_or_exponentially_shared_pickles_are_rejected() {
    let mut deep = vec![b'N'];
    deep.extend(std::iter::repeat_n(0x85, 1000));
    deep.push(b'.');
    assert!(wot_platform::client_formats::pickle::from_bytes(deep).is_err());
    let mut shared = vec![b'N'];
    for _ in 0..30 {
        shared.extend_from_slice(&[b'2', 0x86]);
    }
    shared.push(b'.');
    assert!(wot_platform::client_formats::pickle::from_bytes(shared).is_err());
    assert_eq!(
        wot_platform::client_formats::pickle::from_bytes(b"N.").unwrap(),
        wot_platform::client_formats::pickle::Value::None
    );
}

#[test]
fn peer_and_reliable_buffer_limits_reject_excess_then_recover() {
    let mut protocol = Protocol::new();
    ping_recovers(&mut protocol);
    for port in 10000..11000 {
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        let mut packet = Packet::new();
        packet.grow(2).copy_from_slice(&[2, 7]);
        let _ = protocol.accept(packet, addr);
    }
    assert_eq!(protocol.transport_usage().0, 512);
    ping_recovers(&mut protocol);
    protocol.maintenance(Instant::now() + Duration::from_secs(121));
    assert_eq!(protocol.transport_usage(), (0, 0, 0));
    let addr = "127.0.0.1:20001".parse().unwrap();
    for number in 2..1000u32 {
        let mut packet = Packet::new();
        packet.write_flags(flags::ON_CHANNEL | flags::IS_RELIABLE | flags::HAS_SEQUENCE_NUMBER);
        packet.grow(4).copy_from_slice(&number.to_le_bytes());
        let _ = protocol.accept(packet, addr);
    }
    assert!(protocol.transport_usage().2 <= 256);
    ping_recovers(&mut protocol);
}

#[test]
fn login_rate_limit_is_checked_before_decoding_and_ping_still_works() {
    use wot_platform::mercury::app::login::{App, Event, element::LoginRequest};
    use wot_platform::mercury::net::{bundle::Bundle, socket::PacketSocket};
    let mut server = App::new("127.0.0.1:0".parse().unwrap()).unwrap();
    let address = server.addr().unwrap();
    let socket = PacketSocket::bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let mut protocol = Protocol::new();
    for attempt in 0..9 {
        let mut bundle = Bundle::new();
        bundle
            .element_writer()
            .write_simple_request(
                LoginRequest {
                    username: "test".into(),
                    password: "test".into(),
                    blowfish_key: vec![42; 56],
                    ..Default::default()
                },
                attempt + 1,
            )
            .unwrap();
        protocol.off_channel(address).prepare(&mut bundle, false);
        socket.send_bundle(&bundle, address).unwrap();
        let event = server.poll_until(Instant::now() + Duration::from_secs(1));
        match (attempt, event) {
            (0..=7, Event::Login(_)) => {}
            (8, Event::IoError(error)) => assert!(error.error.to_string().contains("rate limit")),
            (_, event) => panic!("unexpected event {event:?}"),
        }
    }
    let mut bundle = Bundle::new();
    bundle
        .element_writer()
        .write_simple_request(Ping { num: 7 }, 10)
        .unwrap();
    protocol.off_channel(address).prepare(&mut bundle, false);
    socket.send_bundle(&bundle, address).unwrap();
    assert!(matches!(
        server.poll_until(Instant::now() + Duration::from_secs(1)),
        Event::Ping(_)
    ));
}

#[test]
fn outgoing_reliable_budget_applies_backpressure_and_expires() {
    use wot_platform::mercury::net::bundle::Bundle;
    let mut protocol = Protocol::new();
    let mut bundle = Bundle::new();
    bundle
        .element_writer()
        .write_simple(Ping { num: 7 })
        .unwrap();
    for port in 10000..10008 {
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        for _ in 0..2048 {
            assert!(protocol.can_queue_reliable(addr, 1));
            bundle.clear();
            bundle
                .element_writer()
                .write_simple(Ping { num: 7 })
                .unwrap();
            protocol.channel(addr, None).prepare(&mut bundle, true);
        }
        assert!(!protocol.can_queue_reliable(addr, 1));
    }
    let extra = "127.0.0.1:20002".parse().unwrap();
    assert!(!protocol.can_queue_reliable(extra, 1));
    protocol.maintenance(Instant::now() + Duration::from_secs(31));
    assert!(protocol.can_queue_reliable(extra, 1));
    ping_recovers(&mut protocol);
}
