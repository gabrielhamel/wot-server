use super::{
    bootstrap,
    schema::{Account, AccountBase, AccountData, ServerStats},
};
use crate::{
    access::domain::Admissions,
    accounts::{Accounts, GarageCatalog, PostgresAccounts, PreparedAccount},
};
use anyhow::{Context, Result};
use blowfish::{Blowfish, cipher::KeyInit};
use rsa::{
    RsaPrivateKey,
    rand_core::{OsRng, RngCore},
};
mod ammunition;
mod commands;
mod crew;
mod expendables;
mod grants;
mod progression;
mod social;
use crate::social::{Contact, Conversations, Player, PostgresSocial, Social};

use crate::access::mercury::app::{
    base::{self, ClientKey},
    dispatch::MethodCall,
    entity::{EntityKey, Pickle},
    login::{self, element::LoginError},
};
use crate::access::mercury::script;
use crate::client_formats::pickle::{self, Value};
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

pub struct GatewayConfig {
    pub login_bind: SocketAddrV4,
    pub base_bind: SocketAddrV4,
    pub advertised_base: Option<SocketAddrV4>,
    pub private_key: Option<Arc<RsaPrivateKey>>,
    pub max_clients: usize,
    pub max_pending: usize,
}

struct Ticket {
    addr: SocketAddr,
    blowfish: Arc<Blowfish>,
    account: PreparedAccount,
    expires: Instant,
}
struct Session {
    account: PreparedAccount,
    entity: Option<EntityKey<Account>>,
    garage_synced: bool,
    saving: bool,
    crew_running: bool,
    crew_queue: VecDeque<crew::QueuedCrewCommand>,
    social_pending: bool,
    social_ready: bool,
    social_budget: crate::social::RequestBudget,
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
        addr: SocketAddr,
        generation: u64,
        grant_epoch: u64,
        blowfish: Arc<Blowfish>,
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
}

pub struct Gateway {
    xmpp: super::xmpp::XmppServer,
    social: Social<PostgresSocial>,
    conversations: Conversations,
    login: login::App,
    base: base::App,
    config: GatewayConfig,
    catalog: Arc<GarageCatalog>,
    accounts: crate::access::application::AccountAccess<PostgresAccounts>,
    admissions: Admissions,
    tickets: HashMap<u32, Ticket>,
    sessions: HashMap<ClientKey, Session>,
    pending_writes: HashSet<i64>,
    grant_epoch: u64,
    grants: tokio::sync::mpsc::Receiver<crate::access::application::grants::GrantRequest>,
    grant_handle: crate::access::PlayerGrants,
    completions: mpsc::Receiver<TracedCompletion>,
    sender: mpsc::SyncSender<TracedCompletion>,
    workers: Arc<Semaphore>,
    runtime: Handle,
}

impl Gateway {
    pub fn bind(
        config: GatewayConfig,
        resources: &ResourceBundle,
        catalog: Arc<GarageCatalog>,
        accounts: Accounts<PostgresAccounts>,
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
        let mut base =
            base::App::new(config.base_bind.into(), script).context("Bind BaseApp UDP socket")?;
        base.register::<Account>()
            .context("Bind garage Account schema")?;
        let (sender, completions) = mpsc::sync_channel(config.max_pending + 2 * config.max_clients);
        let workers = Arc::new(Semaphore::new(config.max_pending));
        let (grant_handle, grants) = crate::access::PlayerGrants::channel();
        let admissions = Admissions::new(config.max_pending);
        let social = Social::new(PostgresSocial::new(accounts.repository().connection_pool()));
        let xmpp = super::xmpp::XmppServer::bind(
            base.addr()?,
            social.clone(),
            config.max_clients,
            &runtime,
        )?;
        info!(login = %login.addr()?, base = %base.addr()?, advertised_base = ?config.advertised_base, "Garage gateway ready");
        Ok(Self {
            xmpp,
            social,
            conversations: Conversations::default(),
            login,
            base,
            config,
            catalog,
            accounts: crate::access::application::AccountAccess::new(accounts),
            admissions,
            tickets: HashMap::new(),
            sessions: HashMap::new(),
            pending_writes: HashSet::new(),
            grant_epoch: 0,
            grants,
            grant_handle,
            completions,
            sender,
            workers,
            runtime,
        })
    }

    pub fn addresses(&self) -> Result<(SocketAddr, SocketAddr)> {
        Ok((self.login.addr()?, self.base.addr()?))
    }

    pub fn player_grants(&self) -> crate::access::PlayerGrants {
        self.grant_handle.clone()
    }

