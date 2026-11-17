use blowfish::{Blowfish, cipher::KeyInit};
use std::{
    collections::BTreeMap,
    io::{self, Read, Write},
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use wot_server::client_formats::pickle;
use wot_server::client_gateway::mercury::app::{
    base::element as base,
    client::element as client,
    dispatch::{MethodCall, MethodDef, ScriptDispatch},
    login::element::{LoginRequest, LoginResponse},
};
use wot_server::client_gateway::mercury::net::{
    bundle::{Bundle, NextElementReader},
    element::{Element, ElementLength},
    proto::Protocol,
    socket::PacketSocket,
};
use wot_server::client_gateway::mercury::script::Value;

struct Method<'a> {
    methods: &'a [MethodDef],
    index: u16,
    args: Vec<Value>,
}
impl Element<()> for Method<'_> {
    fn write_length(&self, _: &()) -> io::Result<ElementLength> {
        Ok(ElementLength::Variable16)
    }
    fn write(&self, write: &mut dyn Write, _: &()) -> io::Result<u8> {
        let (id, sub) =
            base::id::BASE_ENTITY_METHOD.from_exposed_id(self.methods.len() as u16, self.index);
        if let Some(sub) = sub {
            write.write_all(&[sub])?;
        }
        self.methods[self.index as usize].write_args(write, &self.args)?;
        Ok(id)
    }
    fn read_length(_: &(), _: u8) -> io::Result<ElementLength> {
        unreachable!()
    }
    fn read(_: &mut dyn Read, _: &(), _: usize, _: u8) -> io::Result<Self> {
        unreachable!()
    }
}

pub struct Peer {
    socket: PacketSocket,
    protocol: Protocol,
    cipher: Arc<Blowfish>,
    login_key: Option<Arc<rsa::RsaPublicKey>>,
    base: SocketAddr,
    session: u32,
    pub created: bool,
    pub shown: bool,
    resources: BTreeMap<u16, BTreeMap<u8, Vec<u8>>>,
    completed: BTreeMap<u16, Vec<u8>>,
    pub methods: Vec<(String, Vec<Value>)>,
    pub responses: Vec<(i16, i16)>,
}

impl Peer {
    pub fn login(
        login: SocketAddr,
        username: &str,
        password: &str,
        key: Option<Arc<rsa::RsaPublicKey>>,
    ) -> LoginResponse {
        let mut peer = Self::new(key);
        peer.authenticate(login, username, password)
    }

    pub fn new(login_key: Option<Arc<rsa::RsaPublicKey>>) -> Self {
        Self {
            socket: PacketSocket::bind("127.0.0.1:0".parse().unwrap()).unwrap(),
            protocol: Protocol::new(),
            cipher: Arc::new(Blowfish::new_from_slice(&[42; 56]).unwrap()),
            login_key,
            base: "127.0.0.1:0".parse().unwrap(),
            session: 0,
            created: false,
            shown: false,
            resources: BTreeMap::new(),
            completed: BTreeMap::new(),
            methods: Vec::new(),
            responses: Vec::new(),
        }
    }

    pub fn authenticate(
        &mut self,
        login: SocketAddr,
        username: &str,
        password: &str,
    ) -> LoginResponse {
        self.socket
            .set_recv_timeout(Some(Duration::from_secs(15)))
            .unwrap();
        let mut bundle = Bundle::new();
        let request = LoginRequest {
            username: serde_json::json!({
                "login": username, "auth_method": "basic", "session": "test-session",
                "auth_realm": "EU", "temporary": "1", "publication": "0"
            })
            .to_string(),
            password: password.into(),
            context: r#"{"lang_id":"en"}"#.into(),
            blowfish_key: vec![42; 56],
            ..Default::default()
        };
        if let Some(key) = self.login_key.as_deref() {
            bundle
                .element_writer()
                .write_request(request, 1, key)
                .unwrap();
        } else {
            bundle
                .element_writer()
                .write_simple_request(request, 1)
                .unwrap();
        }
        self.protocol.off_channel(login).prepare(&mut bundle, false);
        self.socket.send_bundle(&bundle, login).unwrap();
        let (packet, addr) = self.socket.recv().unwrap();
        let bundle = self
            .protocol
            .accept(packet, addr)
            .unwrap()
            .next_bundle()
            .unwrap();
        let mut reader = bundle.element_reader();
        let Some(NextElementReader::Reply(reply)) = reader.next_element() else {
            panic!("Expected login reply")
        };
        reply.read::<LoginResponse, _>(&*self.cipher).unwrap()
    }

