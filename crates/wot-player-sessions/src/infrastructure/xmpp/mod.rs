mod framing;
mod protocol;
use anyhow::{Context, Result};
use framing::{Frame, Framing};
use protocol::Connection;
use rsa::rand_core::{OsRng, RngCore};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
    sync::{OwnedSemaphorePermit, Semaphore, mpsc, watch},
    task::JoinHandle,
};
use wot_social::contract::{ChatMessage, Friendship, SocialService};

pub const DOMAIN: &str = "wot-server.local";
pub const RESOURCE: &str = "wot";
struct Token {
    value: String,
    ip: IpAddr,
    issued: Instant,
}
struct TracedOutbound {
    parent: tracing::Span,
    value: Outbound,
}

enum Outbound {
    Stanza(String),
    Direct(ChatMessage),
}
#[derive(Clone)]
struct Online {
    generation: u64,
    sender: mpsc::Sender<TracedOutbound>,
    present: bool,
    game_host: Option<String>,
}
struct Shared {
    social: SocialService,
    tokens: Mutex<HashMap<i64, Token>>,
    online: tokio::sync::Mutex<HashMap<i64, Online>>,
}
pub struct XmppServer {
    shared: Arc<Shared>,
    task: JoinHandle<()>,
    shutdown: watch::Sender<bool>,
}
impl XmppServer {
    pub fn from_listener(
        socket: std::net::TcpListener,
        social: SocialService,
        capacity: usize,
        runtime: &tokio::runtime::Handle,
    ) -> Result<Self> {
        let address = socket.local_addr()?;
        socket.set_nonblocking(true)?;
        let _guard = runtime.enter();
        let listener = TcpListener::from_std(socket)?;
        let shared = Arc::new(Shared {
            social,
            tokens: Mutex::new(HashMap::new()),
            online: Default::default(),
        });
        let (shutdown, mut stop) = watch::channel(false);
        let state = shared.clone();
        let permits = Arc::new(Semaphore::new(capacity));
        let task=runtime.spawn(tracing::Instrument::in_current_span(async move {
            loop {
                tokio::select! {
                    _=stop.changed()=>break,
                    result=listener.accept()=>{
                        let (socket,address)=match result {Ok(pair)=>pair,Err(error)=>{tracing::error!(%error,"XMPP accept failed");continue;}};
                        let Ok(permit)=permits.clone().try_acquire_owned() else {continue;};
                        let state=state.clone(); let stop=stop.clone();
                        tokio::spawn(async move {
                            if let Err(error)=serve(socket,address,state,stop,permit).await {tracing::debug!(%address,%error,"XMPP connection ended");}
                        });
                    }
                }
            }
        }));
        tracing::info!(%address,"Native XMPP listener ready");
        Ok(Self {
            shared,
            task,
            shutdown,
        })
    }
    pub fn issue(&self, account: i64, ip: IpAddr) -> Result<String> {
        let mut bytes = [0u8; 32];
        OsRng
            .try_fill_bytes(&mut bytes)
            .context("Generate XMPP token")?;
        let value = bytes.iter().map(|v| format!("{v:02x}")).collect::<String>();
        let mut tokens = self
            .shared
            .tokens
            .lock()
            .map_err(|_| anyhow::anyhow!("Chat token registry unavailable"))?;
        tokens.insert(
            account,
            Token {
                value: value.clone(),
                ip,
                issued: Instant::now(),
            },
        );
        tracing::debug!(account, %ip, "Issued game chat token");
        Ok(value)
    }
    pub fn revoke(&self, account: i64) {
        if let Ok(mut tokens) = self.shared.tokens.lock() {
            tokens.remove(&account);
        }
    }
}
impl Drop for XmppServer {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
        self.task.abort();
    }
}

impl Shared {
    fn authorized(&self, id: i64, credential: &str, ip: IpAddr, initial: bool) -> Result<bool> {
        let tokens = self
            .tokens
            .lock()
            .map_err(|_| anyhow::anyhow!("Chat token registry unavailable"))?;
        Ok(tokens.get(&id).is_some_and(|token| {
            token.ip == ip
                && token.value == credential
                && (!initial || token.issued.elapsed() < Duration::from_secs(60))
        }))
    }
    #[tracing::instrument(name = "player_sessions.push", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.id = id, input.stanza.bytes = stanza.len()))]
    async fn push(&self, id: i64, stanza: String) {
        self.enqueue(id, Outbound::Stanza(stanza)).await;
    }