    pub fn run(mut self, stop: Arc<AtomicBool>) -> Result<()> {
        let mut tick = Instant::now() + Duration::from_millis(100);
        while !stop.load(Ordering::Relaxed) {
            while let Ok(completion) = self.completions.try_recv() {
                let TracedCompletion { parent, value } = completion;
                parent.in_scope(|| self.complete(value));
            }
            if let Ok(request) = self.grants.try_recv() {
                self.start_grant(request);
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
                base::Event::BaseMethod(event) => {
                    let command_span = if let MethodCall::Known { name, .. } = &event.call {
                        self.sessions.get(&event.client).map(|session| {
                            tracing::info_span!(
                                parent: None, "game.command",
                                "code.layer" = "infrastructure",
                                "wot.context" = "access",
                                "user.id" = %session.account.snapshot.id,
                                "rpc.method" = %name,
                                "network.protocol.name" = "mercury",
                            )
                        })
                    } else {
                        None
                    };
                    let _command = command_span.as_ref().map(tracing::Span::enter);
                    if let MethodCall::Known { name, .. } = &event.call {
                        tracing::debug!(client = %event.client, method = %name, "Received base method");
                    }

                    if let Some((entity, request)) =
                        self.base.decode::<AccountBase>(event.entity, &event.call)
                        && self
                            .sessions
                            .get(&event.client)
                            .is_some_and(|session| session.entity == Some(entity))
                        && let Err(error) = self.command(event.client, entity, request)
                    {
                        error!(%error, "Account command failed");
                    }
                }
                base::Event::IoError(error) => error!(error = %error.error, "Base packet failed"),
                _ => {}
            }
            if Instant::now() >= tick {
                self.base.disconnect_idle_clients(Duration::from_secs(120));
                for client in self.base.take_disconnected_clients() {
                    if let Some(session) = self.sessions.get(&client) {
                        self.xmpp.revoke(session.account.snapshot.id);
                    }
                    self.social_disconnect(client);
                    self.sessions.remove(&client);
                }
                tick = Instant::now() + Duration::from_millis(100);
            }
        }
        info!("Garage gateway stopped");
        Ok(())
    }

