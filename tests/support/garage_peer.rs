use blowfish::{Blowfish, cipher::KeyInit};
use std::{
    collections::BTreeMap,
    io::{self, Read, Write},
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use wot_server::platform::client_formats::pickle;
use wot_server::player_sessions::test_support::mercury::app::{
    base::element as base,
    client::element as client,
    dispatch::{MethodCall, MethodDef, ScriptDispatch},
    login::element::{LoginRequest, LoginResponse},
};
use wot_server::player_sessions::test_support::mercury::net::{
    bundle::{Bundle, NextElementReader},
    element::{Element, ElementLength, SimpleElement},
    proto::Protocol,
    socket::PacketSocket,
};
use wot_server::player_sessions::test_support::mercury::script::{StringValue, Value};

struct Method<'a> {
    methods: &'a [MethodDef],
    index: u16,
    args: Vec<Value>,
    target: Option<u32>,
}
impl Element<()> for Method<'_> {
    fn write_length(&self, _: &()) -> io::Result<ElementLength> {
        Ok(ElementLength::Variable16)
    }
    fn write(&self, write: &mut dyn Write, _: &()) -> io::Result<u8> {
        let range = if let Some(target) = self.target {
            write.write_all(&target.to_le_bytes())?;
            base::id::CELL_ENTITY_METHOD
        } else {
            base::id::BASE_ENTITY_METHOD
        };
        let (id, sub) = range.from_exposed_id(self.methods.len() as u16, self.index);
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
    pub training_room_visible: Option<bool>,
    pub equipment_upgrades_enabled: bool,
    pub shop_url: Option<String>,
    resources: BTreeMap<u16, BTreeMap<u8, Vec<u8>>>,
    completed: BTreeMap<u16, Vec<u8>>,
    pub methods: Vec<(String, Vec<Value>)>,
    pub responses: Vec<(i16, i16)>,
    pub player_entity_id: u32,
    pub entered_arena: Option<(u64, i32)>,
    pub cell: Option<client::arena::CreateCellPlayer>,
    pub geometry: Option<client::arena::GeometryMapping>,
    pub arena_entities: Vec<client::arena::CreateArenaEntity>,
    pub arena_roster: Option<Vec<u8>>,
    pub resets: usize,
    pub positions: BTreeMap<u32, glam::Vec3>,
    pub vehicle_methods: Vec<(u32, String, Vec<Value>)>,
    selected_entity: Option<u32>,
    player_type: Option<u16>,
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
            training_room_visible: None,
            equipment_upgrades_enabled: false,
            shop_url: None,
            resources: BTreeMap::new(),
            completed: BTreeMap::new(),
            methods: Vec::new(),
            responses: Vec::new(),
            player_entity_id: 0,
            entered_arena: None,
            cell: None,
            geometry: None,
            arena_entities: Vec::new(),
            arena_roster: None,
            resets: 0,
            positions: BTreeMap::new(),
            vehicle_methods: Vec::new(),
            selected_entity: None,
            player_type: None,
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
        success: wot_server::player_sessions::test_support::mercury::app::login::element::LoginSuccess,
    ) {
        self.request_handoff(&success);
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

    pub fn assert_handoff_rejected(
        &mut self,
        ticket: &wot_server::player_sessions::test_support::mercury::app::login::element::LoginSuccess,
    ) {
        self.request_handoff(ticket);
        self.socket
            .set_recv_timeout(Some(Duration::from_millis(250)))
            .unwrap();
        let error = self
            .socket
            .recv()
            .expect_err("Superseded ticket received a BaseApp reply");
        assert!(matches!(
            error.kind(),
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
        ));
    }

    fn request_handoff(
        &mut self,
        success: &wot_server::player_sessions::test_support::mercury::app::login::element::LoginSuccess,
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
        let entity = dispatch.entity_from_id(self.player_type.unwrap()).unwrap();
        self.call(&entity.base_methods, name, args, None);
    }

    pub fn cell_command(&mut self, dispatch: &ScriptDispatch, name: &str, args: Vec<Value>) {
        let avatar = dispatch.entity_from_id(self.player_type.unwrap()).unwrap();
        self.call(&avatar.cell_methods, name, args, Some(0));
    }

    pub fn vehicle_command(
        &mut self,
        dispatch: &ScriptDispatch,
        target: u32,
        name: &str,
        args: Vec<Value>,
    ) {
        let vehicle = dispatch.entity_from_name("Vehicle").unwrap().1;
        self.call(&vehicle.cell_methods, name, args, Some(target));
    }

    fn call(&mut self, methods: &[MethodDef], name: &str, args: Vec<Value>, target: Option<u32>) {
        let index = methods.iter().position(|m| m.name.ends_with(name)).unwrap() as u16;
        let mut bundle = self.authenticated_bundle();
        bundle
            .element_writer()
            .write_simple(Method {
                methods,
                index,
                args,
                target,
            })
            .unwrap();
        self.send(bundle);
    }

    pub fn enable_entities(&mut self) {
        let mut bundle = self.authenticated_bundle();
        bundle
            .element_writer()
            .write_simple(base::EnableEntities {})
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
            let Ok(mut channel) = self.protocol.accept(packet, addr) else {
                continue;
            };
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
                            self.player_entity_id = creation.entity_id;
                            self.player_type = Some(creation.entity_type_id);
                            if creation.entity_type_id
                                == dispatch.entity_from_name("Avatar").unwrap().0
                            {
                                let Value::UInt64(arena) = properties["arenaUniqueID"] else {
                                    panic!("arena ID")
                                };
                                let Value::Int32(map) = properties["arenaTypeID"] else {
                                    panic!("arena type")
                                };
                                assert_eq!(properties["arenaBonusType"], Value::UInt8(2));
                                assert_eq!(properties["arenaGuiType"], Value::UInt8(2));
                                self.entered_arena = Some((arena, map));
                                continue;
                            }
                            let (version_property, required_version) =
                                super::account_version(dispatch);
                            assert_eq!(
                                properties[version_property],
                                Value::String(StringValue::String(required_version.to_owned()))
                            );
                            let Value::Python(
                                wot_server::player_sessions::test_support::mercury::script::PythonValue::Decoded(
                                    pickle::Value::Dict(settings),
                                ),
                            ) = &properties["initialServerSettings"]
                            else {
                                panic!("Account initial server settings")
                            };
                            self.training_room_visible = match settings
                                .get(&pickle::Value::from("isTrainingBattleEnabled"))
                            {
                                Some(pickle::Value::Bool(visible)) => Some(*visible),
                                _ => None,
                            };
                            let pickle::Value::Dict(moderation) =
                                &settings[&pickle::Value::from("pre_moderation_config")]
                            else {
                                panic!("Prebattle description settings");
                            };
                            assert_eq!(
                                moderation[&pickle::Value::from("prebattleDescriptionEnabled")],
                                pickle::Value::Bool(
                                    self.training_room_visible
                                        .expect("Training Room visibility")
                                ),
                            );
                            self.equipment_upgrades_enabled = matches!(
                                settings.get(&pickle::Value::from("isTrophyDevicesEnabled")),
                                Some(pickle::Value::Bool(true))
                            );
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
                            let pickle::Value::Dict(shop) = &settings[&pickle::Value::from("shop")]
                            else {
                                panic!("shop settings")
                            };
                            assert_eq!(
                                shop[&pickle::Value::from("isStorageEnabled")],
                                pickle::Value::Bool(true)
                            );
                            self.shop_url = shop.get(&pickle::Value::from("hostUrl")).map(|url| {
                                let pickle::Value::Bytes(bytes) = url else {
                                    panic!("shop hostUrl must be a Python string")
                                };
                                String::from_utf8(bytes.clone()).unwrap()
                            });
                            let browser_allowlist =
                                settings.get(&pickle::Value::from("igbWhitelist"));
                            if let Some(url) = &self.shop_url {
                                let url: axum::http::Uri =
                                    url.split('#').next().unwrap().parse().unwrap();
                                let host = url.host().unwrap().to_ascii_lowercase();
                                assert_eq!(
                                    browser_allowlist,
                                    Some(&wot_server::pickle!({
                                        "isEnabled": true,
                                        "defaultWhitelist": {(host.as_str())},
                                        "whitelistUrl": "",
                                    }))
                                );
                            } else {
                                assert!(browser_allowlist.is_none());
                            }
                            assert_eq!(creation.entity_type_id, 1);
                            self.created = true;
                        }
                        client::arena::ResetEntities::ID => {
                            let reset = element
                                .read_simple::<client::arena::ResetEntities>()
                                .unwrap()
                                .element;
                            assert!(!reset.keep_player_on_base);
                            self.player_type = None;
                            self.selected_entity = None;
                            self.created = false;
                            self.shown = false;
                            self.entered_arena = None;
                            self.cell = None;
                            self.geometry = None;
                            self.arena_entities.clear();
                            self.arena_roster = None;
                            self.resets += 1;
                        }
                        client::arena::CreateCellPlayer::ID => {
                            self.cell = Some(
                                element
                                    .read_simple::<client::arena::CreateCellPlayer>()
                                    .unwrap()
                                    .element,
                            );
                        }
                        client::arena::GeometryMapping::ID => {
                            self.geometry = Some(
                                element
                                    .read_simple::<client::arena::GeometryMapping>()
                                    .unwrap()
                                    .element,
                            );
                        }
                        client::arena::EnterArenaEntity::ID => {
                            element
                                .read_simple::<client::arena::EnterArenaEntity>()
                                .unwrap();
                        }
                        client::arena::LeaveArenaEntity::ID => {
                            element
                                .read_simple::<client::arena::LeaveArenaEntity>()
                                .unwrap();
                        }
                        client::arena::CreateArenaEntity::ID => {
                            self.arena_entities.push(
                                element
                                    .read_simple::<client::arena::CreateArenaEntity>()
                                    .unwrap()
                                    .element,
                            );
                        }
                        client::arena::NrlCreateNode::ID => {
                            self.arena_roster = Some(
                                element
                                    .read_simple::<client::arena::NrlCreateNode>()
                                    .unwrap()
                                    .element
                                    .0,
                            );
                        }
                        client::arena::SpaceProperty::ID => {
                            element
                                .read_simple::<client::arena::SpaceProperty>()
                                .unwrap();
                        }
                        client::id::SELECT_ENTITY => {
                            self.selected_entity = Some(
                                element
                                    .read_simple::<client::SelectEntity>()
                                    .unwrap()
                                    .element
                                    .entity_id,
                            );
                        }
                        client::arena::VehiclePosition::ID => {
                            let update = element
                                .read_simple::<client::arena::VehiclePosition>()
                                .unwrap()
                                .element;
                            self.positions.insert(update.entity_id, update.position);
                        }
                        client::arena::NrlData::ID => {
                            element.read_simple::<client::arena::NrlData>().unwrap();
                        }
                        client::arena::ApplyNrlUpdate::ID => {
                            element
                                .read_simple::<client::arena::ApplyNrlUpdate>()
                                .unwrap();
                        }
                        client::arena::ArenaEntityProperties::ID => {
                            element
                                .read_simple::<client::arena::ArenaEntityProperties>()
                                .unwrap();
                        }
                        client::id::SELECT_PLAYER_ENTITY => {
                            element.read_simple::<client::SelectPlayerEntity>().unwrap();
                            self.selected_entity = None;
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
                            let methods = if self.selected_entity.is_some() {
                                &dispatch
                                    .entity_from_name("Vehicle")
                                    .unwrap()
                                    .1
                                    .client_methods
                            } else {
                                &dispatch
                                    .entity_from_id(self.player_type.unwrap())
                                    .unwrap()
                                    .client_methods
                            };
                            let method = element
                                .read::<client::EntityMethod, _>(methods.as_slice())
                                .unwrap()
                                .element;
                            if let MethodCall::Known { name, args, .. } = method.call {
                                if let Some(target) = self.selected_entity {
                                    self.vehicle_methods.push((target, name.to_string(), args));
                                    continue;
                                }
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
    pub fn clear_resource(&mut self, id: u16) {
        self.resources.remove(&id);
        self.completed.remove(&id);
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
