use rsa::{RsaPrivateKey, RsaPublicKey, rand_core::OsRng};
use std::{
    io,
    sync::Arc,
    time::{Duration, Instant},
};
use wot_platform::mercury::app::login::{App, Event, element::LoginRequest};
use wot_platform::mercury::net::{bundle::Bundle, proto::Protocol, socket::PacketSocket};

#[test]
fn wrong_key_login_is_rejected_and_next_encrypted_login_still_works() {
    let private = Arc::new(RsaPrivateKey::new(&mut OsRng, 1024).unwrap());
    let public = RsaPublicKey::from(&*private);
    let wrong_public = RsaPublicKey::from(&RsaPrivateKey::new(&mut OsRng, 1024).unwrap());
    let mut server = App::new("127.0.0.1:0".parse().unwrap()).unwrap();
    server.set_encryption(private);
    let address = server.addr().unwrap();
    let socket = PacketSocket::bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let mut protocol = Protocol::new();
    for (key, valid) in [(&wrong_public, false), (&public, true)] {
        let mut bundle = Bundle::new();
        bundle
            .element_writer()
            .write_request(
                LoginRequest {
                    username: "test@example.test".into(),
                    password: "fixture".into(),
                    blowfish_key: vec![42; 56],
                    ..Default::default()
                },
                1,
                key,
            )
            .unwrap();
        protocol.off_channel(address).prepare(&mut bundle, false);
        socket.send_bundle(&bundle, address).unwrap();
        let event = server.poll_until(Instant::now() + Duration::from_secs(3));
        match (valid, event) {
            (false, Event::IoError(error)) => {
                assert_eq!(error.error.kind(), io::ErrorKind::InvalidData);
                assert!(
                    error
                        .error
                        .to_string()
                        .contains("RSA login decryption failed")
                );
                assert_eq!(error.addr, Some(socket.addr().unwrap()));
            }
            (true, Event::Login(event)) => {
                assert_eq!(event.request.username, "test@example.test");
                assert_eq!(event.request.password, "fixture");
            }
            (_, event) => panic!("Unexpected login event: {event:?}"),
        }
    }
}
