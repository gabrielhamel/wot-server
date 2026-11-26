//! Login application where clients ping and initiate connection in order to be routed
//! to the base application afterward.

use crate::mercury::net::limits;
pub mod element;

use std::collections::{HashMap, VecDeque};
use std::io;
use std::net::{SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::{Duration, Instant};

use blowfish::Blowfish;
use blowfish::cipher::KeyInit;
use rsa::RsaPrivateKey;

use crate::mercury::net::bundle::{Bundle, ElementReader, NextElementReader};
use crate::mercury::net::proto::Protocol;
use crate::mercury::net::socket::PacketSocket;

use super::io_invalid_data;

use element::{LoginError, LoginRequest, LoginResponse, LoginSuccess, Ping};

/// The login application.
#[derive(Debug)]
pub struct App {
    /// Internal socket for this application.
    socket: PacketSocket,
    /// The packet tracker used to build bundles.
    protocol: Protocol,
    /// Queue of events that are waiting to be returned.
    events: VecDeque<Event>,
    /// A temporary bundle for sending.
    bundle: Bundle,
    /// Optional private key to set if encryption is enabled on the login app. This
    /// implies that the client should use the matching public key when logging in in
    /// order to validate.
    encryption_key: Option<Arc<RsaPrivateKey>>,
    /// Login requests of each client in process with the login app.
    pending_requests: HashMap<SocketAddr, PendingRequest>,
    /// Responses to be sent in response to login or challenge requests.
    pending_responses: VecDeque<PendingResponse>,
    /// Used for benchmarking performance.
    received_instant: Instant,
    login_window: Instant,
    login_count: usize,
    login_sources: HashMap<std::net::IpAddr, usize>,
}
impl App {
    pub fn new(addr: SocketAddr) -> io::Result<Self> {
        Ok(Self {
            socket: PacketSocket::bind(addr)?,
            protocol: Protocol::new(),
            events: VecDeque::new(),
            bundle: Bundle::new(),
            encryption_key: None,
            pending_requests: HashMap::new(),
            pending_responses: VecDeque::new(),
            received_instant: Instant::now(),
            login_window: Instant::now(),
            login_count: 0,
            login_sources: HashMap::new(),
        })
    }

    pub fn configure_metrics(&mut self, meter: &opentelemetry::metrics::Meter) {
        self.protocol.configure_metrics(meter, "login");
    }

    /// Get the address this app is bound to.
    pub fn addr(&self) -> io::Result<SocketAddr> {
        self.socket.addr()
    }

    /// Enable encryption on login app, given a RSA private key, the client should use
    /// the matching public key in order to validate this server.
    pub fn set_encryption(&mut self, key: Arc<RsaPrivateKey>) {
        self.encryption_key = Some(key);
    }

    /// As opposed to [`Self::set_encryption`], disable encryption on login app.
    pub fn remove_encryption(&mut self) {
        self.encryption_key = None;
    }

    /// Return true if encryption is enabled on this login app.
    pub fn has_encryption(&self) -> bool {
        self.encryption_key.is_some()
    }

    /// Poll for the next event of this login app, blocking until one comes.
    ///
    /// The answers given since the last poll are sent first.
    pub fn poll(&mut self) -> Event {
        self.poll_inner(None)
    }

    /// Same as [`Self::poll`], but returns [`Event::Timeout`] once the deadline passes
    /// with no event, so the caller gets a chance to handle its other events.
    pub fn poll_until(&mut self, deadline: Instant) -> Event {
        self.poll_inner(Some(deadline))
    }

    fn poll_inner(&mut self, deadline: Option<Instant>) -> Event {
        let mut processed = 0;
        loop {
            let now = Instant::now();
            self.protocol.maintenance(now);
            self.pending_requests.retain(|_, request| {
                now.saturating_duration_since(request.created) < Duration::from_secs(30)
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

            // Then send pending login responses.
            while let Some(res) = self.pending_responses.pop_front() {
                let addr = res.addr;
                if let Err(error) = self.send_response(res) {
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

            let received = self.socket.recv_with_rejection(|_addr, reason, bytes| {
                self.protocol.record_rejection(None, reason, bytes);
            });
            let (packet, addr) = match received {
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

            self.received_instant = Instant::now();

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
                        if let Err(error) = self.handle_element(elt, addr) {
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
    fn handle_element(&mut self, elt: ElementReader, addr: SocketAddr) -> io::Result<()> {
        match elt.id() {
            element::id::PING => self.handle_ping(elt, addr),
            element::id::LOGIN_REQUEST => self.handle_login_request(elt, addr),
            id => Err(io_invalid_data(format_args!("unexpected element #{id}"))),
        }
    }

    /// Handle a ping request to the login node, we answer as fast as possible.
    fn handle_ping(&mut self, elt: ElementReader, addr: SocketAddr) -> io::Result<()> {
        let ping = elt.read_simple::<Ping>()?;
        let request_id = ping
            .request_id
            .ok_or_else(|| io_invalid_data(format_args!("ping should be a request")))?;

        self.bundle.clear();
        self.bundle
            .element_writer()
            .write_simple_reply(ping.element, request_id)?;
        self.protocol
            .off_channel(addr)
            .prepare(&mut self.bundle, false);
        self.socket
            .send_bundle_without_encryption(&self.bundle, addr)?;

        let latency = self.received_instant.elapsed();
        self.events
            .push_back(Event::Ping(PingEvent { addr, latency }));

        Ok(())
    }

    /// Handle a login request to the login node.
    fn handle_login_request(&mut self, elt: ElementReader, addr: SocketAddr) -> io::Result<()> {
        let now = Instant::now();
        if now.saturating_duration_since(self.login_window) >= Duration::from_secs(1) {
            self.login_window = now;
            self.login_count = 0;
            self.login_sources.clear();
        }
        if self.login_count >= limits::MAX_LOGIN_REQUESTS_PER_SECOND
            || self.login_sources.get(&addr.ip()).copied().unwrap_or(0)
                >= limits::MAX_LOGIN_REQUESTS_PER_IP
        {
            return Err(io_invalid_data(format_args!(
                "login decryption rate limit exceeded"
            )));
        }
        if self.pending_requests.len() >= limits::MAX_PENDING_HANDSHAKES
            && !self.pending_requests.contains_key(&addr)
        {
            return Err(io_invalid_data(format_args!(
                "pending login capacity exceeded"
            )));
        }
        self.login_count += 1;
        *self.login_sources.entry(addr.ip()).or_default() += 1;
        let login;
        if let Some(encryption_key) = self.encryption_key.as_deref() {
            login = elt.read::<LoginRequest, _>(encryption_key)?;
        } else {
            login = elt.read_simple::<LoginRequest>()?;
        }

        let request_id = login
            .request_id
            .ok_or_else(|| io_invalid_data(format_args!("login should be a request")))?;

        let blowfish = Arc::new(
            Blowfish::new_from_slice(&login.element.blowfish_key)
                .map_err(|_| io_invalid_data(format_args!("login has invalid blowfish key")))?,
        );

        // Update or insert the login tracker...
        self.pending_requests.insert(
            addr,
            PendingRequest {
                created: Instant::now(),
                blowfish,
                request_id,
            },
        );

        self.events.push_back(Event::Login(LoginEvent {
            addr,
            request: login.element,
        }));

        Ok(())
    }

    /// In response to a [`LoginEvent`], authorize a client to log into the base
    /// application, giving them its address and a login key that will be used to
    /// register itself.
    ///
    /// This returns the blowfish encryption instance if a client was effectively
    /// waiting for a response.
    pub fn answer_login_success(
        &mut self,
        addr: SocketAddr,
        app_addr: SocketAddrV4,
        login_key: u32,
        server_message: String,
    ) -> Option<Arc<Blowfish>> {
        self.answer_login_response(
            addr,
            LoginResponse::Success(LoginSuccess {
                addr: app_addr.into(),
                login_key,
                server_message,
            }),
        )
    }

    /// In response to a [`LoginEvent`], authorize a client to log into the base
    /// application, giving them its address and a login key that will be used to
    /// register itself.
    ///
    /// This returns true if a client was effectively waiting for a response.
    pub fn answer_login_error(
        &mut self,
        addr: SocketAddr,
        error: LoginError,
        message: String,
    ) -> bool {
        self.answer_login_response(addr, LoginResponse::Error(error, message))
            .is_some()
    }

    /// Internal wrapper for answering a login response.
    #[inline]
    fn answer_login_response(
        &mut self,
        addr: SocketAddr,
        response: LoginResponse,
    ) -> Option<Arc<Blowfish>> {
        let request = self.pending_requests.remove(&addr)?;

        let bf = Arc::clone(&request.blowfish);

        let parent = tracing::Span::current();
        let span = if parent.is_disabled() {
            tracing::Span::none()
        } else {
            tracing::info_span!(
                parent: &parent, "mercury.login.response",
                "ddd.bounded_context.name" = "player_sessions",
                "input.request_id" = request.request_id,
                "output.response_code" = match &response {
                    LoginResponse::Success(_) => 1_u8,
                    LoginResponse::Error(code, _) => *code as u8,
                    LoginResponse::Unknown(code) => *code,
                },
                "output.delivery" = tracing::field::Empty,
            )
        };
        self.pending_responses.push_back(PendingResponse {
            span,
            request,
            addr,
            inner: response,
        });

        Some(bf)
    }

    /// Send a challenge to the given client, this only works if a tracker exists.
    fn send_response(&mut self, response: PendingResponse) -> io::Result<()> {
        let span = response.span.clone();
        span.in_scope(|| {
            let result = self.write_response(response);
            match &result {
                Ok(()) => {
                    span.record("output.delivery", "sent");
                }
                Err(error) => {
                    span.record("output.delivery", "send_failed");
                    tracing::error!(%error, "Could not send Mercury login response");
                }
            }
            result
        })
    }

    fn write_response(&mut self, response: PendingResponse) -> io::Result<()> {
        self.bundle.clear();
        self.bundle.element_writer().write_reply(
            response.inner,
            response.request.request_id,
            &*response.request.blowfish,
        )?;

        self.protocol
            .off_channel(response.addr)
            .prepare(&mut self.bundle, false);
        self.socket
            .send_bundle_without_encryption(&self.bundle, response.addr)?;

        Ok(())
    }
}

/// Describe a client trying to log into the server.
#[derive(Debug)]
struct PendingRequest {
    created: Instant,
    /// This is the blowfish key as sent by the client when requesting login.
    blowfish: Arc<Blowfish>,
    /// Id of the last request the client sent and where replies should be sent.
    request_id: u32,
}

/// Describe a response pending to be sent to an address.
#[derive(Debug)]
struct PendingResponse {
    span: tracing::Span,
    /// Initial request leading to this response.
    request: PendingRequest,
    /// The address of the client.
    addr: SocketAddr,
    /// Inner login response.
    inner: LoginResponse,
}

// ============ //
//    EVENTS    //
// ============ //

/// An event that happened in the login app regarding the login process.
#[derive(Debug)]
pub enum Event {
    /// The deadline given to [`App::poll_until`] passed with no event.
    Timeout,
    IoError(IoErrorEvent),
    Ping(PingEvent),
    Login(LoginEvent),
}

/// Some IO error happened internally and optionally related to a client.
#[derive(Debug)]
pub struct IoErrorEvent {
    /// The IO error.
    pub error: io::Error,
    /// An optional client address related to the error.
    pub addr: Option<SocketAddr>,
}

/// A client has pinged the login app.
#[derive(Debug)]
pub struct PingEvent {
    /// The address of the client that pinged the login app.
    pub addr: SocketAddr,
    /// Duration between reception of the ping packet to it being fully sent, this is
    /// the internal latency, not representing the real latency to the client.
    pub latency: Duration,
}

/// A client has made a request to login, this request can be answered with the app using:
/// - [`App::answer_login_success`]
/// - [`App::answer_login_error`]
#[derive(Debug)]
pub struct LoginEvent {
    /// The address of the client that request a login.
    pub addr: SocketAddr,
    /// The request received.
    pub request: LoginRequest,
}
