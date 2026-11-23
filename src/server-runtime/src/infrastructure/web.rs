use anyhow::{Result, ensure};
use axum::{
    Router,
    body::Bytes,
    extract::{MatchedPath, Path as RoutePath, Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::{Next, from_fn_with_state},
    response::{IntoResponse, Redirect, Response},
    routing::{get, get_service},
};
use std::{
    net::{IpAddr, SocketAddr},
    path::Path,
};
use tower_http::services::{ServeDir, ServeFile};
use tracing::{Instrument, Span, field::Empty};

mod bundle {
    include!(concat!(env!("OUT_DIR"), "/web-bundle/bundle.rs"));
}

#[derive(Clone)]
struct PageState {
    trusted_proxies: Vec<IpAddr>,
    asset_source: &'static str,
}

pub(crate) fn pages(
    directory: Option<&Path>,
    dashboard: bool,
    shop: bool,
    trusted_proxies: Vec<IpAddr>,
) -> Result<Router> {
    let (mut router, index, asset_source) = if let Some(directory) = directory {
        let index = directory.join("index.html");
        let assets = directory.join("assets");
        ensure!(
            index.is_file() && assets.is_dir(),
            "Build the web UI before using {}",
            directory.display()
        );
        (
            Router::new().nest_service(
                "/assets",
                ServeDir::new(assets).append_index_html_on_directories(false),
            ),
            get_service(ServeFile::new(index)),
            "directory",
        )
    } else {
        (
            Router::new().route("/assets/{*path}", get(embedded_asset)),
            get(embedded_index),
            "bundle",
        )
    };
    if dashboard {
        router = router
            .route("/", get(|| async { Redirect::temporary("/dashboard/") }))
            .route("/dashboard", index.clone())
            .route("/dashboard/", index.clone());
    }
    if shop {
        router = router.route("/shop", index.clone()).route("/shop/", index);
    }
    let state = PageState {
        trusted_proxies,
        asset_source,
    };
    Ok(router.layer(from_fn_with_state(state, serve_page)))
}

async fn embedded_index() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        Bytes::from_static(bundle::INDEX),
    )
}

async fn embedded_asset(RoutePath(path): RoutePath<String>) -> Response {
    let Some(bytes) = bundle::asset(&format!("assets/{path}")) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    (
        [(header::CONTENT_TYPE, mime.essence_str())],
        Bytes::from_static(bytes),
    )
        .into_response()
}

async fn serve_page(State(state): State<PageState>, request: Request, next: Next) -> Response {
    let span = tracing::info_span!(
        parent: None,
        "web.ui.request",
        otel.kind = "server",
        otel.status_code = Empty,
        ddd.bounded_context.name = "server",
        network.transport = "tcp",
        client.address = Empty,
        client.port = Empty,
        network.peer.address = Empty,
        network.peer.port = Empty,
        input.web.asset_source = state.asset_source,
        http.request.method = request.method().as_str(),
        http.route = request.extensions().get::<MatchedPath>().map(MatchedPath::as_str),
        http.response.status_code = Empty,
    );
    wot_platform::http::set_remote_trace_parent(&span, request.headers());
    async move {
        let dashboard = matches!(request.uri().path(), "/dashboard" | "/dashboard/");
        if let Some(peer) = request.extensions().get::<SocketAddr>().copied() {
            wot_platform::http::record_client_address(peer, request.headers(), &state.trusted_proxies);
        }
        let mut response = next.run(request).await;
        let status = response.status();
        let span = Span::current();
        span.record("http.response.status_code", i64::from(status.as_u16()));
        if status.is_server_error() {
            span.record("otel.status_code", "ERROR");
            tracing::error!(%status, "Web UI request failed");
        }
        response.headers_mut().extend([
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
            (header::REFERRER_POLICY, HeaderValue::from_static("no-referrer")),
            (header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")),
            (header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(if dashboard {
                "default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'"
            } else {
                "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; font-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'"
            })),
        ]);
        response
    }
    .instrument(span)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use opentelemetry::trace::{SpanId, TraceId, TracerProvider};
    use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};
    use tower::ServiceExt;
    use tracing_subscriber::prelude::*;

    #[tokio::test]
    async fn serves_embedded_pages_and_assets_with_request_traces() {
        let exporter = InMemorySpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let subscriber = tracing_subscriber::registry()
            .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("web-test")));
        let _subscriber = tracing::subscriber::set_default(subscriber);
        let router = pages(None, true, true, Vec::new()).unwrap();
        let fetch = |path: &str| {
            router.clone().oneshot(
                Request::get(path)
                    .header(
                        "traceparent",
                        "00-01010101010101010101010101010101-0202020202020202-01",
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
        };

        let dashboard = fetch("/dashboard/").await.unwrap();
        assert_eq!(dashboard.status(), StatusCode::OK);
        assert_eq!(
            dashboard.headers()[header::CONTENT_TYPE],
            "text/html; charset=utf-8"
        );
        assert_eq!(dashboard.headers()[header::CACHE_CONTROL], "no-store");
        let body = to_bytes(dashboard.into_body(), 64 * 1024).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        let (_, script) = html.split_once("src=\"/assets/").unwrap();
        let (script, _) = script.split_once('"').unwrap();
        let asset = fetch(&format!("/assets/{script}")).await.unwrap();
        assert_eq!(asset.status(), StatusCode::OK);
        assert!(
            asset.headers()[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .contains("javascript")
        );
        assert_eq!(fetch("/shop/").await.unwrap().status(), StatusCode::OK);
        assert_eq!(
            fetch("/assets/missing.js").await.unwrap().status(),
            StatusCode::NOT_FOUND
        );

        let spans = exporter.get_finished_spans().unwrap();
        for (route, status) in [
            ("/dashboard/", 200),
            ("/shop/", 200),
            ("/assets/{*path}", 404),
        ] {
            let span = spans
                .iter()
                .find(|span| {
                    span.name == "web.ui.request"
                        && span.attributes.iter().any(|attribute| {
                            attribute.key.as_str() == "http.route"
                                && attribute.value.as_str() == route
                        })
                        && span.attributes.iter().any(|attribute| {
                            attribute.key.as_str() == "http.response.status_code"
                                && attribute.value == opentelemetry::Value::I64(status)
                        })
                })
                .unwrap_or_else(|| panic!("Missing span for {route} ({status}): {spans:?}"));
            assert_eq!(span.span_context.trace_id(), TraceId::from_bytes([1; 16]));
            assert_eq!(span.parent_span_id, SpanId::from_bytes([2; 8]));
            assert!(span.attributes.iter().any(|attribute| {
                attribute.key.as_str() == "input.web.asset_source"
                    && attribute.value.as_str() == "bundle"
            }));
        }
    }
}
