use anyhow::{Context, Result, ensure};
use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, State, rejection::BytesRejection},
    http::{HeaderMap, HeaderName, StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::get,
};
use opentelemetry_proto::tonic::{
    collector::trace::v1::ExportTraceServiceRequest,
    common::v1::{AnyValue, KeyValue, any_value::Value},
    resource::v1::Resource,
};
use prost::Message;
use reqwest::{Client, Url, redirect::Policy};
use std::{
    env,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;

const UPLOAD_ROUTE: &str = "/api/web/telemetry";
const FETCH_SITE: HeaderName = HeaderName::from_static("sec-fetch-site");
const CONTENT_TYPE: &str = "application/x-protobuf";
const MAX_BODY: usize = 32 * 1024;
const MAX_SPANS: usize = 24;
const BURST_LIMIT: u32 = 30;
const RATE_WINDOW: Duration = Duration::from_secs(10);

struct Relay {
    origin: String,
    authority: String,
    endpoint: Url,
    client: Client,
    active: Semaphore,
    window: Mutex<(Instant, u32)>,
}

impl Relay {
    fn new(origin: String, endpoint: Url) -> Result<Self> {
        let parsed = Url::parse(&origin).context("Parse browser telemetry origin")?;
        ensure!(
            matches!(parsed.scheme(), "http" | "https")
                && parsed.origin().ascii_serialization() == origin,
            "WOT_WEB_TELEMETRY_ORIGIN must be an HTTP origin"
        );
        ensure!(
            matches!(endpoint.scheme(), "http" | "https") && endpoint.host().is_some(),
            "Browser telemetry endpoint must be HTTP or HTTPS"
        );
        let client = Client::builder()
            .redirect(Policy::none())
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()?;
        Ok(Self {
            authority: parsed.authority().to_owned(),
            origin,
            endpoint,
            client,
            active: Semaphore::new(4),
            window: Mutex::new((Instant::now(), 0)),
        })
    }

    fn accepts_origin(&self, uri: &Uri, headers: &HeaderMap) -> bool {
        let mut origins = headers.get_all(header::ORIGIN).iter();
        match (origins.next(), origins.next()) {
            (Some(origin), None) if origin == self.origin.as_str() => return true,
            (Some(origin), None) if origin == "null" => {}
            (None, None) => {}
            _ => return false,
        }

        let mut sites = headers.get_all(FETCH_SITE).iter();
        if !sites.next().is_some_and(|site| site == "same-origin") || sites.next().is_some() {
            return false;
        }

        let mut hosts = headers.get_all(header::HOST).iter();
        match (uri.authority(), hosts.next(), hosts.next()) {
            (Some(authority), None, None) => {
                authority.as_str().eq_ignore_ascii_case(&self.authority)
            }
            (None, Some(host), None) => host
                .as_bytes()
                .eq_ignore_ascii_case(self.authority.as_bytes()),
            _ => false,
        }
    }

    fn take_budget(&self) -> bool {
        let Ok(mut window) = self.window.lock() else {
            return false;
        };
        if window.0.elapsed() >= RATE_WINDOW {
            *window = (Instant::now(), 0);
        }
        if window.1 >= BURST_LIMIT {
            return false;
        }
        window.1 += 1;
        true
    }
}

pub(crate) fn from_environment() -> Result<Option<Router>> {
    let origin = match env::var("WOT_WEB_TELEMETRY_ORIGIN") {
        Ok(origin) if !origin.is_empty() => origin,
        Ok(_) | Err(env::VarError::NotPresent) => return Ok(None),
        Err(error) => return Err(error).context("Read WOT_WEB_TELEMETRY_ORIGIN"),
    };
    let endpoint =
        env::var("WOT_WEB_TELEMETRY_ENDPOINT").context("Read WOT_WEB_TELEMETRY_ENDPOINT")?;
    Ok(Some(router(Relay::new(origin, Url::parse(&endpoint)?)?)))
}

fn router(relay: Relay) -> Router {
    Router::new()
        .route(
            UPLOAD_ROUTE,
            get(|| async {
                (
                    [(header::CACHE_CONTROL, "no-store")],
                    StatusCode::NO_CONTENT,
                )
            })
            .post(ingest),
        )
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .with_state(Arc::new(relay))
}

#[tracing::instrument(
    name = "browser.telemetry.ingest",
    skip_all,
    fields(
        ddd.bounded_context.name = "server",
        otel.kind = "server",
        http.request.method = "POST",
        http.route = UPLOAD_ROUTE,
        input.body.bytes = tracing::field::Empty,
        input.span_count = tracing::field::Empty,
        output.outcome = tracing::field::Empty,
        http.response.status_code = tracing::field::Empty,
        upstream.http.response.status_code = tracing::field::Empty,
        otel.status_code = tracing::field::Empty,
    )
)]
async fn ingest(
    State(relay): State<Arc<Relay>>,
    uri: Uri,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let body = match body {
        Ok(body) => body,
        Err(rejection) => return finish_upload("body_rejected", rejection.into_response()),
    };
    tracing::Span::current().record("input.body.bytes", body.len() as i64);
    let (outcome, status) = forward_upload(&relay, &uri, &headers, &body).await;
    finish_upload(outcome, status.into_response())
}

