//! BaseApp handshake, garage Account lifetime and method dispatch.

pub mod element;

use std::{
    borrow::Cow,
    collections::{HashMap, VecDeque},
    fmt,
    io::{self, Read, Write},
    net::SocketAddr,
    num::Wrapping,
    sync::Arc,
    time::{Duration, Instant},
};

use blowfish::Blowfish;
use rsa::rand_core::{OsRng, RngCore};
use tracing::{debug, warn};

use crate::player_sessions::mercury::net::{
    bundle::{Bundle, ElementReader, NextElementReader},
    element::{Element, ElementLength, SimpleElement},
    limits,
    proto::Protocol,
    socket::PacketSocket,
};
use crate::player_sessions::mercury::script::Script;

use super::{
    client,
    dispatch::{MethodCall, ScriptDispatch},
    entity::{
        AnyEntity, BindError, EntityKey, EntitySchema, EntityView, Methods, RawEntity, Registry,
        ScriptType,
    },
    io_invalid_data,
    outbox::{ClientOutbox, Outbox},
    slots::{SlotKey, Slots},
};
use element::{LoginKey, SessionKey};

#[derive(Debug)]
pub struct App {
    /// The scripting model for this app, and its per entity-type dispatch tables
    /// computed from it at construction time.
    dispatch: ScriptDispatch,
    /// The registered entity schemas.
    entity_schemas: Registry,
    /// Internal socket for this application.
    socket: PacketSocket,
    /// The channel tracker.
    protocol: Protocol,
    /// Queue of events that are waiting to be returned.
    events: VecDeque<Event>,
    /// What waits to be sent to each client, until the next poll.
    outbox: Outbox,
    /// A bundle for what is sent right away rather than through the outbox: the login
    /// reply, before the client has a channel, and resource fragments.
    bundle: Bundle,
    /// Clients that have made an initial client connection, associated to the request id.
    pending_clients: HashMap<SocketAddr, (u32, Instant)>,
    /// The logged in clients, and the key of each by its address.
    clients: Slots<ClientState>,
    client_keys: HashMap<SocketAddr, ClientKey>,
    disconnected_clients: Vec<ClientKey>,
    /// Every entity this app has created, and the key of each by its id.
    entities: Slots<EntityState>,
    entity_keys: HashMap<u32, EntityKey<AnyEntity>>,
    /// The next id for entities, this is wrapping around and we ensure that the same id
    /// isn't used twice! Ids are positive signed integers, negative ones being the
    /// client's own entities.
    entities_next_id: Wrapping<u32>,
}

impl App {
    pub fn new(addr: SocketAddr, script: Script) -> io::Result<Self> {
        Ok(Self {
            dispatch: ScriptDispatch::new(script),
            entity_schemas: Registry::default(),
            socket: PacketSocket::bind(addr)?,
            protocol: Protocol::new(),
            events: VecDeque::new(),
            outbox: Outbox::default(),
            bundle: Bundle::new(),
            pending_clients: HashMap::new(),
            clients: Slots::default(),
            client_keys: HashMap::new(),
            disconnected_clients: Vec::new(),
            entities: Slots::default(),
            entity_keys: HashMap::new(),
            entities_next_id: Wrapping(OsRng.next_u32()),
        })
    }

    /// The dispatch tables of the script model this app was created with.
    pub fn dispatch(&self) -> &ScriptDispatch {
        &self.dispatch
    }

    /// Get the address this app is bound to.
    pub fn addr(&self) -> io::Result<SocketAddr> {
        self.socket.addr()
    }

    /// Bind a schema to its entity type of the loaded script, checking every member it
    /// declares, see [`super::entity`]. Its entities can then be created, viewed
    /// through it with [`Self::entity`], and their calls decoded with [`Self::decode`].
    pub fn register<T: EntitySchema>(&mut self) -> Result<(), BindError> {
        self.entity_schemas.register::<T>(&self.dispatch)
    }

