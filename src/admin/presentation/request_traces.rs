use super::*;
use crate::client_gateway::GrantRequestError;
use axum::body::Body;
use hyper::service::Service;
use hyper_util::service::TowerToHyperService;
use opentelemetry::trace::{SpanId, TracerProvider};
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider, SpanData};
use tracing::instrument::WithSubscriber;
use tracing_subscriber::prelude::*;

fn attribute(span: &SpanData, key: &str) -> Option<String> {
    span.attributes
        .iter()
        .find(|attribute| attribute.key.as_str() == key)
        .map(|attribute| attribute.value.to_string())
}

#[tokio::test]
#[ignore = "Requires WG_TEST_DATABASE_URL and WG_RESOURCE_DIR"]
async fn exports_admin_http_request_inputs() {
    let url = std::env::var("WG_TEST_DATABASE_URL").unwrap();
    let resource_dir = std::env::var_os("WG_RESOURCE_DIR").unwrap();
    let resources = wot_resources::ResourceBundle::open(Path::new(&resource_dir)).unwrap();
    let catalog = Arc::new(GarageCatalog::load(resources, &[]).unwrap());
    let repository = PostgresAccounts::connect(&url, 2).await.unwrap();
    let players = PostgresPlayers::new(&repository, catalog.clone()).unwrap();
    let (grants, mut queued) = PlayerGrants::channel();
    let service = TowerToHyperService::new(router(
        PlayerAccounts::new(players),
        Accounts::new(repository),
        catalog,
        grants,
        AdminToken::new("test-bearer-secret"),
        None,
    ));

    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    let subscriber = tracing_subscriber::registry()
        .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("admin-http-test")));
    let send = |method, uri, token, body: &'static str| {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, token)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CONTENT_LENGTH, body.len().to_string())
            .body(Body::from(body))
            .unwrap();
        service.call(request)
    };
    const TOKEN: &str = "Bearer test-bearer-secret";
    const PLAYER: &str = r#"{"username":"private-login@example.test","display_name":"Private Display","password":"test-password-secret\n"}"#;
    async {
        for (method, uri, token, body, status) in [
            (
                "GET",
                "/api/admin/players?page=2&per_page=7",
                TOKEN,
                "",
                200,
            ),
            ("GET", "/api/admin/players?page=3", TOKEN, "", 200),
            (
                "GET",
                "/api/admin/players?page=query-secret",
                TOKEN,
                "",
                400,
            ),
            (
                "GET",
                "/api/admin/players?page=9",
                "Bearer unauthorized-secret",
                "",
                401,
            ),
            ("POST", "/api/admin/players", TOKEN, PLAYER, 422),
            (
                "POST",
                "/api/admin/players",
                TOKEN,
                r#"{"password":"malformed-body-secret"}"#,
                422,
            ),
        ] {
            assert_eq!(
                send(method, uri, token, body).await.unwrap().status(),
                status
            );
        }
        for body in [
            r#"{"kind":"currency","currency":"gold","amount":42}"#,
            r#"{"kind":"vehicle","vehicle":123}"#,
        ] {
            let response = send("POST", "/api/admin/players/17/grants", TOKEN, body);
            let (response, ()) = tokio::join!(response, async {
                let request =
                    tokio::time::timeout(std::time::Duration::from_secs(5), queued.recv())
                        .await
                        .expect("grant route must reach the queue")
                        .unwrap();
                request
                    .reply
                    .send(Err(GrantRequestError::NotFound))
                    .unwrap();
            });
            assert_eq!(response.unwrap().status(), 404);
        }
    }
    .with_subscriber(subscriber)
    .await;

    let spans = exporter.get_finished_spans().unwrap();
    let roots: Vec<_> = spans
        .iter()
        .filter(|span| span.name == "admin.http.request")
        .collect();
    assert_eq!(roots.len(), 8);
    for (root, expected) in roots.iter().zip([
        &[
            ("http.response.status_code", "200"),
            ("input.page", "2"),
            ("input.per_page", "7"),
            ("http.route", "/api/admin/players"),
        ][..],
        &[("http.response.status_code", "200"), ("input.page", "3")],
        &[
            ("http.response.status_code", "400"),
            ("input.query.bytes", "17"),
        ],
        &[("http.response.status_code", "401")],
        &[
            ("http.response.status_code", "422"),
            ("input.username.bytes", "26"),
            ("input.display_name.bytes", "15"),
        ],
        &[("http.response.status_code", "422")],
        &[
            ("http.response.status_code", "404"),
            ("input.account_id", "17"),
            ("input.grant.kind", "currency"),
            ("input.grant.currency", "gold"),
            ("input.amount", "42"),
            ("http.route", "/api/admin/players/{id}/grants"),
        ],
        &[
            ("http.response.status_code", "404"),
            ("input.grant.kind", "vehicle"),
            ("input.vehicle_type", "123"),
        ],
    ]) {
        assert_eq!(root.parent_span_id, SpanId::INVALID);
        for (key, value) in expected {
            assert_eq!(attribute(root, key).as_deref(), Some(*value), "{key}");
        }
    }
    assert_eq!(attribute(roots[1], "input.per_page"), None);
    for root in &roots[2..4] {
        assert_eq!(attribute(root, "input.page"), None);
    }
    assert_eq!(attribute(roots[5], "input.username.bytes"), None);
    assert_eq!(
        attribute(roots[4], "input.content_length"),
        Some(PLAYER.len().to_string())
    );
    let contains_operation = |root: &SpanData, operation: &str| {
        spans
            .iter()
            .filter(|span| span.name == operation)
            .any(|span| {
                if span.span_context.trace_id() != root.span_context.trace_id() {
                    return false;
                }
                let mut parent = span.parent_span_id;
                for _ in 0..spans.len() {
                    if parent == root.span_context.span_id() {
                        return true;
                    }
                    let Some(ancestor) = spans
                        .iter()
                        .find(|span| span.span_context.span_id() == parent)
                    else {
                        break;
                    };
                    parent = ancestor.parent_span_id;
                }
                false
            })
    };
    assert!(contains_operation(roots[0], "admin.players.list"));
    assert!(contains_operation(roots[0], "postgresql.query"));
    assert!(contains_operation(roots[6], "admin.garage.grant"));
    assert!(contains_operation(roots[6], "client_gateway.grants.grant"));
    let exported = format!("{spans:?}");
    for excluded in [
        "test-bearer-secret",
        "unauthorized-secret",
        "test-password-secret",
        "private-login@example.test",
        "Private Display",
        "query-secret",
        "malformed-body-secret",
    ] {
        assert!(!exported.contains(excluded), "exported {excluded}");
    }
}
