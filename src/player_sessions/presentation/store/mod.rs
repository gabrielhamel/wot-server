use crate::{
    accounts::{ProgressionError, StoreOrder, StoreOrderError, StorePrice},
    player_sessions::application::store::{StoreCustomer, StoreError, StoreRequests, StoreToken},
};
use axum::{
    Extension, Json, Router,
    extract::{DefaultBodyLimit, MatchedPath, Request, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{Next, from_fn},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use std::net::SocketAddr;
use tracing::Instrument;

#[derive(Clone, Copy)]
pub(crate) struct DashboardStorePlayer(pub(crate) i64);

pub(crate) fn router(requests: StoreRequests) -> Router {
    endpoints("/api/shop/catalog", "/api/shop/orders")
        .layer(from_fn(serve_request))
        .with_state(requests)
}

pub(crate) fn dashboard_router<S>(requests: StoreRequests) -> Router<S> {
    endpoints("/me/shop/catalog", "/me/shop/orders").with_state(requests)
}

fn endpoints(catalog_path: &str, order_path: &str) -> Router<StoreRequests> {
    Router::new()
        .route(catalog_path, get(catalog))
        .route(order_path, post(purchase))
        .layer(DefaultBodyLimit::max(1024))
}

async fn serve_request(request: Request, next: Next) -> Response {
    let span = tracing::info_span!(
        parent: None,
        "store.http.request",
        otel.kind = "server",
        otel.status_code = tracing::field::Empty,
        ddd.bounded_context.name = "player_sessions",
        network.transport = "tcp",
        network.peer.address = tracing::field::Empty,
        network.peer.port = tracing::field::Empty,
        http.request.method = request.method().as_str(),
        http.route = request.extensions().get::<MatchedPath>().map(MatchedPath::as_str),
        http.response.status_code = tracing::field::Empty,
    );
    crate::platform::http::set_remote_trace_parent(&span, request.headers());
    async move {
        let span = tracing::Span::current();
        if let Some(peer) = request.extensions().get::<SocketAddr>() {
            span.record("network.peer.address", tracing::field::display(peer.ip()));
            span.record("network.peer.port", peer.port());
        }
        let mut response = next.run(request).await;
        span.record("http.response.status_code", response.status().as_u16());
        if response.status().is_server_error() {
            span.record("otel.status_code", "ERROR");
            tracing::error!(status = response.status().as_u16(), "Store request failed");
        }
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
    .instrument(span)
    .await
}

#[tracing::instrument(name = "player_sessions.store.catalog_request", skip_all, fields(ddd.bounded_context.name = "player_sessions", http.route = route.as_str(), input.channel = channel(player.as_ref()), user.id = tracing::field::Empty, output.outcome = tracing::field::Empty, otel.status_code = tracing::field::Empty))]
async fn catalog(
    State(store): State<StoreRequests>,
    route: MatchedPath,
    player: Option<Extension<DashboardStorePlayer>>,
    headers: HeaderMap,
) -> Response {
    respond(&store, player, &headers, None).await
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct OrderPayload {
    item: i32,
    revision: String,
    price: StorePrice,
}

impl OrderPayload {
    fn into_order(self) -> Option<StoreOrder> {
        Some(StoreOrder {
            item: self.item,
            revision: self.revision.parse().ok()?,
            price: self.price,
        })
    }
}

#[tracing::instrument(name = "player_sessions.store.purchase_request", skip_all, fields(ddd.bounded_context.name = "player_sessions", http.route = route.as_str(), input.channel = channel(player.as_ref()), input.item = tracing::field::Empty, user.id = tracing::field::Empty, output.outcome = tracing::field::Empty, otel.status_code = tracing::field::Empty))]
async fn purchase(
    State(store): State<StoreRequests>,
    route: MatchedPath,
    player: Option<Extension<DashboardStorePlayer>>,
    headers: HeaderMap,
    order: Result<Json<OrderPayload>, JsonRejection>,
) -> Response {
    let Some(order) = order.ok().and_then(|Json(order)| order.into_order()) else {
        tracing::Span::current().record("output.outcome", "invalid_order");
        return (StatusCode::BAD_REQUEST, "Invalid order.").into_response();
    };
    tracing::Span::current().record("input.item", order.item);
    respond(&store, player, &headers, Some(order)).await
}

fn channel(player: Option<&Extension<DashboardStorePlayer>>) -> &'static str {
    if player.is_some() {
        "dashboard"
    } else {
        "garage"
    }
}

async fn respond(
    store: &StoreRequests,
    player: Option<Extension<DashboardStorePlayer>>,
    headers: &HeaderMap,
    order: Option<StoreOrder>,
) -> Response {
    let customer = if let Some(Extension(player)) = player {
        Some(StoreCustomer::Dashboard(player.0))
    } else {
        headers
            .get(header::AUTHORIZATION)
            .and_then(|header| header.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .and_then(StoreToken::parse)
            .map(StoreCustomer::Garage)
    };
    let result = match customer {
        Some(customer) => store.request(customer, order).await,
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
                    "insufficient_funds",
                    "You need more funds for this offer.",
                ),
                StoreError::Rejected(StoreOrderError::ResearchRequired) => (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "research_required",
                    "Research a Tech Tree vehicle of this tier or higher in the same nation.",
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
