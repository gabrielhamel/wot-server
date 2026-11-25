use blowfish::{Blowfish, cipher::KeyInit};
use std::{
    collections::BTreeMap,
    io::{self, Read, Write},
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use wot_platform::client_formats::pickle;
use wot_platform::mercury::app::{
    base::element as base,
    client::element as client,
    dispatch::{MethodCall, MethodDef, ScriptDispatch},
    login::element::{LoginRequest, LoginResponse},
};
use wot_platform::mercury::net::{
    bundle::{Bundle, NextElementReader},
    codec::Codec,
    element::{Element, ElementLength, SimpleElement},
    packet::{PACKET_HEADER_LEN, PacketConfig},
    proto::Protocol,
    seq::Seq,
    socket::PacketSocket,
};
use wot_platform::mercury::script::{StringValue, Value};

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
    pub arena_tick: Option<u8>,
    pub avatar_position_frames: usize,
    avatar_locally_controlled: Option<bool>,
    pub nrl_updates: Vec<Vec<u8>>,
    pub last_sent_reliable: Seq,
    pub snapshot_ack_through: Option<Seq>,
    last_unreliable_sequence: Option<Seq>,
    pub vehicle_methods: Vec<(u32, String, Vec<Value>)>,
    pub gun_angles: BTreeMap<u32, u16>,
    pub damage_stickers: BTreeMap<u32, Value>,
    pub hull_aim: Option<u64>,
    pub engine_modes: BTreeMap<u32, [u8; 2]>,
    pub vehicle_directions: BTreeMap<u32, glam::Vec3>,
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
            arena_tick: None,
            avatar_position_frames: 0,
            avatar_locally_controlled: None,
            nrl_updates: Vec::new(),
            last_sent_reliable: Seq::ZERO,
            snapshot_ack_through: None,
            last_unreliable_sequence: None,
            vehicle_methods: Vec::new(),
            gun_angles: BTreeMap::new(),
            damage_stickers: BTreeMap::new(),
            hull_aim: None,
            engine_modes: BTreeMap::new(),
            vehicle_directions: BTreeMap::new(),
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

    pub fn handoff(&mut self, success: wot_platform::mercury::app::login::element::LoginSuccess) {
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
        ticket: &wot_platform::mercury::app::login::element::LoginSuccess,
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
        success: &wot_platform::mercury::app::login::element::LoginSuccess,
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
        let mut config = PacketConfig::new();
        bundle
            .iter()
            .last()
            .unwrap()
            .read_config(&mut config)
            .unwrap();
        self.last_sent_reliable = config.sequence_num();
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
    #[track_caller]
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
            let mut config = PacketConfig::new();
            if packet.read_config(&mut config).is_ok() {
                if config.on_channel() && !config.reliable() {
                    assert!(
                        config.has_sequence_num(),
                        "native client rejects unsequenced on-channel datagrams"
                    );
                    let sequence = config.sequence_num();
                    if self
                        .last_unreliable_sequence
                        .is_some_and(|last| sequence.wrapping_cmp(last).is_lt())
                    {
                        continue;
                    }
                    self.last_unreliable_sequence = Some(sequence);
                }
                if self.cell.is_some() && config.on_channel() && config.cumulative_ack().is_some() {
                    assert!(
                        config.footer_offset() > PACKET_HEADER_LEN,
                        "battle ACKs must share a datagram with outgoing messages"
                    );
                }
            }
            let Ok(mut channel) = self.protocol.accept(packet, addr) else {
                continue;
            };
            for bundle in channel.pop_bundles() {
                let mut reader = bundle.element_reader();
                let mut has_tick = false;
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
                            assert_eq!(
                                creation.entity_components_count,
                                dispatch
                                    .entity_from_name("Account")
                                    .unwrap()
                                    .1
                                    .static_components_count
                            );
                            let (version_property, required_version) =
                                super::account_version(dispatch);
                            assert_eq!(
                                properties[version_property],
                                Value::String(StringValue::String(required_version.to_owned()))
                            );
                            let Value::Python(wot_platform::mercury::script::PythonValue::Decoded(
                                pickle::Value::Dict(settings),
                            )) = &properties["initialServerSettings"]
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
                                    Some(&wot_platform::pickle!({
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
                            self.positions.clear();
                            self.arena_tick = None;
                            self.snapshot_ack_through = None;
                            self.avatar_locally_controlled = None;
                            self.avatar_position_frames = 0;
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
                            self.avatar_locally_controlled = Some(true);
                        }
                        client::arena::ControlEntity::ID => {
                            let control = element
                                .read_simple::<client::arena::ControlEntity>()
                                .unwrap()
                                .element;
                            assert_eq!(control.entity_id, self.player_entity_id);
                            assert!(self.cell.is_some());
                            self.avatar_locally_controlled = Some(control.is_locally_controlled);
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
                            assert!(has_tick, "vehicle pose must share its bundle with TickSync");
                            let update = element
                                .read_simple::<client::arena::VehiclePosition>()
                                .unwrap()
                                .element;
                            self.positions.insert(update.entity_id, update.position);
                            self.vehicle_directions
                                .insert(update.entity_id, update.direction);
                        }
                        client::arena::NrlData::ID => {
                            self.nrl_updates.push(
                                element
                                    .read_simple::<client::arena::NrlData>()
                                    .unwrap()
                                    .element
                                    .0,
                            );
                        }
                        client::arena::AvatarPosition::ID => {
                            assert!(has_tick, "Avatar pose must share its bundle with TickSync");
                            let frame = element
                                .read_simple::<client::arena::AvatarPosition>()
                                .unwrap()
                                .element;
                            assert_eq!(frame.position, glam::Vec3::ZERO);
                            if self.avatar_locally_controlled == Some(false) {
                                self.avatar_position_frames += 1;
                            }
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
                            let tick = element
                                .read_simple::<client::TickSync>()
                                .unwrap()
                                .element
                                .tick;
                            if let Some(last) = self.arena_tick {
                                assert!((1..=127).contains(&tick.wrapping_sub(last)));
                            }
                            self.arena_tick = Some(tick);
                            self.snapshot_ack_through = config.cumulative_ack();
                            has_tick = true;
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
                        id if client::id::ENTITY_PROPERTY.contains(id) => {
                            let name = if self.selected_entity.is_some() {
                                "Vehicle"
                            } else {
                                "Avatar"
                            };
                            let properties = &dispatch.entity_from_name(name).unwrap().1.properties;
                            let index = client::id::ENTITY_PROPERTY
                                .to_exposed_id_checked(properties.len() as u16, id)
                                .unwrap();
                            match &*properties[index as usize].name {
                                "gunAnglesPacked" => {
                                    let update = element
                                        .read::<client::FixedEntityProperty<2>, _>(&())
                                        .unwrap()
                                        .element;
                                    self.gun_angles.insert(
                                        self.selected_entity.unwrap(),
                                        u16::from_le_bytes(update.bytes),
                                    );
                                }
                                "engineMode" => {
                                    let update = element
                                        .read::<client::FixedEntityProperty<2>, _>(&())
                                        .unwrap()
                                        .element;
                                    self.engine_modes
                                        .insert(self.selected_entity.unwrap(), update.bytes);
                                }
                                "damageStickers" => {
                                    let update = element
                                        .read::<client::VariableEntityProperty, _>(&())
                                        .unwrap()
                                        .element;
                                    let mut body = update.bytes.as_slice();
                                    let stickers =
                                        Value::read(&mut body, &properties[index as usize].ty)
                                            .unwrap();
                                    assert!(body.is_empty());
                                    self.damage_stickers
                                        .insert(self.selected_entity.unwrap(), stickers);
                                }
                                "wheelsScroll" | "steeringAngles" => {
                                    let update = element
                                        .read::<client::VariableEntityProperty, _>(&())
                                        .unwrap()
                                        .element;
                                    let (&length, wheels) = update.bytes.split_first().unwrap();
                                    assert_eq!(usize::from(length), wheels.len());
                                }
                                "ownVehicleAuxPhysicsData" => {
                                    let update = element
                                        .read::<client::FixedEntityProperty<8>, _>(&())
                                        .unwrap()
                                        .element;
                                    self.hull_aim = Some(u64::from_le_bytes(update.bytes));
                                }
                                name => panic!("unexpected arena property {name}"),
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
            if config.reliable() {
                let mut ack = Bundle::new();
                ack.push_empty();
                self.protocol
                    .channel(self.base, None)
                    .prepare(&mut ack, false);
                self.socket.send_bundle(&ack, self.base).unwrap();
            }
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
