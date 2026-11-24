use super::{
    bootstrap,
    schema::{Account, AccountBase, AccountBootstrap, ServerStats},
};
use crate::domain::Admissions;
use anyhow::{Context, Result};
use blowfish::{Blowfish, cipher::KeyInit};
use rsa::RsaPrivateKey;
use wot_accounts::contract::{GameAccounts, GarageCommands, GarageQueries, PreparedAccount};
use wot_battles::{
    contract::{BattleRuntime, BattleWorker, ParticipantId},
    mercury::MercuryArena,
};
mod account_updates;
mod admin_battles;
mod admission;
mod ammunition;
mod battles;
mod commands;
mod cosmetics;
mod crew;
mod expendables;
mod grants;
mod metrics;
mod premium;
mod progression;
mod settings;
mod social;
mod sockets;
mod store;
mod training;
use crate::application::store::StoreToken;
use admission::{DeferredLogin, LoginAttempt};
use wot_social::contract::{Contact, Conversations, Player, SocialService};

use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    net::{SocketAddr, SocketAddrV4},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use tokio::{runtime::Handle, sync::Semaphore};
use tracing::{Instrument, error, info};
use wot_platform::client_formats::pickle::{self, Value};
use wot_platform::mercury::app::{
    base::{self, ClientKey},
    entity::{EntityKey, Pickle},
    login::{self, element::LoginError},
};
use wot_platform::mercury::script;
use wot_resources::ResourceBundle;

const SYNC_DATA: i16 = 100;
const SYNC_SHOP: i16 = 300;
const SYNC_DOSSIERS: i16 = 600;
const SERVER_STATS: i16 = 501;
const ADD_SETTINGS: i16 = 1600;
const STREAM: i16 = 1;
const SUCCESS: i16 = 0;
const CACHE: i16 = 2;
const FAILURE: i16 = -1;
const SET_ACTIVE_VEHICLE_SEASON: i16 = 121;
const RESET_SESSION_STATS: i16 = 10035;
const REQUEST_NO_RESPONSE: i16 = 202;
const MAINTENANCE_INTERVAL: Duration = Duration::from_millis(100);

/// Attaches the Battles adapter once the shared BaseApp is ready.
pub type BindBattles = fn(&mut base::App, &ResourceBundle, bool) -> Result<BattleRuntime>;

pub struct GatewayAccounts {
    pub accounts: GameAccounts,
    pub catalog: GarageQueries,
    pub garage: GarageCommands,
}

pub struct GatewayConfig {
    pub login_bind: SocketAddrV4,
    pub base_bind: SocketAddrV4,
    pub advertised_base: Option<SocketAddrV4>,
    pub private_key: Option<Arc<RsaPrivateKey>>,
    pub max_clients: usize,
    pub max_pending: usize,
    pub training_room_visible: bool,
    pub periphery_id: u32,
}

struct Ticket {
    generation: u64,
    addr: SocketAddr,
    blowfish: Arc<Blowfish>,
    account: PreparedAccount,
    expires: Instant,
}
struct Session {
    generation: u64,
    account: PreparedAccount,
    entity: Option<EntityKey<Account>>,
    training_room: Option<u32>,
    training_invite_next: Instant,
    garage_synced: bool,
    reported_premium_expiry: Option<u32>,
    store_token: Option<StoreToken>,
    saving: bool,
    crew_running: bool,
    crew_queue: VecDeque<crew::QueuedCrewCommand>,
    social_pending: bool,
    social_ready: bool,
    social_budget: wot_social::contract::RequestBudget,
    contacts: Vec<Contact>,
    social_next: Instant,
    message_next: Instant,
}

struct TracedCompletion {
    parent: tracing::Span,
    value: Completion,
}

impl TracedCompletion {
    fn new(value: Completion) -> Self {
        Self {
            parent: tracing::Span::current(),
            value,
        }
    }
}

enum Completion {
    Social {
        client: ClientKey,
        entity: EntityKey<Account>,
        request: social::Request,
        result: Result<social::Reply>,
    },
    Login {
        attempt: LoginAttempt,
        result: Result<Option<PreparedAccount>>,
    },
    Ammunition(ammunition::Completion),
    Progression(progression::Completion),
    Settings {
        client: ClientKey,
        account_id: i64,
        request_id: i16,
        changes: BTreeMap<i32, i32>,
        result: Result<PreparedAccount>,
    },
    Grant(grants::Completion),
    Store(store::Completion),
}