    /// Poll for the next event of this app, blocking until one comes.
    ///
    /// What was written to clients since the last poll is sent first, one bundle per
    /// client.
    pub fn poll(&mut self) -> Event {
        self.poll_inner(None)
    }

    /// Same as [`Self::poll`], but returns [`Event::Timeout`] once the deadline passes
    /// with no event, so the caller gets a chance to send periodic messages (ticks) to
    /// quiet clients. A deadline already passed still reads a packet waiting, if any.
    pub fn poll_until(&mut self, deadline: Instant) -> Event {
        self.poll_inner(Some(deadline))
    }

    fn poll_inner(&mut self, deadline: Option<Instant>) -> Event {
        for (addr, error) in self.outbox.flush_all(&mut self.protocol, &self.socket) {
            warn!(client = %addr, %error, "BaseApp send failed");
        }

        let mut processed = 0;
        loop {
            let now = Instant::now();
            self.protocol.maintenance(now);
            self.pending_clients.retain(|_, (_, created)| {
                now.saturating_duration_since(*created) < Duration::from_secs(30)
            });
            if processed > 0
                && (processed >= limits::MAX_POLL_PACKETS || deadline.is_some_and(|end| now >= end))
            {
                return Event::Timeout;
            }
            processed += 1;

            // Empty the events before.
            if let Some(event) = self.events.pop_front() {
                return event;
            }

            // Reliable packets the clients haven't acknowledged yet, likely lost.
            let resends = self.protocol.take_resends(RESEND_TIMEOUT, RESEND_GIVE_UP);
            if !resends.is_empty() {
                debug!("Resending {} unacknowledged packets", resends.len());
            }
            for (addr, packet) in resends {
                if let Err(error) = self.socket.send(&packet, addr) {
                    return Event::IoError(IoErrorEvent {
                        error,
                        addr: Some(addr),
                    });
                }
            }

            let timeout = deadline.map(|deadline| {
                deadline
                    .saturating_duration_since(Instant::now())
                    .max(Duration::from_millis(1))
            });
            if let Err(error) = self.socket.set_recv_timeout(timeout) {
                return Event::IoError(IoErrorEvent { error, addr: None });
            }

            let (packet, addr) = match self.socket.recv() {
                Ok(ret) => ret,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    return Event::Timeout;
                }
                Err(error) => return Event::IoError(IoErrorEvent { error, addr: None }),
            };

            let Ok(mut channel) = self.protocol.accept(packet, addr) else {
                continue;
            };

            let Some(bundle) = channel.next_bundle() else {
                continue;
            };

            // Fully read the bundle to determine how to handle that client.
            let mut reader = bundle.element_reader();
            let mut elements = 0;
            while let Some(reader) = reader.next_element() {
                if elements >= limits::MAX_ELEMENTS_PER_BUNDLE {
                    return Event::IoError(IoErrorEvent {
                        error: io_invalid_data(format_args!("bundle element budget exceeded")),
                        addr: Some(addr),
                    });
                }
                elements += 1;
                match reader {
                    NextElementReader::Element(elt) => {
                        if let Err(error) = self.handle_element(addr, elt) {
                            return Event::IoError(IoErrorEvent {
                                error,
                                addr: Some(addr),
                            });
                        }
                    }
                    NextElementReader::Reply(reply) => {
                        return Event::IoError(IoErrorEvent {
                            error: io_invalid_data(format_args!(
                                "unexpected reply #{}",
                                reply.request_id()
                            )),
                            addr: Some(addr),
                        });
                    }
                }
            }
        }
    }

    /// Handle an element read from the given address.
    fn handle_element(
        &mut self,
        addr: SocketAddr,
        reader: ElementReader<'_, '_>,
    ) -> io::Result<()> {
        let client = self.client_keys.get(&addr).copied();
        if let Some(state) = client.and_then(|key| self.clients.get_mut(key.slot)) {
            state.last_received = Instant::now();
        }

        match reader.id() {
            LoginKey::ID => self.handle_client_auth(addr, reader),
            SessionKey::ID => self.handle_client_session_key(client, reader),
            element::id::ENABLE_ENTITIES => {
                reader.read_simple::<element::EnableEntities>()?;
                if let Some(client) = client {
                    self.events
                        .push_back(Event::EnableEntities(EnableEntitiesEvent { client }));
                }
                Ok(())
            }
            element::id::DISCONNECT_CLIENT => {
                reader.read_simple::<element::DisconnectClient>()?;
                if let Some(client) = client {
                    self.disconnect_client(client);
                }
                Ok(())
            }
            element::id::PING_DATACENTER => self.skip::<element::PingDatacenter>(reader),
            element::id::AVATAR_UPDATE_IMPLICIT => {
                self.skip::<element::AvatarUpdateImplicit>(reader)
            }
            element::id::AVATAR_UPDATE_EXPLICIT => {
                self.skip::<element::AvatarUpdateExplicit>(reader)
            }
            element::id::ACK_PHYSICS_CORRECTION => {
                self.skip::<element::AckPhysicsCorrection>(reader)
            }
            element::id::REQUEST_ENTITY_UPDATE => self.skip::<element::RequestEntityUpdate>(reader),
            element::id::NRL_MSG_TO_CELL => self.skip::<element::NrlMsgToCell>(reader),
            element::id::AVATAR_UPDATE_WARD_IMPLICIT => {
                self.skip::<element::AvatarUpdateWardImplicit>(reader)
            }
            element::id::AVATAR_UPDATE_WARD_EXPLICIT => {
                self.skip::<element::AvatarUpdateWardExplicit>(reader)
            }
            element::id::ACK_WARD_PHYSICS_CORRECTION => {
                self.skip::<element::AckWardPhysicsCorrection>(reader)
            }
            element::id::RESTORE_CLIENT_ACK => self.skip::<element::RestoreClientAck>(reader),
            element::id::CLIENT_TO_SERVER_HEARTBEAT => {
                self.skip::<element::ClientToServerHeartbeat>(reader)
            }
            element::id::SEND_TO_CELL => self.skip::<element::SendToCell>(reader),
            id if element::id::BASE_ENTITY_METHOD.contains(id) => {
                self.handle_base_entity_method(client, reader)
            }
            id if element::id::CELL_ENTITY_METHOD.contains(id) => {
                self.skip::<SkippedEntityMethod>(reader)
            }
            id => Err(io_invalid_data(format_args!("unexpected element #{id}"))),
        }
    }

    fn handle_client_auth(
        &mut self,
        addr: SocketAddr,
        reader: ElementReader<'_, '_>,
    ) -> io::Result<()> {
        let auth = reader.read_simple::<LoginKey>()?;
        let request_id = auth
            .request_id
            .ok_or_else(|| io_invalid_data(format_args!("auth should be a request")))?;

        if self.pending_clients.len() >= limits::MAX_PENDING_HANDSHAKES
            && !self.pending_clients.contains_key(&addr)
        {
            return Err(io_invalid_data(format_args!(
                "pending BaseApp login capacity exceeded"
            )));
        }

        self.events.push_back(Event::Login(LoginEvent {
            addr,
            login_key: auth.element.login_key,
            attempt_num: auth.element.attempt_num,
        }));

        self.pending_clients
            .insert(addr, (request_id, Instant::now()));

        Ok(())
    }

    fn handle_client_session_key(
        &mut self,
        client: Option<ClientKey>,
        reader: ElementReader<'_, '_>,
    ) -> io::Result<()> {
        let session = reader.read_simple::<SessionKey>()?;

        if client
            .and_then(|key| self.clients.get(key.slot))
            .is_none_or(|client| client.session_key != session.element.session_key)
        {
            return Err(io_invalid_data(format_args!("invalid BaseApp session key")));
        }

        Ok(())
    }

    /// Decode and dispatch an incoming base method call, targeting the client's current
    /// base-player entity (there's only ever one -- the wire call itself carries no
    /// entity id). Decode happens right here, eagerly, dispatched dynamically against
    /// the entity's own type (resolved from `dispatch`, computed from the script model
    /// at construction time) -- not deferred to the caller.
    ///
    /// Propagates (rather than swallows) a genuine decode error from the dynamic method
    /// read: per its doc comment, that means the bundle reader rolled back to before this
    /// element, so returning `Ok(())` here (as if this element had been consumed) would
    /// make the next `poll()` iteration re-read the exact same unconsumed element
    /// forever.
    fn handle_base_entity_method(
        &mut self,
        client: Option<ClientKey>,
        reader: ElementReader<'_, '_>,
    ) -> io::Result<()> {
        let Some((client, entity)) =
            client.and_then(|key| Some((key, self.clients.get(key.slot)?.player?)))
        else {
            return self.skip::<SkippedEntityMethod>(reader);
        };

        let Some(state) = self.entities.get(entity.slot) else {
            return self.skip::<SkippedEntityMethod>(reader);
        };

        let Some(dispatch) = self.dispatch.entity_from_id(state.type_id) else {
            return self.skip::<SkippedEntityMethod>(reader);
        };

        let addr = client.addr();
        let span = tracing::info_span!(
            parent: None, "game.command",
            "ddd.bounded_context.name" = "player_sessions",
            "user.id" = tracing::field::Empty,
            "rpc.method" = tracing::field::Empty,
            "input.method_id" = tracing::field::Empty,
            "input.entity_id" = %entity.id(),
            "input.args.count" = tracing::field::Empty,
            "input.bytes" = tracing::field::Empty,
            "input.command_id" = tracing::field::Empty,
            "input.request_id" = tracing::field::Empty,
            "output.outcome" = tracing::field::Empty,
            "client.address" = %addr.ip(),
            "client.port" = addr.port(),
            "network.peer.address" = %addr.ip(),
            "network.peer.port" = addr.port(),
            "network.transport" = "udp",
            "network.protocol.name" = "mercury",
        );
        let call = span.in_scope(|| {
            let call = reader
                .read::<element::BaseEntityMethod, _>(&dispatch.base_methods)
                .map_err(|error| {
                    span.record("output.outcome", "wire_decode_failed");
                    tracing::error!(%error, "Could not decode Mercury base method");
                    error
                })?
                .element
                .call;
            match &call {
                MethodCall::Known {
                    exposed_id,
                    name,
                    args,
                } => {
                    span.record("input.method_id", exposed_id);
                    span.record("rpc.method", name.as_ref());
                    span.record("input.args.count", args.len());
                }
                MethodCall::Unknown { exposed_id, data } => {
                    span.record("input.method_id", exposed_id);
                    span.record("input.bytes", data.len());
                }
            }
            Ok::<_, io::Error>(call)
        })?;
        self.events.push_back(Event::BaseMethod(BaseMethodEvent {
            span,
            client,
            entity,
            call,
        }));

        Ok(())
    }

    /// Read (and discard) a single already-typed element, without aborting the rest of
    /// the bundle -- used for ids whose framing is known but that this app doesn't act
    /// on yet.
    fn skip<T: Element<()>>(&mut self, reader: ElementReader<'_, '_>) -> io::Result<()> {
        reader.read_simple::<T>()?;
        Ok(())
    }
}