    pub fn connect(&mut self, login: SocketAddr, username: &str, password: &str) {
        let LoginResponse::Success(success) = self.authenticate(login, username, password) else {
            panic!("Login failed")
        };
        self.handoff(success);
    }

    pub fn handoff(
        &mut self,
        success: wot_server::client_gateway::mercury::app::login::element::LoginSuccess,
    ) {
        self.base = success.addr.addr.into();
        let mut bundle = Bundle::new();
        bundle
            .element_writer()
            .write_simple_request(
                base::LoginKey {
                    login_key: success.login_key,
                    attempt_num: 0,
                    unk: 0,
                },
                2,
            )
            .unwrap();
        self.protocol
            .off_channel(self.base)
            .prepare(&mut bundle, false);
        self.socket.send_bundle(&bundle, self.base).unwrap();
        self.socket.set_encryption(self.base, self.cipher.clone());
        let (packet, addr) = self.socket.recv().unwrap();
        let bundle = self
            .protocol
            .accept(packet, addr)
            .unwrap()
            .next_bundle()
            .unwrap();
        let mut reader = bundle.element_reader();
        let Some(NextElementReader::Reply(reply)) = reader.next_element() else {
            panic!("Expected BaseApp login reply")
        };
        self.session = reply.read_simple::<base::SessionKey>().unwrap().session_key;
        let mut bundle = self.authenticated_bundle();
        bundle
            .element_writer()
            .write_simple(base::EnableEntities {})
            .unwrap();
        self.send(bundle);
    }

