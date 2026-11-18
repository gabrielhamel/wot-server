use super::{Peer, ammunition_flow, dict, equipment_flow, player_fixture, telemetry_capture};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tower::ServiceExt;
use wot_server::{
    accounts::{Accounts, GarageCatalog, PostgresAccounts},
    platform::client_formats::pickle::Value as Pickle,
    player_sessions::{
        Gateway, GatewayConfig,
        mercury::app::{dispatch::ScriptDispatch, login::element::LoginResponse},
    },
};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires WG_TEST_DATABASE_URL and WG_RESOURCE_DIR"]
async fn store_purchase_updates_the_garage_and_rejects_replay() {
    telemetry_capture::exporter();
    let database_url = std::env::var("WG_TEST_DATABASE_URL").unwrap();
    let resource_dir = std::env::var("WG_RESOURCE_DIR").unwrap();
    let resources = wot_resources::ResourceBundle::open(Path::new(&resource_dir)).unwrap();
    let repository = PostgresAccounts::connect(&database_url, 4).await.unwrap();
    let accounts = Accounts::new(repository.clone());
    let username = format!(
        "store_{}",
        std::time::UNIX_EPOCH.elapsed().unwrap().as_nanos()
    );
    let starter = ["usa:A01_T1_Cunningham".to_owned()];
    let account_id =
        player_fixture::insert_player(&repository, &username, "Store test", "store", &starter)
            .await
            .unwrap();
    let catalog = Arc::new(GarageCatalog::load(resources.clone(), &starter).unwrap());
    let credits = catalog.store_offers()[0].credits;
    sqlx::query("UPDATE accounts SET credits = $2 WHERE id = $1")
        .bind(account_id)
        .bind(credits)
        .execute(&repository.connection_pool())
        .await
        .unwrap();
    let dispatch = ScriptDispatch::new(
        wot_server::player_sessions::mercury::script::load(&resources).unwrap(),
    );
    let mut gateway = Gateway::bind(
        GatewayConfig {
            login_bind: "127.0.0.1:0".parse().unwrap(),
            base_bind: "127.0.0.1:0".parse().unwrap(),
            advertised_base: None,
            private_key: None,
            max_clients: 2,
            max_pending: 4,
        },
        &resources,
        catalog,
        accounts.clone(),
        tokio::runtime::Handle::current(),
    )
    .unwrap();
    let (login, _) = gateway.addresses().unwrap();
    assert!(
        gateway
            .enable_store("https://*.example.test".to_owned())
            .is_err()
    );
    let router = gateway
        .enable_store("http://127.0.0.1:8081".to_owned())
        .unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let worker = tokio::task::spawn_blocking(move || gateway.run(worker_stop));
    let runtime = tokio::runtime::Handle::current();
    let exercise = tokio::task::spawn_blocking(move || {
        let request = |token: Option<&str>, order: Option<&Value>| {
            runtime.block_on(store_request(&router, token, order))
        };
        assert_eq!(request(None, None).0, StatusCode::UNAUTHORIZED);
        let connect = || {
            let mut peer = Peer::new(None);
            let LoginResponse::Success(ticket) = peer.authenticate(login, &username, "store")
            else {
                panic!("Expected a garage handoff");
            };
            peer.handoff(ticket);
            peer.until(&dispatch, |p| p.created && p.shown);
            let state = equipment_flow::full_cache(&mut peer, &dispatch);
            let token = peer
                .shop_url
                .as_ref()
                .unwrap()
                .strip_prefix("http://127.0.0.1:8081/store/#")
                .unwrap()
                .to_owned();
            (peer, token, state)
        };
        let (mut peer, token, _) = connect();
        let (status, catalog) = request(Some(&token), None);
        assert_eq!(status, StatusCode::OK);
        let offer = &catalog["offers"][0];
        assert_eq!(offer["in_depot"], 0);
        let item = offer["item"].as_i64().unwrap();
        let cost = offer["credits"].as_i64().unwrap();
        let credits = catalog["credits"].as_i64().unwrap();
        let revision: i64 = catalog["revision"].as_str().unwrap().parse().unwrap();
        let order = json!({"item": item, "credits": cost, "revision": catalog["revision"]});
        let mut wrong_price = order.clone();
        wrong_price["credits"] = json!(cost - 1);
        assert_eq!(
            request(Some(&token), Some(&wrong_price)).0,
            StatusCode::CONFLICT
        );
        let mut unknown = order.clone();
        unknown["item"] = json!(-42);
        assert_eq!(
            request(Some(&token), Some(&unknown)).0,
            StatusCode::NOT_FOUND
        );

        let (first, second) = runtime.block_on(async {
            tokio::join!(
                store_request(&router, Some(&token), Some(&order)),
                store_request(&router, Some(&token), Some(&order)),
            )
        });
        let (committed, rejected) = if first.0 == StatusCode::OK {
            (first, second)
        } else {
            (second, first)
        };
        assert_eq!(committed.0, StatusCode::OK);
        assert!(matches!(
            rejected.0,
            StatusCode::SERVICE_UNAVAILABLE | StatusCode::CONFLICT
        ));
        assert_eq!(committed.1["credits"], credits - cost);
        assert_eq!(committed.1["offers"][0]["in_depot"], 1);
        peer.until(&dispatch, |p| {
            p.methods.iter().any(|(name, _)| name.ends_with("update"))
        });
        let diff = ammunition_flow::update(&peer, revision + 1);
        assert_eq!(dict(&diff, "prevRev"), Pickle::I64(revision));
        assert_eq!(
            dict(&dict(&diff, "stats"), "credits"),
            Pickle::I64(credits - cost)
        );
        assert_depot_item(&diff, item);
        assert_eq!(request(Some(&token), Some(&order)).0, StatusCode::CONFLICT);
        let persisted = runtime.block_on(accounts.snapshot(account_id)).unwrap();
        assert_eq!(persisted.revision, revision + 1);
        assert_eq!(persisted.balances.credits, credits - cost);
        let new_order = json!({"item": item, "credits": cost, "revision": committed.1["revision"]});
        assert_eq!(
            request(Some(&token), Some(&new_order)).0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        peer.disconnect();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while request(Some(&token), None).0 != StatusCode::UNAUTHORIZED {
            assert!(
                std::time::Instant::now() < deadline,
                "Disconnected Store token still accepted"
            );
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        let (mut reconnected, fresh_token, state) = connect();
        assert_ne!(fresh_token, token);
        assert_depot_item(&state, item);
        assert_eq!(
            request(Some(&fresh_token), Some(&order)).0,
            StatusCode::CONFLICT
        );
        reconnected.disconnect();
        assert_store_traces(account_id, item, cost);
    })
    .await;
    stop.store(true, Ordering::Relaxed);
    worker.await.unwrap().unwrap();
    sqlx::query("DELETE FROM accounts WHERE id = $1")
        .bind(account_id)
        .execute(&repository.connection_pool())
        .await
        .unwrap();
    exercise.unwrap();
}

async fn store_request(
    router: &Router,
    token: Option<&str>,
    order: Option<&Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder().uri(if order.is_some() {
        "/store/orders"
    } else {
        "/store/catalog"
    });
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let body = if let Some(order) = order {
        request = request
            .method("POST")
            .header(header::CONTENT_TYPE, "application/json");
        Body::from(order.to_string())
    } else {
        Body::empty()
    };
    let response = router
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 16384).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

fn assert_depot_item(cache: &Pickle, item: i64) {
    let Pickle::Dict(inventory) = dict(cache, "inventory") else {
        panic!("Missing inventory")
    };
    let Pickle::Dict(equipment) = &inventory[&Pickle::I64(9)] else {
        panic!("Missing equipment stock")
    };
    assert_eq!(equipment[&Pickle::I64(item)], Pickle::I64(1));
}

fn assert_store_traces(account_id: i64, item: i64, credits: i64) {
    let spans = telemetry_capture::exporter().get_finished_spans().unwrap();
    let has = |span: &opentelemetry_sdk::trace::SpanData, key, expected: &str| {
        span.attributes
            .iter()
            .any(|attr| attr.key.as_str() == key && attr.value.to_string() == expected)
    };
    let root = |outcome| {
        spans
            .iter()
            .find(|span| {
                span.name == "player_sessions.store.purchase_request"
                    && has(span, "user.id", &account_id.to_string())
                    && has(span, "input.item", &item.to_string())
                    && has(span, "output.outcome", outcome)
            })
            .unwrap_or_else(|| panic!("Missing {outcome} Store request span"))
    };
    assert!(
        spans
            .iter()
            .any(|span| span.name == "player_sessions.enable"
                && has(span, "output.store.host", "127.0.0.1"))
    );
    let success = root("success");
    let stale = root("stale");
    for (operation, status) in [(success, "200"), (stale, "409")] {
        let root = spans
            .iter()
            .find(|span| span.span_context.span_id() == operation.parent_span_id)
            .expect("Missing Store HTTP request parent");
        assert_eq!(root.name, "store.http.request");
        assert!(has(root, "http.route", "/store/orders"));
        assert!(has(root, "http.response.status_code", status));
    }
    let child = |parent: &opentelemetry_sdk::trace::SpanData, name| {
        spans
            .iter()
            .find(|span| {
                span.name == name
                    && span.span_context.trace_id() == parent.span_context.trace_id()
                    && span.parent_span_id == parent.span_context.span_id()
            })
            .unwrap_or_else(|| panic!("Missing {name} under {}", parent.name))
    };
    let purchase = child(success, "player_sessions.store.purchase");
    let plan = child(purchase, "accounts.store.plan_purchase");
    assert!(has(plan, "input.price.credits", &credits.to_string()));
    let commit = child(purchase, "player_sessions.commit_progression");
    let save = child(
        child(commit, "accounts.save_progression"),
        "accounts.postgres.save_progression",
    );
    let sql = child(save, "postgresql.query");
    assert!(
        sql.attributes
            .iter()
            .any(|attr| attr.key.as_str() == "db.query.text")
    );
    let complete = child(purchase, "player_sessions.store.complete_purchase");
    let publish = child(complete, "player_sessions.publish_account_change");
    assert!(has(publish, "output.delivery", "queued"));
    assert!(has(
        child(publish, "mercury.response"),
        "output.delivery",
        "sent"
    ));
    let rejected = child(stale, "player_sessions.store.purchase");
    let decision = child(rejected, "accounts.store.plan_purchase");
    assert!(matches!(
        decision.status,
        opentelemetry::trace::Status::Error { .. }
    ));
    assert!(!decision.events.is_empty());
    assert!(spans.iter().all(
        |span| span.parent_span_id != rejected.span_context.span_id()
            || span.name != "player_sessions.commit_progression"
    ));
}
