use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

use super::*;
use crate::player_sessions::GrantRequestError;
use axum::{
    body::{Body, to_bytes},
    http::StatusCode,
};
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
    let admin: i64 = sqlx::query_scalar("INSERT INTO accounts(username, display_name, password_hash, role) VALUES ($1, 'Trace test admin', 'unused', 'admin') RETURNING id")
        .bind(format!("trace_admin_{}@example.test", std::time::UNIX_EPOCH.elapsed().unwrap().as_nanos()))
        .fetch_one(&repository.connection_pool()).await.unwrap();
    let secret = super::super::application::SessionSecret::parse(
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    )
    .unwrap();
    assert!(
        PlayerAccounts::new(players.clone())
            .open_web_session(admin, &secret.digest(), 60)
            .await
            .unwrap()
    );
    let cookie = format!("wot_admin_session={}", secret.value());
    let (grants, mut queued) = PlayerGrants::channel();
    let service = TowerToHyperService::new(router(
        players,
        accounts.clone(),
        catalog,
        grants,
        None,
        true,
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
    let send = |method, uri, cookie, body: &str| {
        let request = Request::builder()
            .extension("192.0.2.10:2345".parse::<SocketAddr>().unwrap())
            .header("x-forwarded-for", "198.51.100.7")
            .method(method)
            .uri(uri)
            .header(header::COOKIE, cookie)
            .header("x-wot-admin", "1")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CONTENT_LENGTH, body.len().to_string())
            .body(Body::from(body.to_owned()))
            .unwrap();
        service.call(request)
    };
    let session_cookie = cookie.as_str();
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
                session_cookie,
                "",
                200,
            ),
            ("GET", "/api/admin/players?page=3", session_cookie, "", 200),
            (
                "GET",
                "/api/admin/players?page=query-secret",
                session_cookie,
                "",
                400,
            ),
            (
                "GET",
                "/api/admin/players?page=9",
                "wot_admin_session=unauthorized-secret",
                "",
                401,
            ),
            ("POST", "/api/admin/players", session_cookie, PLAYER, 422),
            (
                "POST",
                "/api/admin/players",
                session_cookie,
                r#"{"password":"malformed-body-secret"}"#,
                422,
            ),
            (
                "GET",
                "/api/admin/players/0/garage",
                session_cookie,
                "",
                404,
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
            r#"{"kind":"attachment","id":2033,"count":1}"#,
        ] {
            let response = send("POST", "/api/admin/players/17/grants", session_cookie, body);
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
    let response = send("POST", "/api/admin/players", session_cookie, &body)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let created: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 8 * 1024).await.unwrap()).unwrap();

    let response = send(
        "GET",
        "/api/admin/attachments",
        "wot_admin_session=unauthorized-secret",
        "",
    )
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = send("GET", "/api/admin/attachments", session_cookie, "")
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
    let response = send("GET", garage_path.as_str(), session_cookie, "")
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let garage: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 32 * 1024).await.unwrap()).unwrap();
    assert_eq!(garage["attachments"], serde_json::json!({}));

    let response = send("GET", "/api/admin/stat-trackers", session_cookie, "")
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
        session_cookie,
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

    let duplicate_body = serde_json::json!({
        "username": username,
        "display_name": "Another Dashboard Name",
        "password": PASSWORD,
    })
    .to_string();
    let duplicate = send(
        "POST",
        "/api/admin/players",
        session_cookie,
        &duplicate_body,
    )
    .await
    .unwrap();
    assert_eq!(duplicate.status(), StatusCode::CONFLICT);

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
    sqlx::query("DELETE FROM accounts WHERE id = $1")
        .bind(admin)
        .execute(&repository.connection_pool())
        .await
        .unwrap();
    let snapshot = snapshot.expect("dashboard player can log in");
    assert_eq!(
        created["id"].as_str(),
        Some(snapshot.id.to_string().as_str())
    );
    assert_eq!(snapshot.display_name, "Dashboard Player");
    assert_eq!(snapshot.balances.credits, 200_000);
    assert_eq!(snapshot.balances.gold, 500);
    assert_eq!(snapshot.balances.bonds, 0);
    assert_eq!(snapshot.balances.free_xp, 0);
    let mut owned_keys: Vec<_> = snapshot
        .vehicles
        .iter()
        .map(|vehicle| vehicle.catalog_key.clone())
        .collect();
    owned_keys.sort_unstable();
    assert_eq!(owned_keys, starter_keys);
    assert!(!snapshot.settings.is_empty());
    let crew = snapshot.crew.as_ref().expect("starter crew persisted");
    for vehicle in &snapshot.vehicles {
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
    assert_eq!(roots.len(), 17);
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
        &[("http.response.status_code", "409")],
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
    for (root, outcome, transaction_end) in [
        (roots[10], "created", "COMMIT"),
        (roots[16], "duplicate_login", "ROLLBACK"),
    ] {
        assert!(contains_operation(root, "accounts.players.provision"));
        let provision = spans
            .iter()
            .find(|span| {
                span.name == "accounts.players.provision"
                    && span.span_context.trace_id() == root.span_context.trace_id()
            })
            .unwrap();
        assert_eq!(
            attribute(provision, "input.starter.credits").as_deref(),
            Some("200000")
        );
        assert_eq!(
            attribute(provision, "input.starter.gold").as_deref(),
            Some("500")
        );
        assert_eq!(
            attribute(provision, "output.outcome").as_deref(),
            Some(outcome)
        );
        let mut queries = spans.iter().filter(|span| {
            span.name == "postgresql.query"
                && span.parent_span_id == provision.span_context.span_id()
        });
        assert!(queries.clone().any(|span| {
            attribute(span, "db.query.text").is_some_and(|statement| {
                statement.starts_with("INSERT INTO accounts(")
                    && statement.contains("credits, gold) SELECT $1, $2, $3, $4, $5, $6")
            })
        }));
        assert!(
            queries
                .any(|span| attribute(span, "db.query.text").as_deref() == Some(transaction_end))
        );
    }
    assert!(contains_operation(
        roots[10],
        "accounts.players.persist_starter_garage"
    ));
    assert!(!contains_operation(
        roots[16],
        "accounts.players.persist_starter_garage"
    ));
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
        secret.value(),
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

#[tokio::test]
#[ignore = "Requires WG_TEST_DATABASE_URL and WG_RESOURCE_DIR"]
async fn exports_admin_http_access_lifecycle() {
    let url = std::env::var("WG_TEST_DATABASE_URL").unwrap();
    let directory = std::env::var_os("WG_RESOURCE_DIR").unwrap();
    let resources = wot_resources::ResourceBundle::open(Path::new(&directory)).unwrap();
    let catalog = Arc::new(GarageCatalog::load(resources.clone(), &[]).unwrap());
    let root_pool = sqlx::PgPool::connect(&url).await.unwrap();
    let schema = format!(
        "admin_access_{}",
        std::time::UNIX_EPOCH.elapsed().unwrap().as_nanos()
    );
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&root_pool)
        .await
        .unwrap();
    let separator = if url.contains('?') { '&' } else { '?' };
    let scoped_url = format!("{url}{separator}options[search_path]={schema}");
    let repository = PostgresAccounts::connect(&scoped_url, 4).await.unwrap();
    let mut gateway = crate::player_sessions::Gateway::bind(
        crate::player_sessions::GatewayConfig {
            training_room_visible: false,
            login_bind: "127.0.0.1:0".parse().unwrap(),
            base_bind: "127.0.0.1:0".parse().unwrap(),
            advertised_base: None,
            private_key: None,
            max_clients: 2,
            max_pending: 4,
        },
        &resources,
        catalog.clone(),
        Accounts::new(repository.clone()),
        tokio::runtime::Handle::current(),
    )
    .unwrap();
    let store = gateway
        .configure_store("http://127.0.0.1:8081".to_owned())
        .unwrap();
    let build_router = |repository: &PostgresAccounts| {
        let (grants, _) = PlayerGrants::channel();
        TowerToHyperService::new(router(
            PostgresPlayers::new(repository, catalog.clone()).unwrap(),
            Accounts::new(repository.clone()),
            catalog.clone(),
            grants,
            Some(store.clone()),
            true,
            vec![],
        ))
    };
    let service = build_router(&repository);
    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    let subscriber = tracing_subscriber::registry()
        .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("admin-access-test")));
    tracing::subscriber::set_global_default(subscriber).unwrap();

    struct StopGateway(Arc<AtomicBool>);

    impl Drop for StopGateway {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
        }
    }

    let stop = StopGateway(Arc::new(AtomicBool::new(false)));
    let worker_stop = stop.0.clone();
    let worker = tokio::task::spawn_blocking(move || gateway.run(worker_stop));
    let request = |method, path: &str, cookie: &str, body: &str| {
        Request::builder()
            .method(method)
            .uri(format!("/api/admin{path}"))
            .header(header::COOKIE, cookie)
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-wot-admin", "1")
            .header(
                "traceparent",
                "00-abcdef0123456789abcdef0123456789-1234567890abcdef-01",
            )
            .body(Body::from(body.to_owned()))
            .unwrap()
    };
    let send = |method, path: &str, cookie: &str, body: &str| {
        service.call(request(method, path, cookie, body))
    };
    let json = |response: Response| async {
        serde_json::from_slice::<serde_json::Value>(
            &to_bytes(response.into_body(), 1024 * 1024).await.unwrap(),
        )
        .unwrap()
    };
    const OWNER: &str = r#"{"username":"owner@example.test","display_name":"Owner","password":"first-admin-password"}"#;
    const OWNER_LOGIN: &str = r#"{"email":"owner@example.test","password":"first-admin-password"}"#;
    const SECOND: &str = r#"{"username":"recruit@example.test","display_name":"Recruit","password":"other-player-password"}"#;
    const SECOND_LOGIN: &str =
        r#"{"email":"recruit@example.test","password":"other-player-password"}"#;
    assert_eq!(
        json(send("GET", "/session", "", "").await.unwrap()).await["status"],
        "setup_required"
    );
    assert_eq!(send("GET", "/players", "", "").await.unwrap().status(), 401);
    assert_eq!(
        send("GET", "/me/garage", "", "").await.unwrap().status(),
        401
    );
    const REGISTRATION: &str = r#"{"email":"signup@example.test","username":"NewCadet","password":"Tank123!","confirm_password":"Tank123!"}"#;
    assert_eq!(
        send("POST", "/registration", "", REGISTRATION)
            .await
            .unwrap()
            .status(),
        409
    );
    assert_eq!(
        send(
            "POST",
            "/registration/username",
            "",
            r#"{"username":"NewCadet"}"#
        )
        .await
        .unwrap()
        .status(),
        409
    );

    let (first, second) = tokio::join!(
        send("POST", "/setup", "", OWNER),
        send("POST", "/setup", "", SECOND)
    );
    let (first, second) = (first.unwrap(), second.unwrap());
    let (created, conflict, first_login, second_account, second_login) = if first.status() == 201 {
        (first, second, OWNER_LOGIN, SECOND, SECOND_LOGIN)
    } else {
        (second, first, SECOND_LOGIN, OWNER, OWNER_LOGIN)
    };
    assert_eq!(created.status(), 201);
    assert_eq!(conflict.status(), 409);
    assert!(!created.headers().contains_key(header::SET_COOKIE));
    let owner = json(created).await;
    assert_eq!(owner["role"], "admin");
    let wrong_login =
        serde_json::json!({"email": owner["username"], "password": "wrong-password"}).to_string();
    assert_eq!(
        json(send("GET", "/session", "", "").await.unwrap()).await["status"],
        "signed_out"
    );
    assert_eq!(
        send("POST", "/session", "", &wrong_login)
            .await
            .unwrap()
            .status(),
        401
    );
    let session_cookie = |response: Response| {
        assert_eq!(response.status(), 204);
        let cookie = response.headers()[header::SET_COOKIE].to_str().unwrap();
        for attribute in [
            "Secure",
            "HttpOnly",
            "SameSite=Strict",
            "Path=/api/admin",
            "Max-Age=5184000",
        ] {
            assert!(
                cookie.split("; ").any(|part| part == attribute),
                "{attribute}"
            );
        }
        cookie.split(';').next().unwrap().to_owned()
    };
    let owner_cookie = session_cookie(send("POST", "/session", "", first_login).await.unwrap());
    let restarted = PostgresAccounts::connect(&scoped_url, 2).await.unwrap();
    let persisted = build_router(&restarted)
        .call(request("GET", "/session", &owner_cookie, ""))
        .await
        .unwrap();
    assert_eq!(json(persisted).await["player"], owner);
    let remaining: f64 = sqlx::query_scalar(
        "SELECT extract(epoch FROM expires_at - now())::float8 FROM web_sessions",
    )
    .fetch_one(&repository.connection_pool())
    .await
    .unwrap();
    assert!((5_183_900.0..=5_184_000.0).contains(&remaining));

    for (body, field) in [
        (
            REGISTRATION.replace("signup@example.test", "signup@example"),
            "email",
        ),
        (REGISTRATION.replace("NewCadet", "New Cadet"), "username"),
        (
            REGISTRATION.replacen("Tank123!", "Different123!", 1),
            "confirm_password",
        ),
    ]
    .into_iter()
    .chain(
        ["Tank1!", "TankTest!", "tank123!", "TANK123!", "Tank1234"]
            .map(|password| (REGISTRATION.replace("Tank123!", password), "password")),
    ) {
        let response = send("POST", "/registration", "", &body).await.unwrap();
        assert_eq!(response.status(), 422);
        assert!(json(response).await["errors"][field].is_string());
    }
    let availability = r#"{"username":"newcadet"}"#;
    assert_eq!(
        json(
            send("POST", "/registration/username", "", availability)
                .await
                .unwrap()
        )
        .await["available"],
        true
    );
    let rival = REGISTRATION
        .replace("signup@example.test", "rival@example.test")
        .replace("NewCadet", "newcadet");
    let (registered, rival) = tokio::join!(
        send("POST", "/registration", "", REGISTRATION),
        send("POST", "/registration", "", &rival)
    );
    let (first, second) = (registered.unwrap(), rival.unwrap());
    let (registered, conflict) = if first.status() == 201 {
        (first, second)
    } else {
        (second, first)
    };
    assert_eq!(registered.status(), 201);
    assert!(!registered.headers().contains_key(header::SET_COOKIE));
    assert_eq!(conflict.status(), 409);
    assert!(json(conflict).await["errors"]["username"].is_string());
    let registered = json(registered).await;
    assert_eq!(registered["role"], "player");
    let claimed_email = registered["username"].as_str().unwrap();
    let duplicate_email = REGISTRATION
        .replace("signup@example.test", &claimed_email.to_ascii_uppercase())
        .replace("NewCadet", "AnotherCadet");
    let conflict = send("POST", "/registration", "", &duplicate_email)
        .await
        .unwrap();
    assert_eq!(conflict.status(), 409);
    assert!(json(conflict).await["errors"]["email"].is_string());
    assert_eq!(
        json(
            send("POST", "/registration/username", "", availability)
                .await
                .unwrap()
        )
        .await["available"],
        false
    );
    let player_login =
        serde_json::json!({"email": claimed_email, "password": "Tank123!"}).to_string();
    let player_cookie = session_cookie(send("POST", "/session", "", &player_login).await.unwrap());
    assert_eq!(
        json(send("GET", "/session", &player_cookie, "").await.unwrap()).await["player"],
        registered
    );
    let own_garage = json(send("GET", "/me/garage", &player_cookie, "").await.unwrap()).await;
    assert_eq!(own_garage["display_name"], registered["display_name"]);
    assert_eq!(own_garage["bonds"], "0");
    assert_eq!(own_garage["free_xp"], "0");
    assert!(!own_garage["vehicles"].as_array().unwrap().is_empty());
    let player_id: i64 = registered["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(own_garage["gold"], "500");
    const CATALOG: &str = "/me/shop/catalog";
    const ORDERS: &str = "/me/shop/orders";
    assert_eq!(send("GET", CATALOG, "", "").await.unwrap().status(), 401);
    assert_eq!(send("POST", ORDERS, "", "{}").await.unwrap().status(), 401);
    let session = json(send("GET", "/session", &player_cookie, "").await.unwrap()).await;
    assert_eq!(session["store_enabled"], true);
    let listing = send("GET", CATALOG, &player_cookie, "").await.unwrap();
    assert_eq!(listing.status(), 200);
    assert_eq!(listing.headers()[header::CACHE_CONTROL], "no-store");
    let listing = json(listing).await;
    assert!(listing["premium_expires_at"].is_null());
    let mut payload = serde_json::json!({
        "item": -1,
        "revision": listing["revision"],
        "price": {"currency": "gold", "amount": 250}
    });
    let order = payload.to_string();
    let mut unprotected = request("POST", ORDERS, &player_cookie, &order);
    unprotected.headers_mut().remove("x-wot-admin");
    assert_eq!(service.call(unprotected).await.unwrap().status(), 403);
    payload["account_id"] = owner["id"].clone();
    assert_eq!(
        send("POST", ORDERS, &player_cookie, &payload.to_string())
            .await
            .unwrap()
            .status(),
        400
    );
    let purchased = send("POST", ORDERS, &player_cookie, &order).await.unwrap();
    assert_eq!(purchased.status(), 200);
    let purchased = json(purchased).await;
    assert!(purchased["premium_expires_at"].is_u64());
    assert_ne!(purchased["revision"], listing["revision"]);
    assert_eq!(
        send("POST", ORDERS, &player_cookie, &order)
            .await
            .unwrap()
            .status(),
        409
    );
    let updated_garage = json(send("GET", "/me/garage", &player_cookie, "").await.unwrap()).await;
    assert_eq!(updated_garage["gold"], "250");

    let game_account = Accounts::new(repository.clone())
        .authenticate(claimed_email, "Tank123!".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        game_account.display_name,
        registered["display_name"].as_str().unwrap()
    );
    let other_garage = format!("/players/{}/garage", owner["id"].as_str().unwrap());
    let other_grants = format!("/players/{}/grants", owner["id"].as_str().unwrap());
    assert_eq!(
        send("GET", "/players", &player_cookie, "")
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        send("GET", &other_garage, &player_cookie, "")
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        send(
            "POST",
            &other_grants,
            &player_cookie,
            r#"{"kind":"currency","currency":"gold","amount":1}"#
        )
        .await
        .unwrap()
        .status(),
        401,
    );
    let player_role = format!("/players/{}/role", registered["id"].as_str().unwrap());
    assert_eq!(
        send("POST", &player_role, &owner_cookie, r#"{"role":"admin"}"#)
            .await
            .unwrap()
            .status(),
        204
    );
    assert_eq!(
        send("GET", "/players", &player_cookie, "")
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        send("GET", "/me/garage", &player_cookie, "")
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        send("POST", &player_role, &owner_cookie, r#"{"role":"player"}"#)
            .await
            .unwrap()
            .status(),
        204
    );

    let owner_role = format!("/players/{}/role", owner["id"].as_str().unwrap());
    assert_eq!(
        send("POST", &owner_role, &owner_cookie, r#"{"role":"player"}"#)
            .await
            .unwrap()
            .status(),
        403
    );

    let created = send("POST", "/players", &owner_cookie, second_account)
        .await
        .unwrap();
    assert_eq!(created.status(), 201);
    let second = json(created).await;
    assert_eq!(second["role"], "player");
    assert_eq!(
        send("POST", "/session", "", second_login)
            .await
            .unwrap()
            .status(),
        204
    );
    let second_role = format!("/players/{}/role", second["id"].as_str().unwrap());
    let mut without_header = request("POST", &second_role, &owner_cookie, r#"{"role":"admin"}"#);
    without_header.headers_mut().remove("x-wot-admin");
    assert_eq!(service.call(without_header).await.unwrap().status(), 403);
    assert_eq!(
        send("POST", &second_role, &owner_cookie, r#"{"role":"admin"}"#)
            .await
            .unwrap()
            .status(),
        204
    );
    let second_cookie = session_cookie(send("POST", "/session", "", second_login).await.unwrap());

    let (first, second) = tokio::join!(
        send("POST", &second_role, &owner_cookie, r#"{"role":"player"}"#),
        send("POST", &owner_role, &second_cookie, r#"{"role":"player"}"#),
    );
    let (first, second) = (first.unwrap().status(), second.unwrap().status());
    let (active_cookie, revoked_cookie, revoked_role, revoked_login) =
        match (first.as_u16(), second.as_u16()) {
            (204, 401) => (&owner_cookie, &second_cookie, &second_role, second_login),
            (401, 204) => (&second_cookie, &owner_cookie, &owner_role, first_login),
            result => panic!("unexpected concurrent demotion results: {result:?}"),
        };
    assert_eq!(
        send("GET", "/players", revoked_cookie, "")
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        send("POST", "/session", "", revoked_login)
            .await
            .unwrap()
            .status(),
        204
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM accounts WHERE role = 'admin'")
        .fetch_one(&repository.connection_pool())
        .await
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(
        send("POST", revoked_role, active_cookie, r#"{"role":"admin"}"#)
            .await
            .unwrap()
            .status(),
        204
    );
    assert_eq!(
        send("GET", "/players", revoked_cookie, "")
            .await
            .unwrap()
            .status(),
        401
    );

    let logout = send("POST", "/logout", active_cookie, "{}").await.unwrap();
    assert_eq!(logout.status(), 204);
    assert!(
        logout.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .contains("Max-Age=0")
    );
    assert_eq!(
        send("GET", "/players", active_cookie, "")
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        json(send("GET", "/session", active_cookie, "").await.unwrap()).await["status"],
        "signed_out"
    );
    let expired_cookie = session_cookie(send("POST", "/session", "", first_login).await.unwrap());
    sqlx::query("UPDATE web_sessions SET expires_at = now() - interval '1 second'")
        .execute(&repository.connection_pool())
        .await
        .unwrap();
    assert_eq!(
        send("GET", "/players", &expired_cookie, "")
            .await
            .unwrap()
            .status(),
        401
    );

    assert_eq!(
        send("GET", CATALOG, &expired_cookie, "")
            .await
            .unwrap()
            .status(),
        401
    );

    let mut wrong_case: serde_json::Value = serde_json::from_str(first_login).unwrap();
    wrong_case["email"] = owner["username"]
        .as_str()
        .unwrap()
        .to_ascii_uppercase()
        .into();
    let wrong_case = wrong_case.to_string();
    assert_eq!(
        send("POST", "/session", "", &wrong_case)
            .await
            .unwrap()
            .status(),
        401
    );
    sqlx::query("UPDATE accounts SET role = 'player'")
        .execute(&repository.connection_pool())
        .await
        .unwrap();
    assert_eq!(
        json(send("GET", "/session", "", "").await.unwrap()).await["status"],
        "signed_out"
    );
    assert_eq!(
        send("POST", "/setup", "", OWNER).await.unwrap().status(),
        409
    );

    drop(stop);
    worker.await.unwrap().unwrap();
    let spans = exporter.get_finished_spans().unwrap();
    let child = |parent: &SpanData, name| {
        spans
            .iter()
            .find(|span| span.parent_span_id == parent.span_context.span_id() && span.name == name)
            .unwrap_or_else(|| panic!("Missing {name} under {}", parent.name))
    };
    for (handler, outcome, status) in [
        ("player_sessions.store.catalog_request", "success", "200"),
        ("player_sessions.store.purchase_request", "success", "200"),
        ("player_sessions.store.purchase_request", "stale", "409"),
    ] {
        let operation = spans
            .iter()
            .find(|span| {
                span.name == handler
                    && attribute(span, "input.channel").as_deref() == Some("dashboard")
                    && attribute(span, "output.outcome").as_deref() == Some(outcome)
            })
            .unwrap();
        assert_eq!(attribute(operation, "user.id"), Some(player_id.to_string()));
        let root = spans
            .iter()
            .find(|span| span.span_context.span_id() == operation.parent_span_id)
            .unwrap();
        assert_eq!(root.name, "admin.http.request");
        assert_eq!(
            root.parent_span_id,
            SpanId::from_hex("1234567890abcdef").unwrap()
        );
        assert_eq!(
            root.span_context.trace_id(),
            operation.span_context.trace_id()
        );
        assert_eq!(
            attribute(root, "http.response.status_code").as_deref(),
            Some(status)
        );
        let auth = child(root, "admin.authenticate");
        assert_eq!(
            attribute(auth, "output.outcome").as_deref(),
            Some("authorized")
        );
        if handler.ends_with("catalog_request") {
            let load = child(operation, "player_sessions.store.load_catalog");
            let snapshot = child(child(load, "player_sessions.snapshot"), "accounts.snapshot");
            let repository = child(snapshot, "accounts.postgres.snapshot");
            let read = child(repository, "accounts.postgres.read_snapshot");
            assert!(attribute(child(read, "postgresql.query"), "db.query.text").is_some());
        } else {
            let purchase = child(operation, "player_sessions.store.purchase");
            child(purchase, "accounts.store.plan_purchase");
            let completion = child(purchase, "player_sessions.store.complete_purchase");
            if outcome == "success" {
                let commit = child(purchase, "player_sessions.commit_progression");
                let save = child(
                    child(commit, "accounts.save_progression"),
                    "accounts.postgres.save_progression",
                );
                assert!(attribute(child(save, "postgresql.query"), "db.query.text").is_some());
                let published = child(completion, "player_sessions.publish_account_change");
                assert_eq!(
                    attribute(published, "output.delivery").as_deref(),
                    Some("next_sync")
                );
            } else {
                assert_eq!(
                    attribute(completion, "output.success").as_deref(),
                    Some("false")
                );
            }
        }
    }

    for (name, key, outcome, status) in [
        ("admin.sign_in", "output.outcome", "authorized", "204"),
        ("admin.sign_in", "output.outcome", "rejected", "401"),
        (
            "admin.players.change_role",
            "output.outcome",
            "updated",
            "204",
        ),
        (
            "admin.players.change_role",
            "output.outcome",
            "own_role",
            "403",
        ),
        (
            "web_panel.registration.create",
            "output.outcome",
            "created",
            "201",
        ),
        (
            "web_panel.registration.create",
            "output.outcome",
            "duplicate_username",
            "409",
        ),
        (
            "web_panel.registration.create",
            "output.outcome",
            "duplicate_login",
            "409",
        ),
        (
            "web_panel.registration.create",
            "output.outcome",
            "invalid_email",
            "422",
        ),
        (
            "web_panel.registration.create",
            "output.outcome",
            "invalid_password",
            "422",
        ),
        (
            "web_panel.registration.check_username",
            "output.username.available",
            "false",
            "200",
        ),
        (
            "web_panel.garage.show_own",
            "user.id",
            registered["id"].as_str().unwrap(),
            "200",
        ),
    ] {
        let span = spans
            .iter()
            .find(|span| span.name == name && attribute(span, key).as_deref() == Some(outcome))
            .unwrap();
        let root = spans
            .iter()
            .find(|root| {
                root.name == "admin.http.request"
                    && root.span_context.span_id() == span.parent_span_id
            })
            .unwrap();
        assert_eq!(
            root.parent_span_id,
            SpanId::from_hex("1234567890abcdef").unwrap()
        );
        assert_eq!(
            attribute(root, "http.response.status_code").as_deref(),
            Some(status)
        );
        assert!(
            spans
                .iter()
                .filter(|child| child.name == "postgresql.query")
                .any(|child| {
                    let mut parent = child.parent_span_id;
                    while let Some(ancestor) = spans
                        .iter()
                        .find(|candidate| candidate.span_context.span_id() == parent)
                    {
                        if parent == span.span_context.span_id() {
                            return true;
                        }
                        parent = ancestor.parent_span_id;
                    }
                    false
                }),
            "{name}: no SQL descendant"
        );
    }
    let exported = format!("{spans:?}");
    for excluded in [
        "owner@example.test",
        "recruit@example.test",
        "first-admin-password",
        "other-player-password",
        "wrong-password",
        "signup@example.test",
        "rival@example.test",
        "Tank123!",
        "TankTest!",
        "Different123!",
        "NewCadet",
        "newcadet",
        player_cookie.split_once('=').unwrap().1,
        owner_cookie.split_once('=').unwrap().1,
        second_cookie.split_once('=').unwrap().1,
    ] {
        assert!(!exported.contains(excluded));
    }
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&root_pool)
        .await
        .unwrap();
}