    fn authenticated_bundle(&self) -> Bundle {
        let mut bundle = Bundle::new();
        bundle
            .element_writer()
            .write_simple(base::SessionKey {
                session_key: self.session,
            })
            .unwrap();
        bundle
    }
    fn send(&mut self, mut bundle: Bundle) {
        self.protocol
            .channel(self.base, None)
            .prepare(&mut bundle, true);
        self.socket.send_bundle(&bundle, self.base).unwrap();
    }
    pub fn command(&mut self, dispatch: &ScriptDispatch, name: &str, args: Vec<Value>) {
        let methods = &dispatch.entity_from_name("Account").unwrap().1.base_methods;
        let index = methods.iter().position(|m| m.name.ends_with(name)).unwrap() as u16;
        let mut bundle = self.authenticated_bundle();
        bundle
            .element_writer()
            .write_simple(Method {
                methods,
                index,
                args,
            })
            .unwrap();
        self.send(bundle);
    }
    pub fn disconnect(&mut self) {
        let mut bundle = self.authenticated_bundle();
        bundle
            .element_writer()
            .write_simple(base::DisconnectClient { reason: 0 })
            .unwrap();
        self.send(bundle);
    }
    pub fn until(&mut self, dispatch: &ScriptDispatch, condition: impl Fn(&Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !condition(self) {
            assert!(
                Instant::now() < deadline,
                "Timed out waiting for garage protocol"
            );
            self.socket
                .set_recv_timeout(Some(Duration::from_millis(500)))
                .unwrap();
            let Ok((packet, addr)) = self.socket.recv() else {
                continue;
            };
            let mut channel = self.protocol.accept(packet, addr).unwrap();
            for bundle in channel.pop_bundles() {
                let mut reader = bundle.element_reader();
                while let Some(next) = reader.next_element() {
                    let NextElementReader::Element(element) = next else {
                        panic!("Unexpected reply");
                    };
                    match element.id() {
                        client::id::UPDATE_FREQUENCY_NOTIFICATION => {
                            element
                                .read_simple::<client::UpdateFrequencyNotification>()
                                .unwrap();
                        }
                        client::id::SET_GAME_TIME => {
                            element.read_simple::<client::SetGameTime>().unwrap();
                        }
                        client::id::CREATE_BASE_PLAYER => {
                            let creation = element
                                .read::<client::CreateBasePlayer<'_>, _>(dispatch)
                                .unwrap()
                                .element;
                            let Value::Dict(properties) = creation.entity_data.as_ref() else {
                                panic!("Account creation properties")
                            };
                            let Value::Python(
                                wot_server::client_gateway::mercury::script::PythonValue::Decoded(
                                    pickle::Value::Dict(settings),
                                ),
                            ) = &properties["initialServerSettings"]
                            else {
                                panic!("Account initial server settings")
                            };
                            for name in [
                                "comp7_config",
                                "comp7_ranks_config",
                                "comp7_light_config",
                                "fun_random_config",
                                "battle_royale_config",
                                "vehicle_post_progression_config",
                            ] {
                                assert!(
                                    settings.contains_key(&pickle::Value::Bytes(
                                        name.as_bytes().to_vec()
                                    )),
                                    "Missing startup/replay config: {name}"
                                );
                            }
                            for (name, expected) in
                                [("rewardBookId", 62), ("XppToConvert", 100_000)]
                            {
                                assert_eq!(
                                    settings.get(&pickle::Value::Bytes(name.as_bytes().to_vec())),
                                    Some(&pickle::Value::I64(expected)),
                                    "Crew post-progression setting: {name}"
                                );
                            }
                            let pickle::Value::Dict(epic) =
                                &settings[&pickle::Value::Bytes(b"epic_config".to_vec())]
                            else {
                                panic!("epic settings")
                            };
                            assert!(
                                epic.contains_key(&pickle::Value::Bytes(b"epicMetaGame".to_vec()))
                            );
                            assert_eq!(
                                settings.get(&pickle::Value::from("shop")),
                                Some(&wot_server::pickle!({"isStorageEnabled": true}))
                            );
                            assert_eq!(creation.entity_type_id, 1);
                            self.created = true;
                        }
                        client::id::SELECT_PLAYER_ENTITY => {
                            element.read_simple::<client::SelectPlayerEntity>().unwrap();
                        }
                        client::id::TICK_SYNC => {
                            element.read_simple::<client::TickSync>().unwrap();
                        }
                        client::id::RESOURCE_HEADER => {
                            element.read_simple::<client::ResourceHeader>().unwrap();
                        }
                        client::id::RESOURCE_FRAGMENT => {
                            let fragment = element
                                .read_simple::<client::ResourceFragment>()
                                .unwrap()
                                .element;
                            let pieces = self.resources.entry(fragment.id).or_default();
                            pieces.insert(fragment.sequence_num, fragment.data);
                            if fragment.last {
                                self.completed.insert(
                                    fragment.id,
                                    pieces.values().flatten().copied().collect(),
                                );
                            }
                        }
                        id if client::id::ENTITY_METHOD.contains(id) => {
                            let methods = &dispatch
                                .entity_from_name("Account")
                                .unwrap()
                                .1
                                .client_methods;
                            let method = element
                                .read::<client::EntityMethod, _>(methods.as_slice())
                                .unwrap()
                                .element;
                            if let MethodCall::Known { name, args, .. } = method.call {
                                self.methods.push((name.to_string(), args.clone()));
                                if name.ends_with("showGUI") {
                                    self.shown = true;
                                }
                                if name.ends_with("onCmdResponse")
                                    || name.ends_with("onCmdResponseExt")
                                {
                                    self.responses.push((
                                        args[0].as_i64().unwrap() as i16,
                                        args[1].as_i64().unwrap() as i16,
                                    ));
                                }
                            }
                        }
                        id => panic!("Unexpected client element {id:02x}"),
                    }
                }
            }
            let mut ack = Bundle::new();
            self.protocol
                .channel(self.base, None)
                .prepare(&mut ack, false);
            self.socket.send_bundle(&ack, self.base).unwrap();
        }
    }
    pub fn has_resource(&self, id: u16) -> bool {
        self.completed.contains_key(&id)
    }
    pub fn resource(&self, id: u16) -> pickle::Value {
        pickle::from_reader(flate2::read::ZlibDecoder::new(
            self.completed[&id].as_slice(),
        ))
        .unwrap()
    }
}
