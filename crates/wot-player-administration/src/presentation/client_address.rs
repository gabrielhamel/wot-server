#[cfg(test)]
mod tests {
    use crate::presentation::set_browser_headers;
    use axum::{Router, body::Body, http::Request, middleware, routing::get};
    use hyper::service::Service;
    use hyper_util::service::TowerToHyperService;
    use opentelemetry::trace::{SpanId, TraceId, TracerProvider};
    use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};
    use std::net::{IpAddr, SocketAddr};
    use tracing_subscriber::prelude::*;

    #[tokio::test]
    async fn exports_client_ip_through_http_listener_with_safe_proxy_fallbacks() {
        let exporter = InMemorySpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let subscriber = tracing_subscriber::registry().with(
            tracing_opentelemetry::layer().with_tracer(provider.tracer("client-address-test")),
        );
        let _guard = tracing::subscriber::set_default(subscriber);
        let peer: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        for (trusted, forwarded, expected, status, forwarded_port) in [
            (false, vec!["198.51.100.7"], "127.0.0.1", 200, false),
            (true, vec!["198.51.100.7"], "198.51.100.7", 200, true),
            (true, vec!["2001:db8::7"], "2001:db8::7", 401, true),
            (
                true,
                vec!["198.51.100.7, 192.0.2.1"],
                "127.0.0.1",
                401,
                false,
            ),
            (
                true,
                vec!["198.51.100.7", "192.0.2.1"],
                "127.0.0.1",
                200,
                false,
            ),
            (
                true,
                vec!["malformed-private-value"],
                "127.0.0.1",
                200,
                false,
            ),
            (true, vec![], "127.0.0.1", 200, false),
        ] {
            let trusted = if trusted { vec![peer.ip()] } else { vec![] };
            let router = Router::new()
                .route(
                    "/",
                    get(move || async move {
                        let span = tracing::info_span!(
                            "test.handler",
                            "ddd.bounded_context.name" = "player_administration"
                        );
                        span.in_scope(|| axum::http::StatusCode::from_u16(status).unwrap())
                    }),
                )
                .layer(middleware::from_fn_with_state(trusted, set_browser_headers));
            let service = TowerToHyperService::new(router.layer(axum::Extension(peer)));
            let mut request = Request::builder().uri("/");
            for value in forwarded {
                request = request.header("x-forwarded-for", value);
            }
            request = request
                .header("forwarded", "for=203.0.113.99")
                .header("x-real-ip", "203.0.113.99");
            let response = service
                .call(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), status);
            let spans = exporter.get_finished_spans().unwrap();
            let root = spans
                .iter()
                .rev()
                .find(|span| span.name == "admin.http.request")
                .unwrap();
            let attr = |key: &str| {
                root.attributes
                    .iter()
                    .find(|a| a.key.as_str() == key)
                    .map(|a| a.value.to_string())
            };
            assert_eq!(attr("client.address").as_deref(), Some(expected));
            assert_eq!(attr("network.peer.address").as_deref(), Some("127.0.0.1"));
            assert_eq!(attr("network.peer.port").as_deref(), Some("12345"));
            assert_eq!(
                attr("client.port").as_deref(),
                if forwarded_port { None } else { Some("12345") }
            );
            assert_eq!(attr("http.response.status_code"), Some(status.to_string()));
            assert_eq!(root.parent_span_id, SpanId::INVALID);
            assert!(spans.iter().any(|span| span.name == "test.handler"
                && span.parent_span_id == root.span_context.span_id()));
            assert!(!format!("{spans:?}").contains("malformed-private-value"));
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router =
            Router::new()
                .route("/", get(|| async { "ok" }))
                .layer(middleware::from_fn_with_state(
                    Vec::<IpAddr>::new(),
                    set_browser_headers,
                ));
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(wot_platform::http::serve(
            listener, router, "admin", stopped,
        ));
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        let client = stream.local_addr().unwrap();
        stream.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\ntraceparent: 00-abcdef0123456789abcdef0123456789-1234567890abcdef-01\r\nX-Forwarded-For: 203.0.113.99\r\n\r\n").await.unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();
        stop.send(()).unwrap();
        server.await.unwrap();
        assert!(response.starts_with(b"HTTP/1.1 200"));
        let spans = exporter.get_finished_spans().unwrap();
        let root = spans
            .iter()
            .rev()
            .find(|span| span.name == "admin.http.request")
            .unwrap();
        assert_eq!(
            root.parent_span_id,
            SpanId::from_hex("1234567890abcdef").unwrap()
        );
        assert_eq!(
            root.span_context.trace_id(),
            TraceId::from_hex("abcdef0123456789abcdef0123456789").unwrap()
        );
        assert!(
            root.attributes
                .iter()
                .any(|a| a.key.as_str() == "client.port"
                    && a.value.to_string() == client.port().to_string())
        );
    }
}