/// How long a reliable packet waits for its acknowledgment before being sent again, and
/// for how long it is sent again before its client is assumed gone. A poll timeout (see
/// [`App::poll_until`]) lets resends go out while no packet comes in.
const RESEND_TIMEOUT: Duration = Duration::from_millis(400);
const RESEND_GIVE_UP: Duration = Duration::from_secs(20);

/// A `BASE_ENTITY_METHOD` or `CELL_ENTITY_METHOD`-range element skipped without knowing
/// its method table: its framing is always `Variable16`, whatever the entity.
struct SkippedEntityMethod;

impl Element<()> for SkippedEntityMethod {
    fn write_length(&self, _config: &()) -> io::Result<ElementLength> {
        unreachable!("SkippedEntityMethod is read-only")
    }

    fn write(&self, _write: &mut dyn Write, _config: &()) -> io::Result<u8> {
        unreachable!("SkippedEntityMethod is read-only")
    }

    fn read_length(_config: &(), _id: u8) -> io::Result<ElementLength> {
        Ok(ElementLength::Variable16)
    }

    fn read(read: &mut dyn Read, _config: &(), _len: usize, _id: u8) -> io::Result<Self> {
        io::copy(read, &mut io::sink())?;
        Ok(Self)
    }
}

impl App {
    /// Accept the login of the given user, in response to [`Event::Login`], registering
    /// the client (blowfish key included -- everything sent to this client from this
    /// point on, including this very reply, goes out encrypted) and replying with a
    /// freshly generated session key. A client logged in earlier from the same address
    /// is gone, with its entities.
    ///
    /// This returns the new client, if one was actually waiting for this reply.
    pub fn answer_login_success(
        &mut self,
        addr: SocketAddr,
        blowfish: Arc<Blowfish>,
    ) -> Option<ClientKey> {
        let (request_id, _) = self.pending_clients.remove(&addr)?;

        if let Some(previous) = self.client_keys.get(&addr).copied() {
            self.disconnect_client(previous);
        }

        let session_key = OsRng.next_u32();

        let slot = self.clients.insert(ClientState {
            last_received: Instant::now(),
            session_key,
            player: None,
        });
        let key = ClientKey { slot, addr };
        self.client_keys.insert(addr, key);

        self.socket.set_encryption(addr, blowfish);

        self.bundle.clear();
        let _ = self
            .bundle
            .element_writer()
            .write_simple_reply(SessionKey { session_key }, request_id);
        self.protocol
            .off_channel(addr)
            .prepare(&mut self.bundle, false);
        let _ = self.socket.send_bundle(&self.bundle, addr);

        Some(key)
    }

