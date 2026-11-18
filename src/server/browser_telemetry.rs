use anyhow::{Context, Result, ensure};
use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode, header},
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

const CONTENT_TYPE: &str = "application/x-protobuf";
const MAX_BODY: usize = 32 * 1024;
const MAX_SPANS: usize = 24;
const BURST_LIMIT: u32 = 30;
const RATE_WINDOW: Duration = Duration::from_secs(10);

struct Relay {
    origin: String,
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
            "WG_WEB_TELEMETRY_ORIGIN must be an HTTP origin"
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
            origin,
            endpoint,
            client,
            active: Semaphore::new(4),
            window: Mutex::new((Instant::now(), 0)),
        })
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

pub(super) fn from_environment() -> Result<Option<Router>> {
    let origin = match env::var("WG_WEB_TELEMETRY_ORIGIN") {
        Ok(origin) if !origin.is_empty() => origin,
        Ok(_) | Err(env::VarError::NotPresent) => return Ok(None),
        Err(error) => return Err(error).context("Read WG_WEB_TELEMETRY_ORIGIN"),
    };
    let endpoint =
        env::var("WG_WEB_TELEMETRY_ENDPOINT").context("Read WG_WEB_TELEMETRY_ENDPOINT")?;
    Ok(Some(router(Relay::new(origin, Url::parse(&endpoint)?)?)))
}

fn router(relay: Relay) -> Router {
    Router::new()
        .route(
            "/api/web/telemetry",
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

async fn ingest(State(relay): State<Arc<Relay>>, headers: HeaderMap, body: Bytes) -> StatusCode {
    if headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        != Some(&relay.origin)
    {
        return StatusCode::FORBIDDEN;
    }
    if headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        != Some(CONTENT_TYPE)
    {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE;
    }
    let Ok(_permit) = relay.active.try_acquire() else {
        return StatusCode::TOO_MANY_REQUESTS;
    };
    if !relay.take_budget() {
        return StatusCode::TOO_MANY_REQUESTS;
    }

    let Some(traces) = browser_traces(&body) else {
        return StatusCode::BAD_REQUEST;
    };
    let response = relay
        .client
        .post(relay.endpoint.clone())
        .header(header::CONTENT_TYPE, CONTENT_TYPE)
        .body(traces.encode_to_vec())
        .send()
        .await;
    match response {
        Ok(response) if response.status().is_success() => StatusCode::OK,
        Ok(response) => {
            tracing::warn!(
                status = response.status().as_u16(),
                "Collector rejected browser traces"
            );
            StatusCode::BAD_GATEWAY
        }
        Err(error) => {
            tracing::warn!(
                timeout = error.is_timeout(),
                "Could not forward browser traces"
            );
            StatusCode::BAD_GATEWAY
        }
    }
}

fn browser_traces(bytes: &[u8]) -> Option<ExportTraceServiceRequest> {
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
    Some(traces)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request, routing::post};
    use opentelemetry_proto::tonic::trace::v1::{ResourceSpans, ScopeSpans, Span};
    use tower::ServiceExt;

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
                            received.send(bytes).await.unwrap();
                            StatusCode::OK
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
        let send = |origin, content_type, body| {
            router.clone().oneshot(
                Request::post("/api/web/telemetry")
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
        let wire = ExportTraceServiceRequest {
            resource_spans: vec![ResourceSpans {
                scope_spans: vec![ScopeSpans {
                    spans: vec![span.clone()],
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
            send("https://example.test", CONTENT_TYPE, wire)
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        let bytes = collector.recv().await.unwrap();
        let forwarded = ExportTraceServiceRequest::decode(bytes).unwrap();
        let resource = &forwarded.resource_spans[0];
        assert_eq!(resource.scope_spans[0].spans, vec![span]);
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

        for _ in 1..BURST_LIMIT {
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
        upstream.abort();
    }
}