    fn authenticate(&mut self, event: login::LoginEvent) {
        let addr = event.addr;
        let username = match super::credentials::account_username(&event.request.username) {
            Ok(username) => username,
            Err(error) => {
                self.login.answer_login_error(addr, error, String::new());
                return;
            }
        };
        let Ok(blowfish) = Blowfish::new_from_slice(&event.request.blowfish_key) else {
            self.login
                .answer_login_error(addr, LoginError::MalformedRequest, String::new());
            return;
        };
        let Ok(permit) = self.workers.clone().try_acquire_owned() else {
            self.login
                .answer_login_error(addr, LoginError::RateLimited, String::new());
            return;
        };
        let Some(generation) = self.admissions.begin(addr, Instant::now()) else {
            return;
        };
        let accounts = self.accounts.clone();
        let catalog = self.catalog.clone();
        let sender = self.sender.clone();
        let grant_epoch = self.grant_epoch;
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
            "code.layer" = "infrastructure",
            "wot.context" = "access",
            outcome = tracing::field::Empty,
            "user.id" = tracing::field::Empty,
            "user.name" = tracing::field::Empty,
            "user.email" = tracing::field::Empty,
            "wot.client.protocol_version" = event.request.protocol,
            "wot.client.resource_digest" = digest.as_deref(),
            "wot.client.language" = language.as_deref(),
            "network.protocol.name" = "mercury",
        );
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
                    span.record("user.id", snapshot.id.to_string());
                    span.record("user.name", username.as_str());
                    if let Some((local, domain)) = username.split_once('@')
                        && !local.is_empty()
                        && domain.contains('.')
                        && !username.chars().any(char::is_whitespace)
                    {
                        span.record("user.email", username.as_str());
                    }
                    tokio::task::spawn_blocking(crate::observability::in_current_span(move || {
                        catalog.prepare(snapshot)
                    }))
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
                let _ = sender.try_send(TracedCompletion::new(Completion::Login {
                    addr,
                    generation,
                    grant_epoch,
                    blowfish: Arc::new(blowfish),
                    result,
                }));
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
            Completion::Login {
                addr,
                generation,
                grant_epoch,
                blowfish,
                result,
            } => {
                let result = match result {
                    Ok(Some(account))
                        if grant_epoch != self.grant_epoch
                            && self.admissions.is_pending(addr, generation, Instant::now()) =>
                    {
                        self.refresh_admission(addr, generation, blowfish, account.snapshot.id);
                        return;
                    }
                    result => result,
                };
                if !self.admissions.complete(addr, generation, Instant::now()) {
                    return;
                }
                let account = match result {
                    Ok(Some(account)) => account,
                    Ok(None) => {
                        self.login.answer_login_error(
                            addr,
                            LoginError::InvalidPassword,
                            String::new(),
                        );
                        return;
                    }
                    Err(error) => {
                        error!(%error, "Account lookup failed");
                        self.login.answer_login_error(
                            addr,
                            LoginError::DatabaseGeneralFailure,
                            String::new(),
                        );
                        return;
                    }
                };
                if self.pending_writes.contains(&account.snapshot.id)
                    || self
                        .sessions
                        .values()
                        .any(|s| s.account.snapshot.id == account.snapshot.id)
                    || self
                        .tickets
                        .values()
                        .any(|t| t.account.snapshot.id == account.snapshot.id)
                {
                    self.login
                        .answer_login_error(addr, LoginError::AlreadyLoggedIn, String::new());
                    return;
                }
                if self.sessions.len() + self.tickets.len() >= self.config.max_clients {
                    self.login
                        .answer_login_error(addr, LoginError::BaseAppOverload, String::new());
                    return;
                }
                let destination = match self.base.addr().and_then(|bound| {
                    super::endpoint::advertised_base(self.config.advertised_base, bound, addr)
                }) {
                    Ok(destination) => destination,
                    Err(error) => {
                        error!(%error, "Resolve client-facing BaseApp address");
                        self.login
                            .answer_login_error(addr, LoginError::NoBaseApp, String::new());
                        return;
                    }
                };
                let mut key = OsRng.next_u32();
                while key == 0 || self.tickets.contains_key(&key) {
                    key = OsRng.next_u32();
                }
                self.tickets.insert(
                    key,
                    Ticket {
                        addr,
                        blowfish,
                        account,
                        expires: Instant::now() + Duration::from_secs(30),
                    },
                );
                if self
                    .login
                    .answer_login_success(addr, destination, key, String::new())
                    .is_none()
                {
                    self.tickets.remove(&key);
                }
            }
            Completion::Progression(completion) => self.complete_progression(completion),
            Completion::Ammunition(completion) => self.complete_ammunition(completion),
            Completion::Grant(completion) => self.complete_grant(completion),
            Completion::Settings {
                client,
                account_id,
                request_id,
                changes,
                result,
            } => {
                self.pending_writes.remove(&account_id);
                let Some(session) = self.sessions.get_mut(&client) else {
                    return;
                };
                session.saving = false;
                let Some(entity) = session.entity else {
                    return;
                };
                match result {
                    Ok(prepared) => {
                        let revision = prepared.snapshot.revision;
                        let previous = session.account.snapshot.revision;
                        session.account = prepared;
                        let diff = crate::pickle!({"prevRev": previous, "rev": revision, "intUserSettings": (Value::Dict(changes.into_iter().map(|(k,v)| (Value::from(k),Value::from(v))).collect()))});
                        let result = self.base.entity(entity).and_then(|mut account| {
                            account.update(Pickle(diff))?;
                            account.onCmdResponse(request_id, SUCCESS, String::new())
                        });
                        if let Err(error) = result {
                            error!(%error, "Send settings result failed");
                        }
                    }
                    Err(error) => {
                        error!(%error, "Persist settings failed");
                        let _ = self.base.entity(entity).and_then(|mut account| {
                            account.onCmdResponse(
                                request_id,
                                FAILURE,
                                "Database unavailable".into(),
                            )
                        });
                    }
                }
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
            let saving = self.pending_writes.contains(&ticket.account.snapshot.id);
            self.sessions.insert(
                client,
                Session {
                    account: ticket.account,
                    entity: None,
                    garage_synced: false,
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

    #[tracing::instrument(name = "access.infrastructure.enable", skip_all, fields(code.layer = "infrastructure", wot.context = "access", input.client.address = %client), err)]
    fn enable(&mut self, client: ClientKey) -> Result<()> {
        let Some(session) = self.sessions.get_mut(&client) else {
            return Ok(());
        };
        if session.entity.is_some() {
            return Ok(());
        }
        let snapshot = &session.account.snapshot;
        let entity = self.base.create_base_player::<Account>(
            client,
            AccountData {
                name: snapshot.display_name.clone(),
                incarnationID: 1000,
                requiredVersion_2410: self.catalog.required_version.clone(),
                initialServerSettings: bootstrap::server_settings_with_chat(
                    super::endpoint::advertised_base(
                        self.config.advertised_base,
                        self.base.addr()?,
                        client.addr(),
                    )?,
                ),
            },
        )?;
        session.entity = Some(entity);
        self.base
            .entity(entity)?
            .showGUI(bootstrap::show_gui_ctx(snapshot.id))?;
        let account_id = snapshot.id;
        info!(account_id = snapshot.id, vehicles = snapshot.vehicles.len(), %client, "Garage account created");
        self.notify_presence(account_id, true);
        Ok(())
    }
}
