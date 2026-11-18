use super::*;
use crate::player_sessions::GrantRequestError;
use axum::body::{Body, to_bytes};
use hyper::service::Service;
use hyper_util::service::TowerToHyperService;
use opentelemetry::trace::{SpanId, TracerProvider};
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider, SpanData};
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
    let has_attachments =
        resources.has_file("scripts/item_defs/customization/attachments/list.xml");
    let catalog = Arc::new(GarageCatalog::load(resources, &[]).unwrap());
    let attachment_count = catalog.grantable_attachments().count();
    let tracker_count = catalog.grantable_stat_trackers().count();
    let starter_keys = catalog.tier_one_keys().unwrap();
    let repository = PostgresAccounts::connect(&url, 2).await.unwrap();
    let accounts = Accounts::new(repository.clone());
    let players = PostgresPlayers::new(&repository, catalog.clone()).unwrap();
    let (grants, mut queued) = PlayerGrants::channel();
    let service = TowerToHyperService::new(router(
        PlayerAccounts::new(players),
        accounts.clone(),
        catalog,
        grants,
        AdminToken::new("test-bearer-secret"),
        None,
        vec!["192.0.2.10".parse().unwrap()],
    ));

    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    let subscriber = tracing_subscriber::registry()
        .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("admin-http-test")));
    // This test uses the current-thread runtime, including the spawned creation task.
    let subscriber_guard = tracing::subscriber::set_default(subscriber);
    let send = |method, uri, token, body: &str| {
        let request = Request::builder()
            .extension("192.0.2.10:2345".parse::<SocketAddr>().unwrap())
            .header("x-forwarded-for", "198.51.100.7")
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, token)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CONTENT_LENGTH, body.len().to_string())
            .body(Body::from(body.to_owned()))
            .unwrap();
        service.call(request)
    };
    const TOKEN: &str = "Bearer test-bearer-secret";
    const PLAYER: &str = r#"{"username":"private-login@example.test","display_name":"Private Display","password":"test-password-secret\n"}"#;
    const PASSWORD: &str = "test-valid-password-secret";
    let username = format!(
        "admin_test_{}@example.test",
        std::time::UNIX_EPOCH.elapsed().unwrap().as_nanos()
    );
    {
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
            ("GET", "/api/admin/players/0/garage", TOKEN, "", 404),
        ] {
            assert_eq!(
                send(method, uri, token, body).await.unwrap().status(),
                status
            );
        }
        for body in [
            r#"{"kind":"currency","currency":"gold","amount":42}"#,
            r#"{"kind":"vehicle","vehicle":123}"#,
            r#"{"kind":"attachment","id":2033,"count":1}"#,
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
    let body = serde_json::json!({
        "username": username,
        "display_name": "Dashboard Player",
        "password": PASSWORD,
    })
    .to_string();
    let response = send("POST", "/api/admin/players", TOKEN, &body)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let created: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 8 * 1024).await.unwrap()).unwrap();

    let response = send(
        "GET",
        "/api/admin/attachments",
        "Bearer unauthorized-secret",
        "",
    )
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = send("GET", "/api/admin/attachments", TOKEN, "")
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let attachments: Vec<serde_json::Value> =
        serde_json::from_slice(&to_bytes(response.into_body(), 256 * 1024).await.unwrap()).unwrap();
    assert_eq!(attachments.len(), attachment_count);
    if has_attachments {
        let boat = attachments
            .iter()
            .find(|entry| entry["id"] == 2033)
            .unwrap();
        assert!(boat["name"].as_str().is_some_and(|name| !name.is_empty()));
        assert!(boat.get("max_owned").is_some());
        assert!(!attachments.iter().any(|entry| entry["id"] == 0));
    }
    let garage_path = format!(
        "/api/admin/players/{}/garage",
        created["id"].as_str().unwrap()
    );
    let response = send("GET", garage_path.as_str(), TOKEN, "").await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let garage: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 32 * 1024).await.unwrap()).unwrap();
    assert_eq!(garage["attachments"], serde_json::json!({}));

    let response = send("GET", "/api/admin/stat-trackers", TOKEN, "")
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let trackers: Vec<serde_json::Value> =
        serde_json::from_slice(&to_bytes(response.into_body(), 32 * 1024).await.unwrap()).unwrap();
    assert_eq!(trackers.len(), tracker_count);
    if tracker_count > 0 {
        assert!(
            trackers.iter().any(|item| item["id"] == 1
                && item["name"].as_str().is_some_and(|name| !name.is_empty()))
        );
    }
    let response = send(
        "POST",
        "/api/admin/players/17/grants",
        TOKEN,
        r#"{"kind":"stat_tracker","vehicle":43,"id":1,"count":1}"#,
    );
    let (response, ()) = tokio::join!(response, async {
        let request = tokio::time::timeout(std::time::Duration::from_secs(5), queued.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            request.grant,
            crate::accounts::PlayerGrant::StatTracker {
                vehicle: 43,
                id: 1,
                count: 1
            }
        ));
        request
            .reply
            .send(Err(GrantRequestError::NotFound))
            .unwrap();
    });
    assert_eq!(response.unwrap().status(), StatusCode::NOT_FOUND);

    let spans = exporter.get_finished_spans().unwrap();
    drop(subscriber_guard);
    let snapshot = accounts
        .authenticate(&username, PASSWORD.into())
        .await
        .unwrap();
    sqlx::query("DELETE FROM accounts WHERE username = $1")
        .bind(&username)
        .execute(&repository.connection_pool())
        .await
        .unwrap();
    let snapshot = snapshot.expect("dashboard player can log in");
    assert_eq!(
        created["id"].as_str(),
        Some(snapshot.id.to_string().as_str())
    );
    assert_eq!(snapshot.display_name, "Dashboard Player");
    assert_eq!(snapshot.vehicles.len(), starter_keys.len());
    assert!(!snapshot.settings.is_empty());
    let crew = snapshot.crew.as_ref().expect("starter crew persisted");
    for vehicle in &snapshot.vehicles {
        assert!(starter_keys.contains(&vehicle.catalog_key));
        assert!(vehicle.has_crew);
        assert!(crew.members.values().any(|member| {
            member
                .seat
                .is_some_and(|seat| seat.vehicle == vehicle.inventory_id)
        }));
        assert!(
            snapshot
                .progression
                .installed
                .contains_key(&vehicle.inventory_id)
        );
        assert!(
            vehicle.ammunition.as_ref().is_some_and(|ammunition| {
                ammunition.loaded.iter().any(|&(_, count)| count > 0)
            })
        );
    }

    let roots: Vec<_> = spans
        .iter()
        .filter(|span| span.name == "admin.http.request")
        .collect();
    assert_eq!(roots.len(), 16);
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
        &[("http.response.status_code", "404")],
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
        &[
            ("http.response.status_code", "404"),
            ("input.grant.kind", "attachment"),
            ("input.attachment_id", "2033"),
            ("input.amount", "1"),
        ],
        &[("http.response.status_code", "201")],
        &[
            ("http.response.status_code", "401"),
            ("http.route", "/api/admin/attachments"),
        ],
        &[
            ("http.response.status_code", "200"),
            ("http.route", "/api/admin/attachments"),
        ],
        &[
            ("http.response.status_code", "200"),
            ("http.route", "/api/admin/players/{id}/garage"),
        ],
        &[
            ("http.response.status_code", "200"),
            ("http.route", "/api/admin/stat-trackers"),
        ],
        &[
            ("http.response.status_code", "404"),
            ("input.grant.kind", "stat_tracker"),
            ("input.inventory_id", "43"),
            ("input.stat_tracker_id", "1"),
            ("input.amount", "1"),
        ],
    ]) {
        assert_eq!(root.parent_span_id, SpanId::INVALID);
        assert_eq!(
            attribute(root, "ddd.bounded_context.name").as_deref(),
            Some("player_administration")
        );
        for (key, value) in expected {
            assert_eq!(attribute(root, key).as_deref(), Some(*value), "{key}");
        }
    }
    for root in &roots {
        assert_eq!(
            attribute(root, "client.address").as_deref(),
            Some("198.51.100.7")
        );
        assert_eq!(
            attribute(root, "network.peer.address").as_deref(),
            Some("192.0.2.10")
        );
        assert_eq!(
            attribute(root, "network.peer.port").as_deref(),
            Some("2345")
        );
        assert_eq!(attribute(root, "client.port"), None);
    }
    assert_eq!(attribute(roots[1], "input.per_page"), None);
    for root in &roots[2..4] {
        assert_eq!(attribute(root, "input.page"), None);
    }
    assert_eq!(attribute(roots[5], "input.username.bytes"), None);
    assert_eq!(attribute(roots[6], "input.account_id"), None);
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
    assert!(contains_operation(roots[7], "admin.garage.grant"));
    assert!(contains_operation(roots[7], "player_sessions.grants.grant"));
    assert!(contains_operation(roots[9], "player_sessions.grants.grant"));
    assert!(contains_operation(
        roots[10],
        "accounts.players.persist_starter_garage"
    ));
    assert!(contains_operation(roots[10], "postgresql.query"));
    let auth_span = spans
        .iter()
        .find(|span| {
            span.name == "admin.authenticate"
                && span.parent_span_id == roots[12].span_context.span_id()
        })
        .unwrap();
    assert_eq!(
        attribute(auth_span, "output.outcome").as_deref(),
        Some("authorized")
    );
    let attachments_span = spans
        .iter()
        .find(|span| {
            span.name == "admin.garage.catalog"
                && span.parent_span_id == roots[12].span_context.span_id()
        })
        .unwrap();
    assert_eq!(
        attribute(attachments_span, "input.collection").as_deref(),
        Some("attachments")
    );
    assert_eq!(
        attribute(attachments_span, "output.items.count"),
        Some(attachment_count.to_string())
    );
    assert!(!contains_operation(roots[11], "admin.garage.catalog"));
    assert!(contains_operation(roots[14], "admin.garage.catalog"));
    assert!(spans.iter().any(|span| span.name == "admin.garage.catalog"
        && span.span_context.trace_id() == roots[14].span_context.trace_id()
        && attribute(span, "input.collection").as_deref() == Some("stat_trackers")
        && attribute(span, "output.items.count") == Some(tracker_count.to_string())));
    assert!(contains_operation(
        roots[15],
        "player_sessions.grants.grant"
    ));
    let exported = format!("{spans:?}");
    for excluded in [
        "test-bearer-secret",
        "unauthorized-secret",
        "test-password-secret",
        PASSWORD,
        "private-login@example.test",
        "Private Display",
        "query-secret",
        "malformed-body-secret",
    ] {
        assert!(!exported.contains(excluded), "exported {excluded}");
    }
}
