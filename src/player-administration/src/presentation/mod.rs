mod access;
mod battles;
#[cfg(test)]
mod client_address;
mod garage;
mod players;
mod registration;

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
use wot_accounts::contract::{GameAccounts, GarageQueries, PlayerAccess};
use wot_player_sessions::contract::{
    AdminBattles, PlayerGrants, StoreRequests, dashboard_store_router,
};

#[derive(Clone)]
struct AdminState {
    accounts: PlayerAccess,
    secure_cookies: bool,
    store_enabled: bool,
    sign_in_slots: Arc<Semaphore>,
    garage_accounts: GameAccounts,
    catalog: GarageQueries,
    grants: PlayerGrants,
    battles: Option<AdminBattles>,
    battle_loadout_slots: Arc<Semaphore>,
    create_slots: Arc<Semaphore>,
    username_check_slots: Arc<Semaphore>,
}

#[derive(Clone)]
struct RequestSpan(tracing::Span);

pub struct DashboardConfig {
    pub secure_cookies: bool,
    pub trusted_proxies: Vec<IpAddr>,
    pub battles: Option<AdminBattles>,
}

pub fn router(
    players: PlayerAccess,
    garage_accounts: GameAccounts,
    catalog: GarageQueries,
    grants: PlayerGrants,
    store: Option<StoreRequests>,
    DashboardConfig {
        secure_cookies,
        trusted_proxies,
        battles,
    }: DashboardConfig,
) -> Router {
    let state = AdminState {
        accounts: players,
        secure_cookies,
        store_enabled: store.is_some(),
        garage_accounts,
        catalog,
        grants,
        battles,
        battle_loadout_slots: Arc::new(Semaphore::new(2)),
        create_slots: Arc::new(Semaphore::new(2)),
        username_check_slots: Arc::new(Semaphore::new(4)),
        sign_in_slots: Arc::new(Semaphore::new(2)),
    };
    let protected = Router::new()
        .route("/battles", get(battles::options).post(battles::start))
        .route("/battles/vehicles/{vehicle}", get(battles::loadout))
        .route("/players", get(players::list).post(players::create))
        .route("/players/{id}/role", post(access::change_role))
        .route("/players/{id}/garage", get(garage::show))
        .route("/players/{id}/grants", post(garage::grant))
        .route("/vehicles", get(garage::catalog))
        .route("/crew-books", get(garage::crew_book_catalog))
        .route(
            "/experimental-equipment",
            get(garage::experimental_equipment_catalog),
        )
        .route(
            "/improved-equipment",
            get(garage::improved_equipment_catalog),
        )
        .route("/bounty-equipment", get(garage::bounty_equipment_catalog))
        .route("/attachments", get(garage::attachment_catalog))
        .route("/styles-2d", get(garage::style_2d_catalog))
        .route("/styles-3d", get(garage::style_3d_catalog))
        .route("/crew-skins", get(garage::crew_skin_catalog))
        .route("/special-recruits", get(garage::special_recruit_catalog))
        .route("/stat-trackers", get(garage::stat_tracker_catalog))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            access::authenticate,
        ));
    let dashboard_store = store.map_or_else(Router::new, |store| {
        dashboard_store_router(store).route_layer(middleware::from_fn_with_state(
            state.clone(),
            access::authenticate_store_player,
        ))
    });
    let api = protected
        .merge(dashboard_store)
        .route("/session", get(access::session).post(access::sign_in))
        .route("/logout", post(access::sign_out))
        .route("/setup", post(players::bootstrap))
        .route("/registration", post(registration::register))
        .route("/registration/username", post(registration::check_username))
        .route("/me/garage", get(garage::show_own))
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
        input.email.bytes = tracing::field::Empty,
        input.display_name.bytes = tracing::field::Empty,
        input.grant.kind = tracing::field::Empty,
        input.grant.currency = tracing::field::Empty,
        input.amount = tracing::field::Empty,
        input.item = tracing::field::Empty,
        input.count = tracing::field::Empty,
        input.vehicle_type = tracing::field::Empty,
        input.attachment_id = tracing::field::Empty,
        input.stat_tracker_id = tracing::field::Empty,
        input.inventory_id = tracing::field::Empty,
        http.response.status_code = tracing::field::Empty,
    );
    wot_platform::http::set_remote_trace_parent(&span, request.headers());
    async move {
        if let Some(peer) = request.extensions().get::<SocketAddr>().copied() {
            wot_platform::http::record_client_address(peer, request.headers(), &trusted_proxies);
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
