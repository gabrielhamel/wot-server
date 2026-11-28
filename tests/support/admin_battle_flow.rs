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
    collections::BTreeMap,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use wot_accounts::test_support::{
    self as accounts, Accounts, CosmeticState, GarageCatalog, OutfitSeason, PostgresAccounts,
    PostgresPlayers, VehicleAmmunition, VehicleOutfit,
};
use wot_player_administration::contract::{DashboardConfig, router};
use wot_player_sessions::{Gateway, GatewayConfig};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires WOT_TEST_DATABASE_URL and WOT_RESOURCE_DIR with Training terrain"]
async fn dashboard_launches_player_and_stationary_dummies() {
    let exporter = telemetry_capture::exporter();
    let resources =
        wot_resources::ResourceBundle::open(Path::new(&std::env::var("WOT_RESOURCE_DIR").unwrap()))
            .unwrap();
    let repository = PostgresAccounts::new(
        wot_server_runtime::test_support::connect_database(
            &std::env::var("WOT_TEST_DATABASE_URL").unwrap(),
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
    let styled_tank = "ussr:R19_IS-3";
    let account = player_fixture::insert_player(
        &repository,
        &login_name,
        "Battle Commander",
        "password",
        &[
            tank.clone(),
            "france:F01_RenaultFT".into(),
            styled_tank.into(),
        ],
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
    let garage = accounts::garage_queries(catalog.clone());
    let styled_type = catalog.vehicle_catalog_entry(styled_tank).unwrap().id;
    let styled_catalog = serde_json::to_value(
        garage
            .custom_battle_loadout_options(styled_type)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let styled_shell = styled_catalog["ammunition"][0]["id"].as_i64().unwrap() as i32;
    let styled_ammo = VehicleAmmunition {
        loaded: vec![(styled_shell, 7)],
    };
    let mut cosmetics = CosmeticState::default();
    cosmetics.outfits.insert(
        3,
        BTreeMap::from([(
            OutfitSeason::Common,
            VehicleOutfit {
                style: Some(111),
                ..Default::default()
            },
        )]),
    );
    sqlx::query("UPDATE accounts.accounts SET progression = jsonb_set(progression, '{cosmetics}', $2) WHERE id = $1")
        .bind(account)
        .bind(sqlx::types::Json(&cosmetics))
        .execute(&repository.connection_pool()).await.unwrap();
    sqlx::query("UPDATE accounts.owned_vehicles SET ammunition = $2 WHERE account_id = $1 AND inventory_id = 3")
        .bind(account)
        .bind(sqlx::types::Json(&styled_ammo))
        .execute(&repository.connection_pool()).await.unwrap();

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
        garage.clone(),
        gateway.player_grants(),
        None,
        DashboardConfig {
            secure_cookies: false,
            trusted_proxies: vec![],
            battles: Some(gateway.training_battle_launches()),
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
        let parameters = options["parameters"].as_array().unwrap();
        let duration = parameters.iter().find(|p| p["key"] == "spotting_retention_ticks").unwrap();
        assert_eq!(duration["group"], "Spotting");
        assert_eq!(duration["default"], 100.0);
        assert_eq!(duration["max"], f64::from(u32::MAX));
        let countdown = parameters.iter().find(|p| p["key"] == "countdown_seconds").unwrap();
        assert_eq!(countdown["default"], 10.0);
        assert_eq!(countdown["integer"], true);
        assert!(parameters.iter().all(|p| p["key"] != "hull_physics" && p["key"] != "max_passive_speed_mps"));
        let gravity = parameters.iter().find(|p| p["key"] == "gravity_mps2").unwrap();
        assert!(gravity["min"].as_f64().unwrap() < 0.0);
        assert!(gravity["max"].as_f64().unwrap() > 30.0);
        let track_repair = parameters.iter().find(|p| p["key"] == "track_repair_time_scale").unwrap();
        assert_eq!(track_repair["group"], "Tracks");
        assert_eq!(track_repair["default"], 1.0);
        assert_eq!(track_repair["min"], 0.0);
        assert!(track_repair["max"].as_f64().unwrap() > 10.0);
        assert_eq!(track_repair["integer"], false);
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
        let factors = &loadout["equipment_health_factors"][hardening.to_string()];
        assert!(factors[0].as_f64().unwrap() > 1.0);
        let consumable = loadout["consumables"].as_array().unwrap().iter()
            .find(|entry| entry["name"] == "Small Repair Kit").unwrap()["id"].clone();
        let stock_hp = loadout["stock_hit_points"].as_i64().unwrap();
        let capacity = loadout["ammunition_capacity"].as_u64().unwrap();
        let shells = loadout["ammunition"].as_array().unwrap();
        let shell = shells[0]["id"].as_i64().unwrap();
        assert_eq!(shells[0]["default_count"], capacity);
        assert!(shells[1..].iter().all(|shell| shell["default_count"] == 0));
        assert!(!shells[0]["name"].as_str().unwrap().starts_with("Shell type"));
        let seats = loadout["crew"].as_array().unwrap();
        let driver = seats.iter().position(|seat| seat["name"] == "Driver").unwrap();
        let mut perks = vec![vec![]; seats.len()];
        perks[driver] = vec![33]; // Engineer
        let mut efficiency = vec![100; seats.len()];
        efficiency[driver] = 50;
        let mut battle = json!({
            "arena_type": arena, "duration_seconds": 300,
            "players": [{"account_id": account, "team": 1, "inventory_id": 1}],
            "dummies": [
                {"team": 1, "loadout": {"vehicle": dummy, "equipment": [hardening.clone(), hardening]}},
                {"team": 1, "loadout": {"vehicle": dummy}},
                {"team": 2, "loadout": {"vehicle": dummy}},
            ],
        });
        assert_eq!(request("POST", "/battles", &cookie, battle.clone()).0, 400);
        assert_eq!(peer.resets, 0);
        battle["dummies"][0]["loadout"]["equipment"] = json!([hardening]);
        battle["dummies"][0]["loadout"]["consumables"] = json!([consumable]);
        battle["dummies"][0]["loadout"]["crew_perks"] = json!([[9]]);
        for (field, value) in [
            ("crew_perk_efficiency", json!([101])),
            ("hit_points", json!(0)),
            ("ammunition", json!([[shell, capacity + 1]])),
            ("ammunition", json!([[shell, 1], [shell, 1]])),
            ("ammunition", json!([[0, 1]])),
        ] {
            let mut invalid = battle.clone();
            invalid["dummies"][1]["loadout"][field] = value;
            assert_eq!(request("POST", "/battles", &cookie, invalid).0, 400, "{field}");
        }
        let (dummy_modules, dummy_shell, _) = alternative_gun(&garage, &loadout, dummy);
        let mut wrong_module = battle.clone();
        wrong_module["dummies"][1]["loadout"]["modules"] = json!({"Engine": loadout["stock_modules"]["Gun"]});
        assert_eq!(request("POST", "/battles", &cookie, wrong_module).0, 400);
        battle["dummies"][1]["loadout"]["modules"] = dummy_modules;
        battle["dummies"][1]["loadout"]["hit_points"] = json!(71);
        battle["dummies"][1]["loadout"]["ammunition"] = json!([[dummy_shell, 0]]);
        battle["dummies"][1]["loadout"]["crew_perks"] = json!(perks);
        battle["dummies"][1]["loadout"]["crew_perk_efficiency"] = json!(efficiency);
        for parameters in [
            json!({"unknown_setting": 1}),
            json!({"countdown_seconds": 1.5}),
            json!({"parked_vehicle_pose_interval_ticks": u64::from(u32::MAX) + 1}),
            json!({"countdown_seconds": 429_496_729}),
            json!({"suspension_frequency_hz": -1}),
            json!({"max_flight_segments": 10000}),
            json!({"hull_physics": 0}),
            json!({"track_repair_time_scale": -1.0}),
            json!({"spotting_near_metres": 30}),
        ] {
            let mut invalid = battle.clone();
            invalid["parameters"] = parameters;
            let (status, rejection) = request("POST", "/battles", &cookie, invalid);
            assert_eq!(status, 400);
            assert!(rejection["message"].as_str().unwrap().contains("battle parameter"));
        }
        battle["parameters"] = json!({
            "countdown_seconds": 2,
            "gravity_mps2": 31.0,
            "parked_vehicle_pose_interval_ticks": 5,
            "track_repair_time_scale": 12.125,
            "spotting_retention_ticks": u32::MAX,
        });
        let mut unready = battle.clone();
        unready["players"][0]["inventory_id"] = json!(2);
        let (status, rejected) = request("POST", "/battles", &cookie, unready);
        assert_eq!(status, 409);
        assert!(rejected["message"].as_str().unwrap().contains("Crew incomplete"));

        let custom_tank = options["dummy_vehicles"].as_array().unwrap().iter()
            .find(|vehicle| vehicle["key"] == "usa:A01_T1_Cunningham").unwrap()["id"].as_i64().unwrap();
        let (status, custom_options) =
            request("GET", &format!("/battles/vehicles/{custom_tank}"), &cookie, Json::Null);
        assert_eq!(status, 200, "{custom_options}");
        let (custom_modules, custom_shell, _) = alternative_gun(&garage, &custom_options, custom_tank);
        let custom_loadout = json!({
            "vehicle": custom_tank, "modules": custom_modules, "hit_points": 987, "ammunition": [[custom_shell, 5]],
        });
        let mut ambiguous = battle.clone();
        ambiguous["players"][0]["loadout"] = custom_loadout.clone();
        assert_eq!(request("POST", "/battles", &cookie, ambiguous).0, 400);
        battle["players"][0]["inventory_id"] = Json::Null;
        battle["players"][0]["loadout"] = custom_loadout.clone();
        let mut invalid = battle.clone();
        invalid["players"][0]["loadout"]["hit_points"] = json!(0);
        assert_eq!(request("POST", "/battles", &cookie, invalid).0, 400);
        let custom: wot_accounts::contract::CustomBattleLoadout =
            serde_json::from_value(custom_loadout).unwrap();
        let descriptor = garage.custom_battle_vehicle(&custom, None).unwrap().unwrap().descriptor;

        let (status, launched) = request("POST", "/battles", &cookie, battle.clone());
        assert_eq!(status, 200, "{launched}");
        assert_eq!(launched["players_handed_off"], 1);
        let battle_id = launched["battle_id"].as_u64().unwrap();
        assert_eq!(request("POST", "/battles", &cookie, battle.clone()).0, 409);
        let (status, busy) = request("GET", "/battles", &cookie, Json::Null);
        assert_eq!(status, 200);
        assert_eq!(busy["players"][0]["unavailable_reason"], "In battle");

        peer.until(&script, |p| p.resets == 1);
        assert!(peer.methods.iter().any(|(name, _)| name == "onArenaCreated"));
        peer.enable_entities();
        peer.until(&script, |p| p.arena_roster.is_some());
        assert_eq!(peer.entered_arena, Some((battle_id, arena)));
        let (vehicle_type, schema) = script.entity_from_name("Vehicle").unwrap();
        peer.until(&script, |p| p.arena_entities.iter()
            .filter(|entity| entity.type_id == vehicle_type).count() == 3);
        use wot_platform::mercury::net::codec::Codec;
        let own = peer.cell.as_ref().unwrap().vehicle_id;
        let mut dummy_health = peer.arena_entities.iter()
            .filter(|entity| entity.type_id == vehicle_type)
            .map(|entity| {
                let mut properties = entity.properties.as_slice();
                let count = u8::read(&mut properties, &()).unwrap();
                let mut health = None;
                let mut max_health = None;
                for _ in 0..count {
                    let id = u8::read(&mut properties, &()).unwrap();
                    let property = &schema.properties[usize::from(id)];
                    let value = Value::read(&mut properties, &property.ty).unwrap();
                    match (&*property.name, value) {
                        ("health", value) => health = value.as_i64(),
                        ("publicInfo", Value::Dict(info)) => {
                            max_health = info["maxHealth"].as_i64();
                            if entity.entity_id == own {
                                assert_eq!(
                                    info["compDescr"],
                                    Value::String(wot_platform::mercury::script::StringValue::Raw(descriptor.clone())),
                                );
                            }
                        }
                        _ => {}
                    }
                }
                assert_eq!(health, max_health);
                (entity.entity_id, health.unwrap())
            }).filter_map(|(id, health)| {
                if id == own {
                    assert_eq!(health, 987);
                    None
                } else {
                    Some(health)
                }
            }).collect::<Vec<_>>();
        dummy_health.sort_unstable();
        assert_eq!(dummy_health.len(), 2);
        assert_eq!(dummy_health[0], 71);
        assert!(dummy_health[1] > stock_hp, "Hardening must increase dummy HP: {dummy_health:?}");
        let dummies = peer.arena_entities.iter()
            .filter(|entity| entity.type_id == vehicle_type && entity.entity_id != own)
            .map(|entity| (entity.entity_id, entity.position)).collect::<Vec<_>>();
        assert_eq!(dummies.len(), 2);
        peer.command(&script, "setClientReady", vec![]);
        peer.until(&script, |p| p.methods.iter().any(|(name, args)| {
            if name != "updateArena" || args[0].as_i64() != Some(3) { return false; }
            let Value::String(wot_platform::mercury::script::StringValue::Raw(data)) = &args[1] else { return false; };
            matches!(wot_platform::client_formats::pickle::from_reader(flate2::read::ZlibDecoder::new(data.as_slice())),
                Ok(super::Pickle::Tuple(fields)) if fields.first() == Some(&super::Pickle::I64(2))
                    && fields.get(2) == Some(&super::Pickle::I64(2)))
        }));
        peer.until(&script, |p| p.methods.iter().any(|(name, args)| {
            if name != "updateArena" || args[0].as_i64() != Some(3) { return false; }
            let Value::String(wot_platform::mercury::script::StringValue::Raw(data)) = &args[1] else { return false; };
            matches!(wot_platform::client_formats::pickle::from_reader(flate2::read::ZlibDecoder::new(data.as_slice())),
                Ok(super::Pickle::Tuple(fields)) if fields.first() == Some(&super::Pickle::I64(3)))
        }) && p.arena_tick.is_some());
        let ready_ack = peer.last_sent_reliable + 1;
        let pose_frames = peer.observe_consecutive_arena_ticks(&script, 15);
        assert_eq!(peer.avatar_position_frames - pose_frames.avatar, 15);
        assert_eq!(peer.vehicle_position_frames[&own] - pose_frames.vehicles[&own], 15);
        assert!(peer.snapshot_ack_through.is_some_and(|ack| ack.wrapping_cmp(ready_ack).is_ge()));
        for (id, position) in dummies {
            assert!(peer.positions[&id].distance(position) < 0.01);
            assert_eq!(peer.vehicle_position_frames[&id] - pose_frames.vehicles[&id], 3);
        }
        let own_position = peer.positions[&own];
        peer.cell_command(&script, "vehicle_moveWith", vec![Value::UInt8(1)]);
        peer.until(&script, |p| p.positions[&own].distance(own_position) > 0.2);
        let movement_frames = peer.observe_consecutive_arena_ticks(&script, 5);
        assert_eq!(peer.vehicle_position_frames[&own] - movement_frames.vehicles[&own], 5);
        peer.cell_command(&script, "vehicle_shoot", vec![]);
        peer.until(&script, |p| p.vehicle_methods.iter().any(|(entity, name, _)| {
            *entity == own && name == "showShooting"
        }));
        peer.command(&script, "leaveArena", vec![]);
        peer.until(&script, |p| p.resets == 2);
        peer.enable_entities();
        peer.until(&script, |p| p.created && p.shown);
        peer.clear_resource(617);
        equipment_flow::full_cache(&mut peer, &script);
        let (status, back_in_garage) = request("GET", "/battles", &cookie, Json::Null);
        assert_eq!(status, 200);
        assert!(back_in_garage["players"][0]["unavailable_reason"].is_null());
        assert_eq!(back_in_garage["players"][0]["vehicles"], options["players"][0]["vehicles"]);
        let owned_path = format!("/battles/players/{account}/vehicles/3");
        assert_eq!(request("GET", &owned_path, "", Json::Null).0, 401);
        let (status, owned) = request("GET", &owned_path, &cookie, Json::Null);
        assert_eq!(status, 200, "{owned}");
        assert_eq!(owned["current"]["vehicle"], styled_type);
        assert_eq!(owned["current"]["modules"], owned["stock_modules"]);
        assert_eq!(owned["current"]["ammunition"], json!([[styled_shell, 7]]));
        assert!(owned["equipped_hit_points"].as_i64().unwrap() > 0);
        let (styled_modules, _, styled_descriptor) = alternative_gun(&garage, &owned, i64::from(styled_type));
        battle["players"][0] = json!({
            "account_id": account, "team": 1, "inventory_id": 3,
            "owned_overrides": {"vehicle": styled_type, "modules": styled_modules, "hit_points": 811},
        });
        let mut invalid = battle.clone();
        invalid["players"][0]["owned_overrides"]["vehicle"] = json!(custom_tank);
        assert_eq!(request("POST", "/battles", &cookie, invalid).0, 400);
        battle["parameters"]["parked_vehicle_pose_interval_ticks"] = json!(1);
        let (status, owned_battle) = request("POST", "/battles", &cookie, battle);
        assert_eq!(status, 200, "{owned_battle}");
        peer.until(&script, |p| p.resets == 3);
        peer.enable_entities();
        peer.until(&script, |p| p.arena_roster.is_some());
        let styled_vehicle = peer.cell.as_ref().unwrap().vehicle_id;
        let vehicle = peer.arena_entities.iter().find(|entity| entity.entity_id == styled_vehicle).unwrap();
        let mut properties = vehicle.properties.as_slice();
        let count = u8::read(&mut properties, &()).unwrap();
        let mut public = None;
        for _ in 0..count {
            let id = u8::read(&mut properties, &()).unwrap();
            let property = &schema.properties[usize::from(id)];
            let value = Value::read(&mut properties, &property.ty).unwrap();
            if &*property.name == "publicInfo" {
                public = Some(value);
            }
        }
        let Value::Dict(public) = public.unwrap() else { panic!("expected Vehicle public info") };
        use wot_platform::mercury::script::StringValue::Raw;
        assert_eq!(public["compDescr"], Value::String(Raw(styled_descriptor)));
        let Value::String(wot_platform::mercury::script::StringValue::String(outfit)) = &public["outfit"] else {
            panic!("expected the saved style descriptor");
        };
        assert_eq!(outfit.as_bytes(), &[4, 16, 111]);
        assert_eq!(public["maxHealth"].as_i64(), Some(811));
        peer.until(&script, |p| p.arena_tick.is_some() && p.vehicle_position_frames.len() == 3);
        let counts = peer.observe_consecutive_arena_ticks(&script, 5).vehicles;
        assert_eq!(counts.len(), 3);
        for (id, count) in counts {
            assert_eq!(peer.vehicle_position_frames[&id] - count, 5);
        }
        peer.command(&script, "leaveArena", vec![]);
        peer.until(&script, |p| p.resets == 4);
        peer.enable_entities();
        peer.until(&script, |p| p.created && p.shown);
        peer.clear_resource(617);
        equipment_flow::full_cache(&mut peer, &script);
        let (status, unchanged) = request("GET", &owned_path, &cookie, Json::Null);
        assert_eq!(status, 200, "{unchanged}");
        assert_eq!(unchanged["current"], owned["current"]);
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

    let saved = Accounts::new(repository.clone())
        .snapshot(account)
        .await
        .unwrap();
    assert_eq!(saved.progression.cosmetics, cosmetics);
    assert!(!saved.progression.installed.contains_key(&3));
    assert_eq!(
        saved
            .vehicles
            .iter()
            .find(|vehicle| vehicle.inventory_id == 3)
            .unwrap()
            .ammunition
            .as_ref(),
        Some(&styled_ammo)
    );

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
                && get(s, "input.players.custom_loadout_count").as_deref() == Some("1")
        })
        .unwrap();
    let owned_launch = spans
        .iter()
        .find(|span| {
            span.name == "player_sessions.start_admin_battle"
                && get(span, "input.players.owned_override_count").as_deref() == Some("1")
                && get(span, "output.outcome").as_deref() == Some("launched")
        })
        .unwrap();
    let readiness = spans
        .iter()
        .find(|span| {
            span.name == "accounts.admin_battle_vehicle"
                && span.parent_span_id == owned_launch.span_context.span_id()
                && get(span, "input.loadout.edited").as_deref() == Some("true")
        })
        .unwrap();
    assert!(spans.iter().any(|span| {
        span.name == "accounts.edit_battle_loadout"
            && span.parent_span_id == readiness.span_context.span_id()
            && get(span, "output.vehicle.hit_points").as_deref() == Some("811")
            && get(span, "output.outcome").as_deref() == Some("prepared")
    }));
    let prefill = spans
        .iter()
        .find(|span| span.name == "accounts.owned_battle_loadout_options")
        .unwrap();
    let prefill_handler = spans
        .iter()
        .find(|span| span.span_context.span_id() == prefill.parent_span_id)
        .unwrap();
    assert_eq!(prefill_handler.name, "admin.owned_battle_loadout");
    assert_eq!(get(prefill, "input.inventory_id").as_deref(), Some("3"));
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
    assert_eq!(get(launch, "input.dummies.count").as_deref(), Some("3"));
    for span in [launch, handler] {
        assert_eq!(
            get(span, "input.players.custom_loadout_count").as_deref(),
            Some("1")
        );
    }
    let player_projection = spans
        .iter()
        .find(|s| {
            s.name == "accounts.custom_battle_vehicle"
                && s.parent_span_id == launch.span_context.span_id()
                && get(s, "user.id").as_deref() == Some(account.to_string().as_str())
        })
        .unwrap();
    assert_eq!(
        get(player_projection, "output.vehicle.hit_points").as_deref(),
        Some("987")
    );
    assert_eq!(
        get(player_projection, "output.ammunition.rounds").as_deref(),
        Some("5")
    );
    for span in [launch, handler] {
        assert_eq!(
            get(span, "input.battle.parameters.count").as_deref(),
            Some("5")
        );
    }
    assert!(spans.iter().any(|span| {
        span.name == "mercury.arena_snapshot"
            && get(span, "input.vehicles.count").as_deref() == Some("1")
            && get(span, "output.snapshot.datagrams").as_deref() == Some("1")
            && get(span, "output.snapshot.udp_payload_bytes")
                .and_then(|bytes| bytes.parse::<u64>().ok())
                .is_some_and(|bytes| bytes > 0)
            && get(span, "output.delivery").as_deref() == Some("sent")
    }));
    for name in [
        "accounts.custom_battle_vehicle",
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
    let projected = spans
        .iter()
        .find(|s| {
            s.name == "accounts.custom_battle_vehicle"
                && s.parent_span_id == launch.span_context.span_id()
                && get(s, "input.vehicle.hit_points").as_deref() == Some("71")
        })
        .unwrap();
    assert_eq!(
        get(projected, "input.crew.perk_efficiency_min").as_deref(),
        Some("50")
    );
    assert_eq!(
        get(projected, "input.ammunition.types").as_deref(),
        Some("1")
    );
    assert_eq!(
        get(projected, "output.vehicle.hit_points").as_deref(),
        Some("71")
    );
    assert_eq!(
        get(projected, "output.ammunition.rounds").as_deref(),
        Some("0")
    );
    let forward_speed = |dummy: &opentelemetry_sdk::trace::SpanData| {
        let edits = spans
            .iter()
            .find(|span| {
                span.name == "accounts.edit_battle_loadout"
                    && span.parent_span_id == dummy.span_context.span_id()
            })
            .unwrap();
        let vehicle = spans
            .iter()
            .find(|s| {
                s.name == "accounts.battle_vehicle"
                    && s.parent_span_id == edits.span_context.span_id()
            })
            .unwrap();
        let mobility = spans
            .iter()
            .find(|s| {
                s.name == "accounts.battle_mobility"
                    && s.parent_span_id == vehicle.span_context.span_id()
            })
            .unwrap();
        get(mobility, "output.mobility.forward_speed_mps")
            .unwrap()
            .parse::<f64>()
            .unwrap()
    };
    let default_efficiency = spans
        .iter()
        .find(|s| {
            s.name == "accounts.custom_battle_vehicle"
                && s.parent_span_id == launch.span_context.span_id()
                && get(s, "input.vehicle.hit_points").is_none()
        })
        .unwrap();
    assert!(
        (forward_speed(projected) - forward_speed(default_efficiency) - 0.5 / 3.6).abs() < 1e-4
    );
    let dummy_options = spans
        .iter()
        .find(|s| {
            s.name == "accounts.custom_battle_loadout_options"
                && get(s, "input.vehicle_type") == get(default_efficiency, "input.vehicle_type")
        })
        .unwrap();
    assert!(get(default_efficiency, "input.ammunition.types").is_none());
    assert_eq!(
        get(default_efficiency, "output.ammunition.rounds").unwrap(),
        get(dummy_options, "output.ammunition.capacity").unwrap(),
    );
    let rejected_ammunition = spans
        .iter()
        .find(|s| {
            s.name == "accounts.custom_battle_vehicle"
                && get(s, "input.ammunition.types").is_some()
                && get(s, "output.outcome").as_deref() == Some("invalid_loadout")
        })
        .unwrap();
    assert!(
        spans
            .iter()
            .any(|s| s.name == "player_sessions.start_admin_battle"
                && s.span_context.span_id() == rejected_ammunition.parent_span_id
                && get(s, "output.outcome").as_deref() == Some("invalid_dummy"))
    );
    let preparation = spans
        .iter()
        .find(|s| s.name == "battles.prepare" && s.parent_span_id == launch.span_context.span_id())
        .unwrap();
    assert!(spans.iter().any(|step| step.name == "battles.hull_physics"
        && get(step, "output.physics.substeps").as_deref() == Some("6")
        && spans.iter().any(|command| command.name == "battles.control"
            && step.parent_span_id == command.span_context.span_id())));
    assert_eq!(
        get(preparation, "input.battle.countdown.seconds").as_deref(),
        Some("2")
    );
    assert_eq!(
        get(preparation, "input.battle.gravity.mps2").as_deref(),
        Some("31")
    );
    assert_eq!(
        get(preparation, "input.battle.suspension.frequency.hz").as_deref(),
        Some("3")
    );
    assert_eq!(
        get(preparation, "input.battle.load_timeout.seconds").as_deref(),
        Some("60")
    );
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
    assert!(spans.iter().any(|s| {
        s.name == "player_sessions.start_admin_battle"
            && get(s, "output.outcome").as_deref() == Some("invalid_parameters")
            && get(s, "input.battle.parameters.count").as_deref() == Some("1")
            && spans.iter().any(|parent| {
                parent.name == "admin.start_battle"
                    && parent.span_context.span_id() == s.parent_span_id
                    && get(parent, "output.outcome").as_deref() == Some("invalid_parameters")
            })
    }));
    for expected in [
        "invalid_dummy",
        "invalid_parameters",
        "invalid_player_loadout",
        "player_unavailable",
        "crew_incomplete",
    ] {
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

fn alternative_gun(
    garage: &wot_accounts::contract::garage::GarageQueries,
    options: &Json,
    vehicle: i64,
) -> (Json, i64, Vec<u8>) {
    let stock_gun = &options["stock_modules"]["Gun"];
    let stock_turret = &options["stock_modules"]["Turret"];
    for weapon in options["weapons"].as_array().unwrap() {
        if &weapon["gun"] == stock_gun {
            continue;
        }
        for suspension in options["modules"]["Chassis"].as_array().unwrap() {
            let mut modules = json!({ "Gun": weapon["gun"] });
            if &weapon["turret"] != stock_turret {
                modules["Turret"] = weapon["turret"].clone();
            }
            if suspension["id"] != options["stock_modules"]["Chassis"] {
                modules["Chassis"] = suspension["id"].clone();
            }
            let loadout =
                serde_json::from_value(json!({"vehicle": vehicle, "modules": modules})).unwrap();
            if let Some(projected) = garage.custom_battle_vehicle(&loadout, None).unwrap() {
                return (
                    modules,
                    weapon["ammunition"][0]["id"].as_i64().unwrap(),
                    projected.descriptor,
                );
            }
        }
    }
    panic!("fixture has no compatible alternative gun");
}