    /// Tell a client the server's update frequency and game time, which the real server
    /// does right after the login reply. `unknown` is a field of unknown meaning, see
    /// [`client::element::UpdateFrequencyNotification`].
    pub fn update_frequency(
        &mut self,
        client: ClientKey,
        frequency: u8,
        unknown: u16,
        game_time: u32,
    ) -> io::Result<()> {
        self.send_simple(
            client,
            client::element::UpdateFrequencyNotification {
                frequency,
                unknown,
                game_time,
            },
        )
    }

    /// Stream a resource as a `ResourceHeader` and `ResourceFragment`s, for example when a client is asking
    /// for a `RES_STREAM` reply. `data` is sent as-is, chunked into fragments -- any
    /// compression/framing (e.g. WoT's zlib+pickle blobs) is the caller's job, since
    /// this library has no opinion on it.
    ///
    /// Sent as one reliable bundle per fragment rather than one giant multi-packet
    /// bundle: `Protocol`'s own bundle-fragment reassembly has a hardcoded 10s timeout
    /// (`FRAGMENT_TIMEOUT`), and real captured traffic paces large resources over
    /// several seconds of separate fragments rather than one burst. Fragments are 8 KB,
    /// like the real server's (up to 9.8 KB for a 443 KB account cache), so that a
    /// resource that large fits the 256 fragments a one-byte sequence number can count.
    /// What waits in the client's outbox is sent before, to keep the order it was written
    /// in.
    pub fn push_resource(
        &mut self,
        client: ClientKey,
        res_id: u16,
        description: Vec<u8>,
        data: &[u8],
    ) -> io::Result<()> {
        const CHUNK_SIZE: usize = 8192;

        self.client(client)?;
        let addr = client.addr;

        if data.len() > CHUNK_SIZE * 256 {
            return Err(io_invalid_data(format_args!(
                "resource of {} bytes, more than 256 fragments",
                data.len()
            )));
        }

        self.outbox.flush(addr, &mut self.protocol, &self.socket)?;

        let packet_budget = data.len().div_ceil(CHUNK_SIZE).max(1) * 7 + 1;
        if !self.protocol.can_queue_reliable(addr, packet_budget) {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "resource send budget exhausted",
            ));
        }
        self.bundle.clear();
        self.bundle
            .element_writer()
            .write_simple(client::element::ResourceHeader {
                id: res_id,
                description,
            })?;
        self.protocol
            .channel(addr, None)
            .prepare(&mut self.bundle, true);
        self.socket.send_bundle(&self.bundle, addr)?;

        let chunks: Vec<&[u8]> = if data.is_empty() {
            vec![&[]]
        } else {
            data.chunks(CHUNK_SIZE).collect()
        };

        let last_seq = (chunks.len() - 1) as u8;

        for (seq, chunk) in chunks.into_iter().enumerate() {
            self.bundle.clear();
            self.bundle
                .element_writer()
                .write_simple(client::element::ResourceFragment {
                    id: res_id,
                    sequence_num: seq as u8,
                    last: seq as u8 == last_seq,
                    data: chunk.to_vec(),
                })?;
            self.protocol
                .channel(addr, None)
                .prepare(&mut self.bundle, true);
            self.socket.send_bundle(&self.bundle, addr)?;
        }

        Ok(())
    }

    pub(crate) fn flush_client(&mut self, key: ClientKey) -> io::Result<()> {
        self.client(key)?;
        self.outbox
            .flush(key.addr, &mut self.protocol, &self.socket)
    }

    pub fn disconnect_client(&mut self, key: ClientKey) {
        if self.client_keys.get(&key.addr) != Some(&key) {
            return;
        }
        self.client_keys.remove(&key.addr);
        self.pending_clients.remove(&key.addr);
        self.remove_client(key);
        self.socket.remove_encryption(key.addr);
        self.protocol.remove_peer(key.addr);
        self.disconnected_clients.push(key);
    }

    /// Reclaim clients that have stopped sending packets, including heartbeats.
    pub fn disconnect_idle_clients(&mut self, idle: Duration) {
        let now = Instant::now();
        let expired: Vec<_> = self
            .client_keys
            .values()
            .copied()
            .filter(|key| {
                self.clients
                    .get(key.slot)
                    .is_some_and(|client| now.duration_since(client.last_received) >= idle)
            })
            .collect();
        for key in expired {
            self.disconnect_client(key);
        }
    }

    /// Drain disconnected session keys so the gateway can release account state.
    pub fn take_disconnected_clients(&mut self) -> Vec<ClientKey> {
        std::mem::take(&mut self.disconnected_clients)
    }

    /// Forget a client and its entities.
    fn remove_client(&mut self, key: ClientKey) {
        if let Some(player) = self
            .clients
            .remove(key.slot)
            .and_then(|client| client.player)
        {
            self.remove_entity(player);
        }
        self.outbox.remove(key.addr);
    }

    /// A logged in client, if not gone.
    fn client(&self, key: ClientKey) -> io::Result<&ClientState> {
        self.clients
            .get(key.slot)
            .ok_or_else(|| io_invalid_data(format_args!("no such client: {}", key.addr)))
    }

    /// The outbox of a logged in client.
    fn out(&mut self, key: ClientKey) -> io::Result<&mut ClientOutbox> {
        self.client(key)?;
        self.outbox
            .client(key.addr, &mut self.protocol, &self.socket)
    }

    /// Write a simple element to a client's outbox.
    fn send_simple<E: SimpleElement>(&mut self, client: ClientKey, element: E) -> io::Result<()> {
        self.out(client)?.write_simple(element)
    }
}

