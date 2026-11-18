mod client_address;
mod garage;
mod players;
#[cfg(test)]
mod request_traces;

use crate::{
    accounts::{Accounts, GarageCatalog, PlayerAccounts, PostgresAccounts, PostgresPlayers},
    player_sessions::PlayerGrants,
};
use axum::{
    Router,
    extract::{DefaultBodyLimit, MatchedPath, Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use sha2::{Digest, Sha256};
use std::{
    net::{IpAddr, SocketAddr},
    path::Path,
    sync::Arc,
};
use subtle::ConstantTimeEq;
use tokio::sync::Semaphore;
use tower_http::services::{ServeDir, ServeFile};

#[derive(Clone)]
pub(crate) struct AdminToken([u8; 32]);

impl AdminToken {
    pub(crate) fn new(value: &str) -> Self {
        Self(Sha256::digest(value.as_bytes()).into())
    }

    fn matches(&self, value: &[u8]) -> bool {
        bool::from(self.0.as_slice().ct_eq(Sha256::digest(value).as_slice()))
    }
}

#[derive(Clone)]
struct AdminState {
    accounts: PlayerAccounts<PostgresPlayers>,
    garage_accounts: Accounts<PostgresAccounts>,
    catalog: Arc<GarageCatalog>,
    grants: PlayerGrants,
    create_slots: Arc<Semaphore>,
}

#[derive(Clone)]
struct RequestSpan(tracing::Span);

pub(crate) fn router(
    accounts: PlayerAccounts<PostgresPlayers>,
    garage_accounts: Accounts<PostgresAccounts>,
    catalog: Arc<GarageCatalog>,
    grants: PlayerGrants,
    token: AdminToken,
    dashboard: Option<&Path>,
    trusted_proxies: Vec<IpAddr>,
) -> Router {
    let api = Router::new()
        .route("/players", get(players::list).post(players::create))
        .route("/players/{id}/garage", get(garage::show))
        .route("/players/{id}/grants", axum::routing::post(garage::grant))
        .route("/vehicles", get(garage::catalog))
        .route("/attachments", get(garage::attachment_catalog))
        .route("/stat-trackers", get(garage::stat_tracker_catalog))
        .route("/session", get(|| async { StatusCode::NO_CONTENT }))
        .layer(DefaultBodyLimit::max(8 * 1024))
        .layer(middleware::from_fn_with_state(token, authenticate))
        .layer(middleware::from_fn(prevent_caching))
        .with_state(AdminState {
            accounts,
            garage_accounts,
            catalog,
            grants,
            create_slots: Arc::new(Semaphore::new(2)),
        });

    let mut router = Router::new().nest("/api/admin", api);
    if let Some(dashboard) = dashboard {
        router = router
            .route_service("/", ServeFile::new(dashboard.join("index.html")))
            .nest_service(
                "/assets",
                ServeDir::new(dashboard.join("assets")).append_index_html_on_directories(false),
            );
    }
    router.layer(middleware::from_fn_with_state(
        trusted_proxies,
        set_browser_headers,
    ))
}

#[tracing::instrument(name = "admin.authenticate", skip_all, fields(ddd.bounded_context.name = "player_administration", input.method = request.method().as_str()))]
async fn authenticate(State(token): State<AdminToken>, request: Request, next: Next) -> Response {
    let authenticated = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.as_bytes().strip_prefix(b"Bearer "))
        .is_some_and(|value| token.matches(value));
    if authenticated {
        return next.run(request).await;
    }
    (
        [(header::WWW_AUTHENTICATE, "Bearer")],
        players::ApiError::new(StatusCode::UNAUTHORIZED, "A valid admin token is required."),
    )
        .into_response()
}

async fn prevent_caching(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[tracing::instrument(
    name = "admin.http.request",
    parent = None,
    skip_all,
    fields(
        ddd.bounded_context.name = "player_administration",
        network.transport = "tcp",
        client.address = tracing::field::Empty,
        client.port = tracing::field::Empty,
        network.peer.address = tracing::field::Empty,
        network.peer.port = tracing::field::Empty,
        http.request.method = request.method().as_str(),
        url.path = request.uri().path(),
        http.route = request.extensions().get::<MatchedPath>().map(MatchedPath::as_str),
        input.query.bytes = request.uri().query().map(str::len),
        input.content_length = request.headers().get(header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .and_then(|length| i64::try_from(length).ok()),
        input.page = tracing::field::Empty,
        input.per_page = tracing::field::Empty,
        input.account_id = tracing::field::Empty,
        input.username.bytes = tracing::field::Empty,
        input.display_name.bytes = tracing::field::Empty,
        input.grant.kind = tracing::field::Empty,
        input.grant.currency = tracing::field::Empty,
        input.amount = tracing::field::Empty,
        input.vehicle_type = tracing::field::Empty,
        input.attachment_id = tracing::field::Empty,
        input.stat_tracker_id = tracing::field::Empty,
        input.inventory_id = tracing::field::Empty,
        http.response.status_code = tracing::field::Empty,
    )
)]
async fn set_browser_headers(
    State(trusted_proxies): State<Vec<IpAddr>>,
    mut request: Request,
    next: Next,
) -> Response {
    if let Some(peer) = request.extensions().get::<SocketAddr>().copied() {
        client_address::record(peer, request.headers(), &trusted_proxies);
    }
    request
        .extensions_mut()
        .insert(RequestSpan(tracing::Span::current()));
    let mut response = next.run(request).await;
    tracing::Span::current().record(
        "http.response.status_code",
        i64::from(response.status().as_u16()),
    );
    if response.status().is_server_error() {
        tracing::error!(status = response.status().as_u16(), "Admin request failed");
    }
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'"),
    );
    response
}