pub struct Gateway {
    xmpp: super::xmpp::XmppServer,
    social: SocialService,
    conversations: Conversations,
    login: login::App,
    base: base::App,
    config: GatewayConfig,
    catalog: GarageQueries,
    accounts: crate::application::ClientAccounts,
    garage: GarageCommands,
    admissions: Admissions,
    deferred_logins: HashMap<SocketAddr, DeferredLogin>,
    tickets: HashMap<u32, Ticket>,
    sessions: HashMap<ClientKey, Session>,
    battle_worker: BattleWorker,
    last_admin_battle_request: u32,
    pending_admin_battles: HashMap<u32, admin_battles::PendingAdminBattle>,
    admin_battle_requests:
        tokio::sync::mpsc::Receiver<crate::application::admin_battles::BattleRequest>,
    admin_battles: crate::contract::AdminBattles,
    pending_battle_launches: HashMap<u32, training::PendingTrainingBattle>,
    battle_participants: HashSet<ParticipantId>,
    battle_departures: VecDeque<(ParticipantId, tracing::Span)>,
    battle_clients: HashMap<ParticipantId, ClientKey>,
    metrics: Option<metrics::GatewayMetrics>,
    account_bootstrap: AccountBootstrap,
    training_rooms: BTreeMap<u32, training::HostedRoom>,
    last_training_room_id: u32,
    pending_writes: HashSet<i64>,
    account_change_epoch: u64,
    grants: tokio::sync::mpsc::Receiver<crate::application::grants::GrantRequest>,
    grant_handle: crate::PlayerGrants,
    store: Option<store::StoreAccess>,
    completions: mpsc::Receiver<TracedCompletion>,
    sender: mpsc::SyncSender<TracedCompletion>,
    workers: Arc<Semaphore>,
    runtime: Handle,
}

impl Gateway {
    pub fn bind(
        bind_battles: BindBattles,
        config: GatewayConfig,
        resources: &ResourceBundle,
        GatewayAccounts {
            accounts,
            catalog,
            garage,
        }: GatewayAccounts,
        social: SocialService,
        runtime: Handle,
    ) -> Result<Self> {
        anyhow::ensure!(
            (1..=256).contains(&config.max_clients) && (1..=256).contains(&config.max_pending),
            "Client and pending limits must be between 1 and 256"
        );
        anyhow::ensure!(
            config
                .advertised_base
                .is_none_or(|address| !address.ip().is_unspecified() && address.port() != 0),
            "Advertised BaseApp address must have a reachable IP and nonzero port"
        );
        let mut login =
            login::App::new(config.login_bind.into()).context("Bind login UDP socket")?;
        if let Some(key) = &config.private_key {
            login.set_encryption(key.clone());
        }
        let script =
            script::load(resources).context("Load client script ABI from resource bundle")?;
        let (base_socket, xmpp_listener) = sockets::bind_base(config.base_bind.into())?;
        let mut base = base::App::from_socket(base_socket, script);
        base.register::<Account>()
            .context("Bind garage Account schema")?;
        let account_bootstrap =
            AccountBootstrap::bind(base.dispatch()).context("Bind garage Account creation data")?;
        let battles = bind_battles(&mut base, resources, config.training_room_visible)?;
        let (sender, completions) = mpsc::sync_channel(config.max_pending + 2 * config.max_clients);
        let workers = Arc::new(Semaphore::new(config.max_pending));
        let (admin_battles, admin_battle_requests) = crate::contract::AdminBattles::channel();
        let (grant_handle, grants) = crate::PlayerGrants::channel();
        let admissions = Admissions::new(config.max_pending);
        let xmpp = super::xmpp::XmppServer::from_listener(
            xmpp_listener,
            social.clone(),
            config.max_clients,
            &runtime,
        )?;
        let SocketAddr::V4(base_address) = base.addr()? else {
            anyhow::bail!("Arena replication requires an IPv4 BaseApp");
        };
        info!(login = %login.addr()?, base = %base_address, advertised_base = ?config.advertised_base, "Garage gateway ready");
        let battle_worker = battles.spawn_worker(&runtime, base_address);
        if config.training_room_visible && !battle_worker.has_maps() {
            tracing::warn!(
                "Training Room creation is unavailable. Re-extract the resource bundle with arena definitions."
            );
        }
        Ok(Self {
            xmpp,
            social,
            conversations: Conversations::default(),
            login,
            base,
            config,
            garage,
            catalog,
            accounts: crate::application::ClientAccounts::new(accounts),
            admissions,
            deferred_logins: HashMap::new(),
            tickets: HashMap::new(),
            sessions: HashMap::new(),
            battle_worker,
            pending_battle_launches: HashMap::new(),
            pending_admin_battles: HashMap::new(),
            last_admin_battle_request: 0,
            admin_battles,
            admin_battle_requests,
            battle_participants: HashSet::new(),
            battle_departures: VecDeque::new(),
            battle_clients: HashMap::new(),
            metrics: None,
            account_bootstrap,
            training_rooms: BTreeMap::new(),
            last_training_room_id: 0,
            pending_writes: HashSet::new(),
            account_change_epoch: 0,
            grants,
            grant_handle,
            store: None,
            completions,
            sender,
            workers,
            runtime,
        })
    }