    #[tracing::instrument(name = "player_sessions.enqueue", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.id = id, output.delivery = tracing::field::Empty))]
    async fn enqueue(&self, id: i64, outbound: Outbound) {
        let mut online = self.online.lock().await;
        let span = tracing::Span::current();
        let Some(peer) = online.get(&id) else {
            span.record("output.delivery", "offline");
            return;
        };
        if peer
            .sender
            .try_send(TracedOutbound {
                parent: span.clone(),
                value: outbound,
            })
            .is_err()
        {
            span.record("output.delivery", "enqueue_failed");
            tracing::error!("XMPP recipient output queue unavailable");
            online.remove(&id);
        } else {
            span.record("output.delivery", "queued");
        }
    }
    #[tracing::instrument(name = "player_sessions.push_available", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.id = id, input.stanza.bytes = stanza.len()))]
    async fn push_available(&self, id: i64, stanza: String) {
        let available = self
            .online
            .lock()
            .await
            .get(&id)
            .is_some_and(|peer| peer.present);
        if available {
            self.push(id, stanza).await;
        }
    }

    async fn contact_presence(
        &self,
        recipient: i64,
        player: &wot_social::contract::Player,
    ) -> String {
        let online = self.online.lock().await;
        let source = online.get(&player.id);
        let available = source.is_some_and(|source| source.present);
        let host = online
            .get(&recipient)
            .and_then(|viewer| viewer.game_host.as_deref());
        protocol::presence(player, available, host)
    }

    #[tracing::instrument(name = "player_sessions.presence", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.id = id, input.available = available), err)]
    async fn presence(&self, id: i64, available: bool) -> Result<()> {
        let player = self
            .social
            .player(id)
            .await?
            .context("Unknown presence account")?;
        let roster = self.social.roster(id).await?;
        let online: Vec<_> = self.online.lock().await.keys().copied().collect();
        for owner in self.social.observers(id, &online).await? {
            if owner != id {
                self.push(owner, self.contact_presence(owner, &player).await)
                    .await;
            }
        }
        if available {
            for contact in roster
                .iter()
                .filter(|c| !c.ignored && c.friendship == Friendship::Accepted)
            {
                self.push(id, self.contact_presence(id, &contact.player).await)
                    .await;
            }
        }
        Ok(())
    }
}