/// A logged in client of the base app: its address and its place among the app's
/// clients, which no later client at the same address takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientKey {
    slot: SlotKey,
    addr: SocketAddr,
}

impl ClientKey {
    /// The address the client is at.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }
}

impl fmt::Display for ClientKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.addr, f)
    }
}

/// A logged in client of the base app.
#[derive(Debug)]
struct ClientState {
    last_received: Instant,
    /// The session key for this client.
    session_key: u32,
    /// This client's current base-player entity, if any created yet. There's only ever
    /// one at a time (real BigWorld: `Proxy` holds a single client channel, and a base
    /// method call carries no entity id of its own -- it always targets this one).
    player: Option<EntityKey<AnyEntity>>,
}

// ============== //
//    ENTITIES    //
// ============== //

impl App {
    /// Decode a call to a base method declared by the entity schema.
    pub fn decode<M: Methods>(
        &self,
        entity: EntityKey<AnyEntity>,
        call: &MethodCall,
    ) -> Option<(EntityKey<M::Schema>, M)> {
        let state = self.entities.get(entity.slot)?;
        let call = self.entity_schemas.bound(state.type_id)?.decode(call)?;
        Some((entity.cast(), call))
    }

    /// Create a new base-player entity of the given schema for a client. This becomes
    /// that client's current base-player entity, replacing any previous one (real
    /// BigWorld only ever has one at a time).
    ///
    /// `entity_data` is of the entity's client-visible data type (see
    /// [`crate::player_sessions::mercury::app::dispatch::EntityDispatch::data_ty`]), checked before anything is
    /// sent. It contains the client-visible properties sent at creation.
    pub fn create_base_player<T: EntitySchema>(
        &mut self,
        client: ClientKey,
        entity_data: impl ScriptType,
    ) -> io::Result<EntityKey<T>> {
        let previous = self.client(client)?.player;
        let entity_type_id = self.entity_schemas.type_of::<T>()?;
        let entity_id = self.new_entity_id();

        let dispatch = self
            .dispatch
            .entity_from_id(entity_type_id)
            .ok_or_else(|| {
                io_invalid_data(format_args!("unknown entity type: {entity_type_id}"))
            })?;
        if !matches_data_type(&entity_data, &dispatch.data_ty) {
            return Err(io_invalid_data(format_args!(
                "{} is not the data of a {}",
                std::any::type_name_of_val(&entity_data),
                T::NAME
            )));
        }
        let entity_data = entity_data.into_value(&dispatch.data_ty);

        self.outbox
            .client(client.addr, &mut self.protocol, &self.socket)?
            .write(
                client::element::CreateBasePlayer {
                    entity_id,
                    entity_type_id,
                    entity_data: Cow::Borrowed(&entity_data),
                    entity_components_count: dispatch.static_components_count,
                },
                dispatch,
            )?;

        if let Some(previous) = previous {
            self.remove_entity(previous);
        }
        let slot = self.entities.insert(EntityState {
            client,
            type_id: entity_type_id,
        });
        let key = EntityKey::<T>::new(slot, entity_id);
        self.entity_keys.insert(entity_id, key.any());
        if let Some(state) = self.clients.get_mut(client.slot) {
            state.player = Some(key.any());
        }
        Ok(key)
    }