    pub fn addresses(&self) -> Result<(SocketAddr, SocketAddr)> {
        Ok((self.login.addr()?, self.base.addr()?))
    }

    pub fn admin_battles(&self) -> crate::contract::AdminBattles {
        self.admin_battles.clone()
    }

    pub fn player_grants(&self) -> crate::PlayerGrants {
        self.grant_handle.clone()
    }

    pub fn configure_metrics(&mut self, meter: &opentelemetry::metrics::Meter) {
        let metrics = metrics::GatewayMetrics::new(meter);
        metrics.record_counts(self.sessions.len(), self.battle_worker.active_count());
        self.metrics = Some(metrics);
    }

    pub fn run(mut self, stop: Arc<AtomicBool>) -> Result<()> {
        let interval = MAINTENANCE_INTERVAL;
        let mut tick = Instant::now() + interval;
        while !stop.load(Ordering::Relaxed) {
            for client in self.base.take_disconnected_clients() {
                self.remove_session(client);
            }
            self.poll_battles();
            if let Ok(request) = self.admin_battle_requests.try_recv() {
                self.handle_admin_battle_request(request);
            }
            while let Ok(completion) = self.completions.try_recv() {
                let TracedCompletion { parent, value } = completion;
                parent.in_scope(|| self.complete(value));
            }
            self.resume_logins();
            if let Ok(request) = self.grants.try_recv() {
                self.start_grant(request);
            }
            if let Some(request) = self
                .store
                .as_mut()
                .and_then(|store| store.requests.try_recv().ok())
            {
                self.handle_store(request);
            }
            self.tickets
                .retain(|_, ticket| ticket.expires > Instant::now());
            match self
                .login
                .poll_until(Instant::now() + Duration::from_millis(1))
            {
                login::Event::Login(event) => self.authenticate(event),
                login::Event::IoError(error) => {
                    error!(client = ?error.addr, error = %error.error, "Login packet failed")
                }
                _ => {}
            }
            match self
                .base
                .poll_until(Instant::now() + Duration::from_millis(1))
            {
                base::Event::Login(event) => self.enter(event),
                base::Event::EnableEntities(event) => {
                    if let Err(error) = self.enable(event.client) {
                        error!(%error, "Garage bootstrap failed");
                    }
                }
                base::Event::BaseMethod(event) => self.handle_base_method(event),
                base::Event::CellMethod(event) => self.handle_battle_control(event),
                base::Event::IoError(error) => error!(error = %error.error, "Base packet failed"),
                _ => {}
            }
            if Instant::now() >= tick {
                self.expire_premium();
                self.base.disconnect_idle_clients(Duration::from_secs(120));
                let now = Instant::now();
                if let Some(metrics) = &self.metrics {
                    metrics.record_counts(self.sessions.len(), self.battle_worker.active_count());
                }
                tick += interval;
                if tick <= now {
                    tick = now + interval;
                }
            }
        }
        if let Some(metrics) = &self.metrics {
            metrics.record_counts(0, 0);
        }
        if let Err(error) = self.battle_worker.shutdown(&self.runtime) {
            error!(%error, "Battle worker failed");
        }
        info!("Garage gateway stopped");
        Ok(())
    }

