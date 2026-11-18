use crate::{
    accounts::{ProgressionError, StoreOrder, StoreOrderError},
    player_sessions::application::store::{StoreError, StoreRequests, StoreToken},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, MatchedPath, Request, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{Next, from_fn},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use std::net::SocketAddr;

pub(crate) fn router(requests: StoreRequests) -> Router {
    Router::new()
        .route("/api/shop/catalog", get(catalog))
        .route("/api/shop/orders", post(purchase))
        .layer(DefaultBodyLimit::max(1024))
        .layer(from_fn(serve_request))
        .with_state(requests)
}

#[tracing::instrument(
    name = "store.http.request",
    skip_all,
    fields(
        ddd.bounded_context.name = "player_sessions",
        network.transport = "tcp",
        network.peer.address = tracing::field::Empty,
        network.peer.port = tracing::field::Empty,
        http.request.method = request.method().as_str(),
        http.route = request.extensions().get::<MatchedPath>().map(MatchedPath::as_str),
        http.response.status_code = tracing::field::Empty,
    )
)]
async fn serve_request(request: Request, next: Next) -> Response {
    let span = tracing::Span::current();
    if let Some(peer) = request.extensions().get::<SocketAddr>() {
        span.record("network.peer.address", tracing::field::display(peer.ip()));
        span.record("network.peer.port", peer.port());
    }
    let mut response = next.run(request).await;
    span.record("http.response.status_code", response.status().as_u16());
    response.headers_mut().extend([
        (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
        (header::REFERRER_POLICY, HeaderValue::from_static("no-referrer")),
        (header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")),
        (header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(
            "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'",
        )),
    ]);
    response
}

#[tracing::instrument(name = "player_sessions.store.catalog_request", skip_all, fields(ddd.bounded_context.name = "player_sessions", http.route = "/api/shop/catalog", user.id = tracing::field::Empty, output.outcome = tracing::field::Empty, otel.status_code = tracing::field::Empty))]
async fn catalog(State(store): State<StoreRequests>, headers: HeaderMap) -> Response {
    respond(&store, &headers, None).await
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct OrderPayload {
    item: i32,
    revision: String,
    credits: i64,
}

impl OrderPayload {
    fn into_order(self) -> Option<StoreOrder> {
        Some(StoreOrder {
            item: self.item,
            revision: self.revision.parse().ok()?,
            credits: self.credits,
        })
    }
}

#[tracing::instrument(name = "player_sessions.store.purchase_request", skip_all, fields(ddd.bounded_context.name = "player_sessions", http.route = "/api/shop/orders", input.item = tracing::field::Empty, user.id = tracing::field::Empty, output.outcome = tracing::field::Empty, otel.status_code = tracing::field::Empty))]
async fn purchase(
    State(store): State<StoreRequests>,
    headers: HeaderMap,
    order: Result<Json<OrderPayload>, JsonRejection>,
) -> Response {
    let Some(order) = order.ok().and_then(|Json(order)| order.into_order()) else {
        tracing::Span::current().record("output.outcome", "invalid_order");
        return (StatusCode::BAD_REQUEST, "Invalid order.").into_response();
    };
    tracing::Span::current().record("input.item", order.item);
    respond(&store, &headers, Some(order)).await
}

async fn respond(
    store: &StoreRequests,
    headers: &HeaderMap,
    order: Option<StoreOrder>,
) -> Response {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|header| header.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .and_then(StoreToken::parse);
    let result = match token {
        Some(token) => store.request(token, order).await,
        None => Err(StoreError::Unauthorized),
    };

    let span = tracing::Span::current();
    match result {
        Ok(catalog) => {
            span.record("output.outcome", "success");
            Json(catalog).into_response()
        }
        Err(error) => {
            let (status, outcome, message) = match &error {
                StoreError::Unauthorized => (
                    StatusCode::UNAUTHORIZED,
                    "unauthorized",
                    "Open the Store from a connected garage.",
                ),
                StoreError::Busy => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "busy",
                    "The account is busy. Try again shortly.",
                ),
                StoreError::Unavailable => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "unavailable",
                    "The Store is temporarily unavailable.",
                ),
                StoreError::Rejected(StoreOrderError::Stale) => (
                    StatusCode::CONFLICT,
                    "stale",
                    "The offer or account changed. Review the current price and try again.",
                ),
                StoreError::Rejected(StoreOrderError::Unavailable) => (
                    StatusCode::NOT_FOUND,
                    "offer_unavailable",
                    "This offer is unavailable.",
                ),
                StoreError::Rejected(StoreOrderError::Purchase(
                    ProgressionError::NotEnoughMoney,
                )) => (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "insufficient_credits",
                    "You need more credits for this offer.",
                ),
                StoreError::Rejected(StoreOrderError::Purchase(
                    ProgressionError::CatalogUnavailable,
                ))
                | StoreError::Internal(_) => {
                    span.record("otel.status_code", "ERROR");
                    tracing::error!(%error, "Store request failed");
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "failed",
                        "The Store is temporarily unavailable.",
                    )
                }
                StoreError::Rejected(_) => (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "rejected",
                    "This offer could not be purchased.",
                ),
            };
            span.record("output.outcome", outcome);
            (status, message).into_response()
        }
    }
}