    pub fn entity<T: EntitySchema>(&mut self, key: EntityKey<T>) -> io::Result<T::View<'_>> {
        let state = self
            .entities
            .get(key.slot)
            .ok_or_else(|| io_invalid_data(format_args!("no such entity: {key}")))?;
        let binding = self
            .entity_schemas
            .bound(state.type_id)
            .filter(|bound| bound.is::<T>())
            .ok_or_else(|| {
                io_invalid_data(format_args!("entity {key} is not a registered {}", T::NAME))
            })?;
        let dispatch = self.dispatch.entity_from_id(state.type_id).ok_or_else(|| {
            io_invalid_data(format_args!("unknown entity type: {}", state.type_id))
        })?;
        let outbox = self
            .outbox
            .client(state.client.addr, &mut self.protocol, &self.socket)?;
        Ok(T::View::new(RawEntity {
            dispatch,
            binding,
            outbox,
        }))
    }

    fn remove_entity(&mut self, key: EntityKey<AnyEntity>) {
        if self.entities.remove(key.slot).is_some() {
            self.entity_keys.remove(&key.id);
        }
    }
    /// A new unique entity id.
    fn new_entity_id(&mut self) -> u32 {
        loop {
            let id = self.entities_next_id.0 & i32::MAX as u32;
            self.entities_next_id += 1;
            if id != 0 && !self.entity_keys.contains_key(&id) {
                break id;
            }
        }
    }
}