    fn authenticate(&mut self, event: login::LoginEvent) {
        let addr = event.addr;
        let language = serde_json::from_str::<serde_json::Value>(&event.request.context)
            .ok()
            .and_then(|value| {
                value
                    .get("lang_id")?
                    .as_str()
                    .filter(|language| language.len() <= 32)
                    .map(str::to_owned)
            });
        let digest = event.request.digest.map(|digest| {
            digest
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        });
        let span = tracing::info_span!(
            parent: None, "game.authenticate",
            "ddd.bounded_context.name" = "player_sessions",
            outcome = tracing::field::Empty,
            "otel.status_code" = tracing::field::Empty,
            "user.id" = tracing::field::Empty,
            "user.name" = tracing::field::Empty,
            "user.email" = tracing::field::Empty,
            "wot.client.protocol_version" = event.request.protocol,
            "wot.client.resource_digest" = digest.as_deref(),
            "wot.client.language" = language.as_deref(),
            "network.protocol.name" = "mercury",
            "client.address" = %addr.ip(),
            "client.port" = addr.port(),
            "network.peer.address" = %addr.ip(),
            "network.peer.port" = addr.port(),
            "network.transport" = "udp",
        );
        span.in_scope(|| self.start_authentication(event));
    }

    fn start_authentication(&mut self, event: login::LoginEvent) {
        let addr = event.addr;
        let span = tracing::Span::current();
        let username = match super::credentials::account_username(&event.request.username) {
            Ok(username) => username,
            Err(error) => {
                span.record("outcome", "invalid_username");
                self.login.answer_login_error(addr, error, String::new());
                return;
            }
        };
        let Ok(blowfish) = Blowfish::new_from_slice(&event.request.blowfish_key) else {
            span.record("outcome", "malformed_request");
            self.login
                .answer_login_error(addr, LoginError::MalformedRequest, String::new());
            return;
        };
        let Ok(permit) = self.workers.clone().try_acquire_owned() else {
            span.record("outcome", "rate_limited");
            self.login
                .answer_login_error(addr, LoginError::RateLimited, String::new());
            return;
        };
        let Some(generation) = self.admissions.begin(addr, Instant::now()) else {
            span.record("outcome", "admission_unavailable");
            return;
        };
        let accounts = self.accounts.clone();
        let catalog = self.catalog.clone();
        let sender = self.sender.clone();
        let account_change_epoch = self.account_change_epoch;
        self.runtime.spawn(
            async move {
                let result = async {
                    let Some(snapshot) = accounts
                        .authenticate(&username, event.request.password)
                        .await?
                    else {
                        return Ok(None);
                    };
                    let span = tracing::Span::current();
                    span.record("user.id", snapshot.id().to_string());
                    if !snapshot.display_name().is_empty() {
                        span.record("user.name", snapshot.display_name());
                    }
                    if let Some((local, domain)) = username.split_once('@')
                        && !local.is_empty()
                        && domain.contains('.')
                        && !username.chars().any(char::is_whitespace)
                    {
                        span.record("user.email", username.as_str());
                    }
                    tokio::task::spawn_blocking(wot_platform::observability::in_current_span(
                        move || catalog.prepare(snapshot),
                    ))
                    .await?
                    .map(Some)
                }
                .await;
                tracing::Span::current().record(
                    "outcome",
                    match &result {
                        Ok(Some(_)) => "authenticated",
                        Ok(None) => "rejected",
                        Err(_) => "error",
                    },
                );
                if sender
                    .try_send(TracedCompletion::new(Completion::Login {
                        attempt: LoginAttempt {
                            addr,
                            generation,
                            account_change_epoch,
                            blowfish: Arc::new(blowfish),
                        },
                        result,
                    }))
                    .is_err()
                {
                    tracing::Span::current().record("outcome", "completion_unavailable");
                    error!("Login completion queue unavailable");
                }
                drop(permit);
            }
            .instrument(span),
        );
    }

    fn complete(&mut self, completion: Completion) {
        match completion {
            Completion::Social {
                client,
                entity,
                request,
                result,
            } => {
                if let Err(error) = self.social_complete(client, entity, request, result) {
                    error!(%error, "Social response failed");
                }
            }
            Completion::Login { attempt, result } => self.complete_login(attempt, result),
            Completion::Progression(completion) => self.complete_progression(completion),
            Completion::Ammunition(completion) => self.complete_ammunition(completion),
            Completion::Grant(completion) => self.complete_grant(completion),
            Completion::Store(completion) => self.complete_store(completion),
            Completion::Settings {
                client,
                account_id,
                request_id,
                changes,
                result,
            } => {
                self.complete_settings(client, account_id, request_id, changes, result);
            }
        }
    }