#[tracing::instrument(name = "player_sessions.serve", skip_all, fields(
    ddd.bounded_context.name = "player_sessions",
    input.client.address = %address,
    output.outcome = tracing::field::Empty,
    output.disconnect.reason = tracing::field::Empty
), err)]
async fn serve(
    socket: TcpStream,
    address: SocketAddr,
    state: Arc<Shared>,
    mut shutdown: watch::Receiver<bool>,
    permit: OwnedSemaphorePermit,
) -> Result<()> {
    tracing::debug!(%address, "Accepted XMPP connection");
    let (mut read, mut write) = socket.into_split();
    let (sender, mut receiver) = mpsc::channel(128);
    let mut connection = Connection::new(state.clone(), address.ip(), sender);
    let mut framing = Framing::default();
    let mut check = tokio::time::interval(Duration::from_secs(1));
    let started = Instant::now();
    let result:Result<&str>=async {
        loop {
            tokio::select! {
                _=shutdown.changed()=>break Ok("server_shutdown"),
                _=check.tick()=>{
                    if let Some((id,credential))=&connection.identity {
                        if !state.authorized(*id,credential,address.ip(),false)? {
                            break Ok("session_revoked");
                        }
                        if connection.bound() {anyhow::ensure!(state.online.lock().await.get(id).is_some_and(|p|p.generation==connection.generation), "XMPP output queue overflow");}
                    } else {anyhow::ensure!(started.elapsed()<Duration::from_secs(30),"XMPP authentication timeout");}
                }
                stanza=receiver.recv()=>{
                    let Some(stanza)=stanza else {break Ok("outbound_closed");};
                    let TracedOutbound { parent, value } = stanza;
                    if let Some(stanza) = parent.in_scope(|| connection.outbound(value)) {
                        tracing::Instrument::instrument(write_stanza(&mut write, &stanza), parent).await?;
                    }
                }
                frame=framing.next(&mut read)=>{
                    match frame? {
                        Frame::End=>break Ok("peer_closed"),
                        Frame::Stream=>{
                            tracing::debug!(%address, authenticated=connection.identity.is_some(), "Received XMPP stream header");
                            anyhow::ensure!(connection.budget.admit(Instant::now()),"XMPP request budget exceeded");
                            let features=if connection.identity.is_some() {"<bind xmlns='urn:ietf:params:xml:ns:xmpp-bind'/><session xmlns='urn:ietf:params:xml:ns:xmpp-session'/>"} else {"<mechanisms xmlns='urn:ietf:params:xml:ns:xmpp-sasl'><mechanism>PLAIN</mechanism></mechanisms>"};
                            let response=format!("<stream:stream from='{DOMAIN}' id='wot-local' version='1.0' xmlns='jabber:client' xmlns:stream='http://etherx.jabber.org/streams'><stream:features>{features}</stream:features>");
                            write_stanza(&mut write, &response).await?;
                        }
                        Frame::Stanza(stanza)=>{
                            tracing::debug!(%address, account=?connection.identity.as_ref().map(|(id,_)|id),bytes=stanza.len(),"XMPP stanza received");
                            let span = tracing::info_span!(
                                parent: None, "xmpp.request", "ddd.bounded_context.name" = "player_sessions",
                                "input.bytes" = stanza.len(), "network.protocol.name" = "xmpp",
                                "client.address" = %address.ip(), "client.port" = address.port(),
                                "network.peer.address" = %address.ip(), "network.peer.port" = address.port(),
                                "network.transport" = "tcp",
                                "user.id" = connection.identity.as_ref().map(|(id, _)| id.to_string()),
                                "output.stanza_count" = tracing::field::Empty,
                                "output.outcome" = tracing::field::Empty,
                                "otel.status_code" = tracing::field::Empty,
                            );
                            tracing::Instrument::instrument(async {
                                if !connection.budget.admit(Instant::now()) {
                                    tracing::Span::current().record("output.outcome", "rate_limited");
                                    return Err(anyhow::anyhow!("XMPP request budget exceeded"));
                                }
                                let result = async {
                                let replies=tokio::time::timeout(Duration::from_secs(10),connection.handle(&stanza)).await??;
                                tracing::Span::current().record("output.stanza_count", replies.len());
                                for response in replies {
                                    write_stanza(&mut write, &response).await?;
                                }
                                Ok::<(), anyhow::Error>(())
                                }.await;
                                let span = tracing::Span::current();
                                if let Err(error) = &result {
                                    span.record("output.outcome", "failed");
                                    span.record("otel.status_code", "ERROR");
                                    tracing::error!(%error, "XMPP request failed");
                                } else {
                                    span.record("output.outcome", "handled");
                                }
                                result
                            }, span).await?;
                        }
                    }
                }
            }
        }
    }.await;
    if let Some((id, _)) = &connection.identity {
        let mut online = state.online.lock().await;
        let current = online
            .get(id)
            .is_some_and(|p| p.generation == connection.generation);
        if current {
            online.remove(id);
        }
        drop(online);
        if current {
            let _ = tokio::time::timeout(Duration::from_secs(10), state.presence(*id, false)).await;
        }
    }
    // Release admission before dropping the socket halves, so a peer that
    // observes EOF can reconnect after its session cleanup has finished.
    drop(permit);
    let span = tracing::Span::current();
    match result {
        Ok(reason) => {
            span.record("output.outcome", "disconnected");
            span.record("output.disconnect.reason", reason);
            Ok(())
        }
        Err(error) => {
            span.record("output.outcome", "failed");
            Err(error)
        }
    }
}

#[tracing::instrument(name = "xmpp.response", skip_all, fields(ddd.bounded_context.name = "player_sessions", output.bytes = stanza.len()), err)]
async fn write_stanza(write: &mut tokio::net::tcp::OwnedWriteHalf, stanza: &str) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(10), write.write_all(stanza.as_bytes())).await??;
    Ok(())
}
