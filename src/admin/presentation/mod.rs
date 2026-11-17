mod garage;
mod players;

use crate::{
    access::PlayerGrants,
    accounts::{Accounts, GarageCatalog, PlayerAccounts, PostgresAccounts, PostgresPlayers},
};
use axum::{
    Router,
    extract::{DefaultBodyLimit, Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Arc};
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

pub(crate) fn router(
    accounts: PlayerAccounts<PostgresPlayers>,
    garage_accounts: Accounts<PostgresAccounts>,
    catalog: Arc<GarageCatalog>,
    grants: PlayerGrants,
    token: AdminToken,
    dashboard: Option<&Path>,
) -> Router {
    let api = Router::new()
        .route("/players", get(players::list).post(players::create))
        .route("/players/{id}/garage", get(garage::show))
        .route("/players/{id}/grants", axum::routing::post(garage::grant))
        .route("/vehicles", get(garage::catalog))
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
    router.layer(middleware::from_fn(set_browser_headers))
}

#[tracing::instrument(name = "admin.authenticate", skip_all, fields(bounded_context = "admin", input.method = request.method().as_str()))]
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

#[tracing::instrument(name = "admin.http.request", parent = None, skip_all, fields(bounded_context = "admin", http.request.method = request.method().as_str(), url.path = request.uri().path(), http.response.status_code = tracing::field::Empty))]
async fn set_browser_headers(request: Request, next: Next) -> Response {
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