    fn enter(&mut self, event: base::LoginEvent) {
        let Some(ticket) = self.tickets.get(&event.login_key) else {
            return;
        };
        if ticket.expires <= Instant::now() || ticket.addr.ip() != event.addr.ip() {
            return;
        }
        let Some(ticket) = self.tickets.remove(&event.login_key) else {
            return;
        };
        if let Some(client) = self.base.answer_login_success(event.addr, ticket.blowfish) {
            let saving = self.pending_writes.contains(&ticket.account.snapshot.id());
            self.battle_clients
                .insert(ParticipantId(ticket.generation), client);
            self.sessions.insert(
                client,
                Session {
                    generation: ticket.generation,
                    account: ticket.account,
                    entity: None,
                    training_room: None,
                    training_invite_next: Instant::now(),
                    garage_synced: false,
                    reported_premium_expiry: None,
                    store_token: None,
                    saving,
                    crew_running: false,
                    crew_queue: VecDeque::new(),
                    social_pending: false,
                    social_ready: false,
                    social_budget: Default::default(),
                    contacts: Vec::new(),
                    social_next: Instant::now(),
                    message_next: Instant::now(),
                },
            );
            let _ = self.base.update_frequency(client, 10, 1, 0);
        }
    }

    #[tracing::instrument(name = "player_sessions.enable", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.client.address = %client, output.account.version_property = tracing::field::Empty, output.store.host = tracing::field::Empty, output.training_room.visible = tracing::field::Empty, output.training_room.description_enabled = tracing::field::Empty, output.equipment_upgrades.enabled = tracing::field::Empty, output.outcome = tracing::field::Empty, otel.status_code = tracing::field::Empty), err)]
    fn enable(&mut self, client: ClientKey) -> Result<()> {
        let Some(session) = self.sessions.get_mut(&client) else {
            return Ok(());
        };
        let participant = ParticipantId(session.generation);
        if self.battle_participants.contains(&participant) {
            let result = self.battle_worker.enter(participant);
            self.handle_battle_submission(client, result);
            return Ok(());
        }
        if session.entity.is_some() {
            return Ok(());
        }
        let store_settings = if let Some((origin, host)) = self
            .store
            .as_ref()
            .and_then(|store| store.origin_for(client.addr()))
        {
            if session.store_token.is_none() {
                session.store_token = Some(StoreToken::generate()?);
            }
            session
                .store_token
                .as_ref()
                .map(|token| (format!("{origin}/shop/#{}", token.encode()), host))
        } else {
            None
        };
        let snapshot = &session.account.snapshot;
        let equipment_upgrades = self.catalog.equipment_upgrades_enabled();
        let entity = self.base.create_base_player::<Account>(
            client,
            self.account_bootstrap.encode(
                snapshot.display_name(),
                bootstrap::server_settings_with_chat(
                    super::endpoint::advertised_base(
                        self.config.advertised_base,
                        self.base.addr()?,
                        client.addr(),
                    )?,
                    store_settings
                        .as_ref()
                        .map(|(url, host)| (url.as_str(), host.as_ref())),
                    self.config.training_room_visible,
                    equipment_upgrades,
                ),
            ),
        )?;
        tracing::Span::current().record(
            "output.account.version_property",
            self.account_bootstrap.version_property(),
        );
        tracing::Span::current().record(
            "output.training_room.visible",
            self.config.training_room_visible,
        );
        tracing::Span::current().record(
            "output.training_room.description_enabled",
            self.config.training_room_visible,
        );
        tracing::Span::current().record("output.equipment_upgrades.enabled", equipment_upgrades);
        if let Some((_, host)) = store_settings {
            tracing::Span::current().record("output.store.host", host.as_ref());
        }
        session.entity = Some(entity);
        self.base
            .entity(entity)?
            .showGUI(bootstrap::show_gui_ctx(snapshot.id()))?;
        let account_id = snapshot.id();
        info!(account_id = snapshot.id(), vehicles = snapshot.vehicle_count(), %client, "Garage account created");
        self.notify_presence(account_id, true);
        Ok(())
    }
}