#[derive(Debug)]
struct EntityState {
    client: ClientKey,
    type_id: u16,
}

fn matches_data_type<D: ScriptType>(
    _data: &D,
    ty: &crate::player_sessions::mercury::script::Ty,
) -> bool {
    D::check(ty)
}

/// An event that happened in the base app.
#[derive(Debug)]
pub enum Event {
    IoError(IoErrorEvent),
    Login(LoginEvent),
    EnableEntities(EnableEntitiesEvent),
    /// No event came before the deadline, see [`App::poll_until`].
    Timeout,
    BaseMethod(BaseMethodEvent),
}

/// Some IO error happened internally and optionally related to a client.
#[derive(Debug)]
pub struct IoErrorEvent {
    /// The IO error.
    pub error: io::Error,
    /// An optional client address related to the error.
    pub addr: Option<SocketAddr>,
}

/// A client is trying to connect.
#[derive(Debug)]
pub struct LoginEvent {
    /// The address of the client that pinged the login app.
    pub addr: SocketAddr,
    /// The given client from the given address
    pub login_key: u32,
    /// The attempt number.
    pub attempt_num: u8,
}

/// A logged in client is ready for entities: the server may now create its player. The
/// client sends this once its logon is complete, and anything sent to it before is not
/// handled as it would be: a player created earlier is never read.
#[derive(Debug)]
pub struct EnableEntitiesEvent {
    pub client: ClientKey,
}

