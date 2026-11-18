use anyhow::{Result, ensure};
use axum::{
    Router,
    extract::{MatchedPath, Request},
    http::{HeaderValue, header},
    middleware::{Next, from_fn},
    response::{Redirect, Response},
    routing::get,
};
use std::{net::SocketAddr, path::Path};
use tower_http::services::{ServeDir, ServeFile};
use tracing::{Span, field::Empty};

pub(super) fn pages(directory: &Path, admin: bool, shop: bool) -> Result<Router> {
    let index = directory.join("index.html");
    let assets = directory.join("assets");
    ensure!(
        index.is_file() && assets.is_dir(),
        "Build the web UI before using {}",
        directory.display()
    );

    let mut router = Router::new().nest_service(
        "/assets",
        ServeDir::new(assets).append_index_html_on_directories(false),
    );
    if admin {
        router = router
            .route("/", get(|| async { Redirect::temporary("/admin/") }))
            .route_service("/admin", ServeFile::new(&index))
            .route_service("/admin/", ServeFile::new(&index));
    }
    if shop {
        router = router
            .route_service("/shop", ServeFile::new(&index))
            .route_service("/shop/", ServeFile::new(&index));
    }
    Ok(router.layer(from_fn(serve_page)))
}

#[tracing::instrument(
    name = "web.ui.request",
    skip_all,
    fields(
        ddd.bounded_context.name = "server",
        network.transport = "tcp",
        network.peer.address = Empty,
        network.peer.port = Empty,
        http.request.method = request.method().as_str(),
        http.route = request.extensions().get::<MatchedPath>().map(MatchedPath::as_str),
        http.response.status_code = Empty,
    )
)]
async fn serve_page(request: Request, next: Next) -> Response {
    let admin = request.uri().path().starts_with("/admin");
    let span = Span::current();
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
        (header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(if admin {
            "default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'"
        } else {
            "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'"
        })),
    ]);
    response
}
