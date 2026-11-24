use super::{Peer, ScriptDispatch, Value, equipment_flow, player_fixture, telemetry_capture};
use axum::{
    body::{Body, to_bytes},
    extract::Request,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hyper::service::Service;
use hyper_util::service::TowerToHyperService;
use opentelemetry::trace::SpanId;
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use wot_accounts::test_support::{
    self as accounts, Accounts, GarageCatalog, PostgresAccounts, PostgresPlayers,
};
use wot_player_administration::contract::{DashboardConfig, router};
use wot_player_sessions::{Gateway, GatewayConfig};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires WG_TEST_DATABASE_URL and WG_RESOURCE_DIR with Training terrain"]
async fn dashboard_launches_player_and_stationary_dummies() {
    let exporter = telemetry_capture::exporter();
    let resources =
        wot_resources::ResourceBundle::open(Path::new(&std::env::var("WG_RESOURCE_DIR").unwrap()))
            .unwrap();
    let repository = PostgresAccounts::new(
        wot_server_runtime::test_support::connect_database(
            &std::env::var("WG_TEST_DATABASE_URL").unwrap(),
            8,
        )
        .await
        .unwrap(),
    );
    let prefix = format!(
        "admin_battle_{}",
        std::time::UNIX_EPOCH.elapsed().unwrap().as_nanos()
    );
    let login_name = format!("{prefix}@example.test");
    let tank = "ussr:R04_T-34".to_owned();
    let account = player_fixture::insert_player(
        &repository,
        &login_name,
        "Battle Commander",
        "password",
        &[tank.clone(), "france:F01_RenaultFT".into()],
    )
    .await
    .unwrap();
    sqlx::query("UPDATE accounts.accounts SET role = 'admin' WHERE id = $1")
        .bind(account)
        .execute(&repository.connection_pool())
        .await
        .unwrap();

    sqlx::query("UPDATE accounts.owned_vehicles SET has_crew = false WHERE account_id = $1 AND inventory_id = 2")
        .bind(account)
        .execute(&repository.connection_pool())
        .await
        .unwrap();
    let catalog =
        Arc::new(GarageCatalog::load(resources.clone(), std::slice::from_ref(&tank)).unwrap());
    let access =
        accounts::player_access(PostgresPlayers::new(&repository, catalog.clone()).unwrap());
    let secret = URL_SAFE_NO_PAD.encode(Sha256::digest(&prefix));
    assert!(
        access
            .open_web_session(account, Sha256::digest(&secret).as_ref(), 60)
            .await
            .unwrap()
    );
    let mut gateway = Gateway::bind(
        wot_battles::mercury::bind,
        GatewayConfig {
            training_room_visible: true,
            periphery_id: 250,
            login_bind: "127.0.0.1:0".parse().unwrap(),
            base_bind: "127.0.0.1:0".parse().unwrap(),
            advertised_base: None,
            private_key: None,
            max_clients: 4,
            max_pending: 4,
        },
        &resources,
        player_fixture::gateway_accounts(&repository, &catalog),
        wot_social::postgres::service(
            repository.connection_pool(),
            accounts::directory(repository.connection_pool()),
        ),
        tokio::runtime::Handle::current(),
    )
    .unwrap();
    let metrics = telemetry_capture::GatewayMetrics::attach(&mut gateway);
    let (login, _) = gateway.addresses().unwrap();
    let api = TowerToHyperService::new(router(
        access,
        accounts::game_accounts(Accounts::new(repository.clone())),
        accounts::garage_queries(catalog),
        gateway.player_grants(),
        None,
        DashboardConfig {
            secure_cookies: false,
            trusted_proxies: vec![],
            battles: Some(gateway.admin_battles()),
        },
    ));
    let running = Arc::new(AtomicBool::new(false));
    let stop = running.clone();
    let gateway = tokio::task::spawn_blocking(move || gateway.run(stop));
    struct Stop(Arc<AtomicBool>);
    impl Drop for Stop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
        }
    }
    let stop = Stop(running);
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let script = ScriptDispatch::new(wot_platform::mercury::script::load(&resources).unwrap());
        let mut peer = Peer::new(None);
        peer.connect(login, &login_name, "password");
        peer.until(&script, |p| p.created && p.shown);
        equipment_flow::full_cache(&mut peer, &script);

        let request = |method: &str, path: &str, cookie: &str, body: Json| {
            let req = Request::builder().method(method)
                .uri(format!("/api/admin{path}"))
                .header("cookie", cookie)
                .header("content-type", "application/json")
                .header("x-wot-admin", "1")
                .header("traceparent", "00-29c2a78c107b11efaef448210f071ee0-2a70f170c9056666-01")
                .body(Body::from(body.to_string())).unwrap();
            runtime.block_on(async {
                let response = api.call(req).await.unwrap();
                let status = response.status().as_u16();
                let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
                (status, serde_json::from_slice::<Json>(&body).unwrap())
            })
        };
        let cookie = format!("wot_admin_session={secret}");
        assert_eq!(request("GET", "/battles", "", Json::Null).0, 401);
        let (status, options) = request("GET", "/battles", &cookie, Json::Null);
        assert_eq!(status, 200, "{options}");
        assert_eq!(options["players"][0]["id"], account);
        let vehicles = options["players"][0]["vehicles"].as_array().unwrap();
        let unavailable = |id| vehicles.iter().find(|vehicle| vehicle["id"] == id).unwrap()["unavailable_reason"].clone();
        assert_eq!(unavailable(1), Json::Null);
        assert_eq!(unavailable(2), "Crew incomplete");
        let arena = (1 << 16) | 5;
        let maps = options["maps"].as_array().unwrap();
        assert!(maps.iter().any(|map| map["id"] == arena && map["name"].as_str().unwrap().contains("Encounter")));
        let dummy = options["dummy_vehicles"].as_array().unwrap().iter()
            .find(|entry| entry["key"] == tank).unwrap()["id"].as_i64().unwrap();
        let (status, loadout) = request("GET", &format!("/battles/vehicles/{dummy}"), &cookie, Json::Null);
        assert_eq!(status, 200, "{loadout}");
        let hardening = loadout["equipment"].as_array().unwrap().iter()
            .find(|entry| entry["name"].as_str().unwrap().contains("Improved Hardening")).unwrap()["id"].clone();
        let consumable = loadout["consumables"].as_array().unwrap()[0]["id"].clone();
        let mut battle = json!({
            "arena_type": arena, "duration_seconds": 300,
            "players": [{"account_id": account, "team": 1, "inventory_id": 1}],
            "dummies": [
                {"team": 2, "loadout": {"vehicle": dummy, "equipment": [hardening.clone(), hardening]}},
                {"team": 2, "loadout": {"vehicle": dummy}},
            ],
        });
        assert_eq!(request("POST", "/battles", &cookie, battle.clone()).0, 400);
        assert_eq!(peer.resets, 0);
        battle["dummies"][0]["loadout"]["equipment"] = json!([hardening]);
        battle["dummies"][0]["loadout"]["consumables"] = json!([consumable]);
        battle["dummies"][0]["loadout"]["crew_perks"] = json!([[9]]);
        let mut unready = battle.clone();
        unready["players"][0]["inventory_id"] = json!(2);
        let (status, rejected) = request("POST", "/battles", &cookie, unready);
        assert_eq!(status, 409);
        assert!(rejected["message"].as_str().unwrap().contains("Crew incomplete"));

        let (status, launched) = request("POST", "/battles", &cookie, battle.clone());
        assert_eq!(status, 200, "{launched}");
        assert_eq!(launched["players_handed_off"], 1);
        let battle_id = launched["battle_id"].as_u64().unwrap();
        assert_eq!(request("POST", "/battles", &cookie, battle).0, 409);
        let (status, busy) = request("GET", "/battles", &cookie, Json::Null);
        assert_eq!(status, 200);
        assert_eq!(busy["players"][0]["unavailable_reason"], "In battle");

        peer.until(&script, |p| p.resets == 1);
        assert!(peer.methods.iter().any(|(name, _)| name == "onArenaCreated"));
        peer.enable_entities();
        peer.until(&script, |p| p.arena_roster.is_some());
        assert_eq!(peer.entered_arena, Some((battle_id, arena)));
        let (vehicle_type, schema) = script.entity_from_name("Vehicle").unwrap();
        use wot_platform::mercury::net::codec::Codec;
        let mut health = peer.arena_entities.iter()
            .filter(|entity| entity.type_id == vehicle_type)
            .map(|entity| {
                let mut properties = entity.properties.as_slice();
                let count = u8::read(&mut properties, &()).unwrap();
                (0..count).find_map(|_| {
                    let id = u8::read(&mut properties, &()).unwrap();
                    let property = &schema.properties[usize::from(id)];
                    let value = Value::read(&mut properties, &property.ty).unwrap();
                    (&*property.name == "health").then(|| value.as_i64().unwrap())
                }).unwrap()
            }).collect::<Vec<_>>();
        health.sort_unstable();
        assert_eq!(health.len(), 3);
        assert_eq!(health[0], health[1]);
        assert!(health[2] > health[1], "Hardening must increase dummy HP: {health:?}");
        let own = peer.cell.as_ref().unwrap().vehicle_id;
        let dummies = peer.arena_entities.iter()
            .filter(|entity| entity.type_id == vehicle_type && entity.entity_id != own)
            .map(|entity| (entity.entity_id, entity.position)).collect::<Vec<_>>();
        assert_eq!(dummies.len(), 2);
        peer.command(&script, "setClientReady", vec![]);
        peer.until(&script, |p| p.methods.iter().any(|(name, args)| {
            if name != "updateArena" || args[0].as_i64() != Some(3) { return false; }
            let Value::String(wot_platform::mercury::script::StringValue::Raw(data)) = &args[1] else { return false; };
            matches!(wot_platform::client_formats::pickle::from_reader(flate2::read::ZlibDecoder::new(data.as_slice())),
                Ok(super::Pickle::Tuple(fields)) if fields.first() == Some(&super::Pickle::I64(3)))
        }) && p.arena_tick.is_some());
        let tick = peer.arena_tick.unwrap();
        peer.until(&script, |p| p.arena_tick.is_some_and(|later| (3..=127).contains(&later.wrapping_sub(tick))));
        for (id, position) in dummies {
            assert!(peer.positions[&id].distance(position) < 0.01);
        }
        peer.command(&script, "leaveArena", vec![]);
        peer.until(&script, |p| p.resets == 2);
        peer.enable_entities();
        peer.until(&script, |p| p.created && p.shown);
        peer.clear_resource(617);
        equipment_flow::full_cache(&mut peer, &script);
        let (status, back_in_garage) = request("GET", "/battles", &cookie, Json::Null);
        assert_eq!(status, 200);
        assert!(back_in_garage["players"][0]["unavailable_reason"].is_null());
        peer.command(&script, "doCmdIntArr", vec![
            Value::Int16(683),
            Value::Int16(105),
            Value::Seq([1, -1, -1, 0, 1].into_iter().map(Value::Int32).collect()),
        ]);
        peer.until(&script, |p| p.responses.contains(&(683, 0)));
        let (status, after_crew_change) = request("GET", "/battles", &cookie, Json::Null);
        assert_eq!(status, 200);
        let vehicles = after_crew_change["players"][0]["vehicles"].as_array().unwrap();
        assert_eq!(vehicles.iter().find(|vehicle| vehicle["id"] == 1).unwrap()["unavailable_reason"], "Crew incomplete");
        metrics.assert_snapshot(1, 0, true);
    }).await.unwrap();
    drop(stop);
    gateway.await.unwrap().unwrap();

    let spans = exporter.get_finished_spans().unwrap();
    let get = |span: &opentelemetry_sdk::trace::SpanData, field: &str| {
        span.attributes
            .iter()
            .find(|attr| attr.key.as_str() == field)
            .map(|attr| attr.value.to_string())
    };
    let launch = spans
        .iter()
        .find(|s| {
            s.name == "player_sessions.start_admin_battle"
                && get(s, "output.outcome").as_deref() == Some("launched")
        })
        .unwrap();
    let handler = spans
        .iter()
        .find(|s| s.span_context.span_id() == launch.parent_span_id)
        .unwrap();
    assert_eq!(handler.name, "admin.start_battle");
    let http = spans
        .iter()
        .find(|s| s.span_context.span_id() == handler.parent_span_id)
        .unwrap();
    assert_eq!(http.name, "admin.http.request");
    assert_eq!(
        http.parent_span_id,
        SpanId::from_hex("2a70f170c9056666").unwrap()
    );
    assert_eq!(get(launch, "input.dummies.count").as_deref(), Some("2"));
    for name in [
        "accounts.dummy_vehicle",
        "battles.prepare",
        "battles.activate",
    ] {
        assert!(
            spans
                .iter()
                .any(|s| s.name == name && s.parent_span_id == launch.span_context.span_id()),
            "{name}"
        );
    }
    let preparation = spans
        .iter()
        .find(|s| s.name == "battles.prepare" && s.parent_span_id == launch.span_context.span_id())
        .unwrap();
    assert!(spans.iter().any(
        |s| s.name == "battles.enter" && s.parent_span_id == preparation.span_context.span_id()
    ));
    let crew_rejection = spans
        .iter()
        .find(|s| {
            s.name == "accounts.admin_battle_vehicle"
                && get(s, "output.outcome").as_deref() == Some("crew_incomplete")
        })
        .unwrap();
    assert_eq!(
        get(crew_rejection, "input.inventory_id").as_deref(),
        Some("2")
    );
    assert!(
        spans
            .iter()
            .any(|s| s.name == "player_sessions.start_admin_battle"
                && s.span_context.span_id() == crew_rejection.parent_span_id
                && get(s, "output.outcome").as_deref() == Some("crew_incomplete"))
    );
    for expected in ["invalid_dummy", "player_unavailable", "crew_incomplete"] {
        assert!(
            spans.iter().any(|s| s.name == "admin.start_battle"
                && get(s, "output.outcome").as_deref() == Some(expected)),
            "{expected}"
        );
    }
    sqlx::query("DELETE FROM accounts.accounts WHERE id = $1")
        .bind(account)
        .execute(&repository.connection_pool())
        .await
        .unwrap();
}