/// A client called a method on its current base-player entity.
#[derive(Debug)]
pub struct BaseMethodEvent {
    pub(crate) span: tracing::Span,
    pub client: ClientKey,
    pub entity: EntityKey<AnyEntity>,
    /// The decoded call, resolved dynamically against the entity's own base-method
    /// table, see [`App::decode`] to decode it further.
    pub call: MethodCall,
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn disconnect_reclaims_peer_and_idle_expiry_preserves_live_clients() {
        use blowfish::cipher::KeyInit;
        let mut app = App::new("127.0.0.1:0".parse().unwrap(), Script::default()).unwrap();
        let peer = PacketSocket::bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let addr = peer.addr().unwrap();
        let cipher = Arc::new(Blowfish::new_from_slice(&[42; 56]).unwrap());
        app.pending_clients.insert(addr, (1, Instant::now()));
        let first = app.answer_login_success(addr, cipher.clone()).unwrap();
        app.disconnect_idle_clients(Duration::from_secs(120));
        assert!(app.client(first).is_ok());
        app.clients.get_mut(first.slot).unwrap().last_received =
            Instant::now() - Duration::from_secs(121);
        app.disconnect_idle_clients(Duration::from_secs(120));
        assert!(app.client(first).is_err());
        assert!(!app.client_keys.contains_key(&addr));
        assert_eq!(app.take_disconnected_clients(), vec![first]);
        assert!(app.take_disconnected_clients().is_empty());
        app.pending_clients.insert(addr, (2, Instant::now()));
        let second = app.answer_login_success(addr, cipher.clone()).unwrap();
        app.disconnect_client(first);
        assert!(app.client(second).is_ok());
        app.pending_clients.insert(addr, (3, Instant::now()));
        let third = app.answer_login_success(addr, cipher).unwrap();
        assert!(app.client(second).is_err());
        assert!(app.client(third).is_ok());
        assert_eq!(app.take_disconnected_clients(), vec![second]);
    }
}