fn finish_upload(outcome: &'static str, response: Response) -> Response {
    let span = tracing::Span::current();
    span.record("output.outcome", outcome);
    span.record(
        "http.response.status_code",
        i64::from(response.status().as_u16()),
    );
    if response.status().is_server_error() {
        span.record("otel.status_code", "ERROR");
        tracing::warn!(outcome, "Browser telemetry forwarding failed");
    }
    response
}

async fn forward_upload(
    relay: &Relay,
    uri: &Uri,
    headers: &HeaderMap,
    body: &[u8],
) -> (&'static str, StatusCode) {
    if !relay.accepts_origin(uri, headers) {
        return ("origin_rejected", StatusCode::FORBIDDEN);
    }
    if headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        != Some(CONTENT_TYPE)
    {
        return ("content_type_rejected", StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    let Ok(_permit) = relay.active.try_acquire() else {
        return ("concurrency_limited", StatusCode::TOO_MANY_REQUESTS);
    };
    if !relay.take_budget() {
        return ("rate_limited", StatusCode::TOO_MANY_REQUESTS);
    }

    let Some((traces, span_count)) = browser_traces(body) else {
        return ("batch_rejected", StatusCode::BAD_REQUEST);
    };
    tracing::Span::current().record("input.span_count", span_count as i64);
    let response = relay
        .client
        .post(relay.endpoint.clone())
        .header(header::CONTENT_TYPE, CONTENT_TYPE)
        .body(traces.encode_to_vec())
        .send()
        .await;
    match response {
        Ok(response) => {
            tracing::Span::current().record(
                "upstream.http.response.status_code",
                i64::from(response.status().as_u16()),
            );
            if response.status().is_success() {
                ("forwarded", StatusCode::OK)
            } else {
                ("collector_rejected", StatusCode::BAD_GATEWAY)
            }
        }
        Err(error) if error.is_timeout() => ("collector_timeout", StatusCode::BAD_GATEWAY),
        Err(_) => ("collector_unreachable", StatusCode::BAD_GATEWAY),
    }
}

fn browser_traces(bytes: &[u8]) -> Option<(ExportTraceServiceRequest, usize)> {
    let mut traces = ExportTraceServiceRequest::decode(bytes).ok()?;
    let [resource] = traces.resource_spans.as_mut_slice() else {
        return None;
    };
    let [scope] = resource.scope_spans.as_mut_slice() else {
        return None;
    };
    if scope.spans.is_empty() || scope.spans.len() > MAX_SPANS {
        return None;
    }
    let attribute = |key: &str, value: &str| KeyValue {
        key: key.into(),
        value: Some(AnyValue {
            value: Some(Value::StringValue(value.into())),
        }),
        ..Default::default()
    };
    let version = option_env!("WOT_SERVER_VERSION")
        .filter(|version| !version.is_empty())
        .unwrap_or(env!("CARGO_PKG_VERSION"));
    resource.resource = Some(Resource {
        attributes: vec![
            attribute("service.name", "wot-server-web"),
            attribute("service.version", version),
        ],
        ..Default::default()
    });
    resource.schema_url.clear();
    scope.scope = None;
    scope.schema_url.clear();
    let span_count = scope.spans.len();
    Some((traces, span_count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request, routing::post};
    use opentelemetry::trace::TracerProvider;
    use opentelemetry_proto::tonic::trace::v1::{ResourceSpans, ScopeSpans, Span};
    use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};
    use tower::ServiceExt;
    use tracing_subscriber::prelude::*;

    #[test]
    fn accepts_opaque_or_missing_origin_only_for_the_configured_same_origin() {
        let relay = Relay::new(
            "https://example.test:8443".into(),
            Url::parse("http://localhost:4318/v1/traces").unwrap(),
        )
        .unwrap();
        let path: Uri = UPLOAD_ROUTE.parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "example.test:8443".parse().unwrap());
        assert!(!relay.accepts_origin(&path, &headers));
        headers.insert(FETCH_SITE, "same-origin".parse().unwrap());
        assert!(relay.accepts_origin(&path, &headers));

        headers.insert(header::ORIGIN, "https://other.test".parse().unwrap());
        assert!(!relay.accepts_origin(&path, &headers));
        headers.insert(header::ORIGIN, "null".parse().unwrap());
        assert!(relay.accepts_origin(&path, &headers));

        for site in ["same-site", "cross-site", "none"] {
            headers.insert(FETCH_SITE, site.parse().unwrap());
            assert!(!relay.accepts_origin(&path, &headers));
        }

        headers.remove(FETCH_SITE);
        assert!(!relay.accepts_origin(&path, &headers));
        headers.insert(FETCH_SITE, "same-origin".parse().unwrap());
        headers.append(FETCH_SITE, "same-origin".parse().unwrap());
        assert!(!relay.accepts_origin(&path, &headers));
        headers.insert(FETCH_SITE, "same-origin".parse().unwrap());

        headers.insert(header::HOST, "other.test:8443".parse().unwrap());
        assert!(!relay.accepts_origin(&path, &headers));
        headers.remove(header::HOST);
        assert!(!relay.accepts_origin(&path, &headers));
        let absolute = "https://example.test:8443/api/web/telemetry"
            .parse()
            .unwrap();
        assert!(relay.accepts_origin(&absolute, &headers));

        headers.append(header::ORIGIN, "null".parse().unwrap());
        assert!(!relay.accepts_origin(&absolute, &headers));
    }

    #[tokio::test]
    async fn forwards_browser_span_ids_with_server_assigned_resource_and_bounded_ingress() {
        let (received, collector) = tokio::sync::mpsc::channel(1);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1/traces", listener.local_addr().unwrap());
        let upstream = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route(
                    "/v1/traces",
                    post(move |bytes: Bytes| {
                        let received = received.clone();
                        async move {
                            if received.send(bytes).await.is_ok() {
                                StatusCode::OK
                            } else {
                                StatusCode::SERVICE_UNAVAILABLE
                            }
                        }
                    }),
                ),
            )
            .await
            .unwrap();
        });
        let router = router(
            Relay::new(
                "https://example.test".into(),
                Url::parse(&endpoint).unwrap(),
            )
            .unwrap(),
        );
        let exporter = InMemorySpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let subscriber = tracing_subscriber::registry().with(
            tracing_opentelemetry::layer().with_tracer(provider.tracer("browser-relay-test")),
        );
        let _subscriber = tracing::subscriber::set_default(subscriber);
        let send = |origin, content_type, body| {
            router.clone().oneshot(
                Request::post(UPLOAD_ROUTE)
                    .header(header::ORIGIN, origin)
                    .header(header::CONTENT_TYPE, content_type)
                    .body(Body::from(body))
                    .unwrap(),
            )
        };

        let mut collector = collector;
        let span = Span {
            trace_id: vec![9; 16],
            span_id: vec![8; 8],
            parent_span_id: vec![7; 8],
            name: "web.http.request".into(),
            ..Default::default()
        };
        let root = Span {
            trace_id: span.trace_id.clone(),
            span_id: span.parent_span_id.clone(),
            name: "web.admin.list_players".into(),
            ..Default::default()
        };
        let wire = ExportTraceServiceRequest {
            resource_spans: vec![ResourceSpans {
                scope_spans: vec![ScopeSpans {
                    spans: vec![root.clone(), span.clone()],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        }
        .encode_to_vec();
        assert_eq!(
            send("https://other.test", CONTENT_TYPE, wire.clone())
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            send("https://example.test", "text/plain", wire.clone())
                .await
                .unwrap()
                .status(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE
        );
        assert_eq!(
            send("https://example.test", CONTENT_TYPE, vec![0; MAX_BODY + 1])
                .await
                .unwrap()
                .status(),
            StatusCode::PAYLOAD_TOO_LARGE
        );
        assert_eq!(
            router
                .clone()
                .oneshot(
                    Request::post(UPLOAD_ROUTE)
                        .header(header::ORIGIN, "null")
                        .header(header::HOST, "example.test")
                        .header(FETCH_SITE, "same-origin")
                        .header(header::CONTENT_TYPE, CONTENT_TYPE)
                        .body(Body::from(wire))
                        .unwrap(),
                )
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        let bytes = collector.recv().await.unwrap();
        let forwarded = ExportTraceServiceRequest::decode(bytes).unwrap();
        let resource = &forwarded.resource_spans[0];
        assert_eq!(resource.scope_spans[0].spans, vec![root, span]);
        assert!(
            resource
                .resource
                .as_ref()
                .unwrap()
                .attributes
                .iter()
                .any(|attribute| {
                    attribute.key == "service.name"
                        && attribute.value.as_ref().unwrap().value
                            == Some(Value::StringValue("wot-server-web".into()))
                })
        );

        drop(collector);
        assert_eq!(
            send(
                "https://example.test",
                CONTENT_TYPE,
                forwarded.encode_to_vec()
            )
            .await
            .unwrap()
            .status(),
            StatusCode::BAD_GATEWAY
        );

        for _ in 2..BURST_LIMIT {
            assert_eq!(
                send("https://example.test", CONTENT_TYPE, vec![0])
                    .await
                    .unwrap()
                    .status(),
                StatusCode::BAD_REQUEST
            );
        }
        assert_eq!(
            send("https://example.test", CONTENT_TYPE, vec![0])
                .await
                .unwrap()
                .status(),
            StatusCode::TOO_MANY_REQUESTS
        );
        let spans = exporter.get_finished_spans().unwrap();
        for (outcome, status) in [
            ("origin_rejected", 403),
            ("content_type_rejected", 415),
            ("body_rejected", 413),
            ("forwarded", 200),
            ("collector_rejected", 502),
            ("batch_rejected", 400),
            ("rate_limited", 429),
        ] {
            let upload = spans
                .iter()
                .find(|span| {
                    span.name == "browser.telemetry.ingest"
                        && span.attributes.iter().any(|attribute| {
                            attribute.key.as_str() == "output.outcome"
                                && attribute.value.as_str() == outcome
                        })
                })
                .unwrap();
            assert!(upload.attributes.iter().any(|attribute| {
                attribute.key.as_str() == "http.response.status_code"
                    && attribute.value == opentelemetry::Value::I64(status)
            }));
            if status == 502 {
                assert!(matches!(
                    upload.status,
                    opentelemetry::trace::Status::Error { .. }
                ));
                assert!(upload.attributes.iter().any(|attribute| {
                    attribute.key.as_str() == "upstream.http.response.status_code"
                        && attribute.value == opentelemetry::Value::I64(503)
                }));
            }
        }
        upstream.abort();
    }
}
