mod access;
#[cfg(test)]
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
    http::{HeaderValue, header},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
};
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};
use tokio::sync::Semaphore;
use tracing::Instrument;

#[derive(Clone)]
struct AdminState {
    accounts: PlayerAccounts<PostgresPlayers>,
    secure_cookies: bool,
    sign_in_slots: Arc<Semaphore>,
    garage_accounts: Accounts<PostgresAccounts>,
    catalog: Arc<GarageCatalog>,
    grants: PlayerGrants,
    create_slots: Arc<Semaphore>,
}

#[derive(Clone)]
struct RequestSpan(tracing::Span);

pub(crate) fn router(
    players: PostgresPlayers,
    garage_accounts: Accounts<PostgresAccounts>,
    catalog: Arc<GarageCatalog>,
    grants: PlayerGrants,
    secure_cookies: bool,
    trusted_proxies: Vec<IpAddr>,
) -> Router {
    let state = AdminState {
        accounts: PlayerAccounts::new(players),
        secure_cookies,
        garage_accounts,
        catalog,
        grants,
        create_slots: Arc::new(Semaphore::new(2)),
        sign_in_slots: Arc::new(Semaphore::new(2)),
    };
    let protected = Router::new()
        .route("/players", get(players::list).post(players::create))
        .route("/players/{id}/role", post(access::change_role))
        .route("/players/{id}/garage", get(garage::show))
        .route("/players/{id}/grants", post(garage::grant))
        .route("/vehicles", get(garage::catalog))
        .route("/attachments", get(garage::attachment_catalog))
        .route("/stat-trackers", get(garage::stat_tracker_catalog))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            access::authenticate,
        ));
    let api = protected
        .route("/session", get(access::session).post(access::sign_in))
        .route("/logout", post(access::sign_out))
        .route("/setup", post(players::bootstrap))
        .layer(DefaultBodyLimit::max(8 * 1024))
        .layer(middleware::from_fn(access::protect_writes))
        .layer(middleware::from_fn(prevent_caching))
        .with_state(state);

    Router::new()
        .nest("/api/admin", api)
        .layer(middleware::from_fn_with_state(
            trusted_proxies,
            set_browser_headers,
        ))
}

async fn prevent_caching(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn set_browser_headers(
    State(trusted_proxies): State<Vec<IpAddr>>,
    mut request: Request,
    next: Next,
) -> Response {
    let span = tracing::info_span!(
        parent: None,
        "admin.http.request",
        otel.kind = "server",
        otel.status_code = tracing::field::Empty,
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
        input.role = tracing::field::Empty,
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
    );
    crate::platform::http::set_remote_trace_parent(&span, request.headers());
    async move {
        if let Some(peer) = request.extensions().get::<SocketAddr>().copied() {
            crate::platform::http::record_client_address(peer, request.headers(), &trusted_proxies);
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
            tracing::Span::current().record("otel.status_code", "ERROR");
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
    .instrument(span)
    .await
}
