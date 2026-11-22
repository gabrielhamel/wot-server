use super::{
    Peer, ammunition_flow, camouflage_flow, dict, equipment_flow, player_fixture, telemetry_capture,
};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use opentelemetry::trace::{SpanId, TraceId};
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
    accounts::test_support::{Accounts, GarageCatalog, PostgresAccounts},
    platform::client_formats::pickle::Value as Pickle,
    player_sessions::{
        Gateway, GatewayConfig,
        test_support::mercury::{
            app::{dispatch::ScriptDispatch, login::element::LoginResponse},
            script::Value as WireValue,
        },
    },
};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires WG_TEST_DATABASE_URL and WG_RESOURCE_DIR"]
async fn store_purchase_updates_the_garage_and_rejects_replay() {
    telemetry_capture::exporter();
    let database_url = std::env::var("WG_TEST_DATABASE_URL").unwrap();
    let resource_dir = std::env::var("WG_RESOURCE_DIR").unwrap();
    let resources = wot_resources::ResourceBundle::open(Path::new(&resource_dir)).unwrap();
    let repository = PostgresAccounts::new(
        wot_server::server::test_support::connect_database(&database_url, 4)
            .await
            .unwrap(),
    );
    let accounts = Accounts::new(repository.clone());
    let expiry_test_db = repository.connection_pool();
    let username = format!(
        "store_{}",
        std::time::UNIX_EPOCH.elapsed().unwrap().as_nanos()
    );
    let starter = ["usa:A01_T1_Cunningham", "ussr:R19_IS-3"].map(str::to_owned);
    let account_id =
        player_fixture::insert_player(&repository, &username, "Store test", "store", &starter)
            .await
            .unwrap();
    let catalog = Arc::new(GarageCatalog::load(resources.clone(), &starter).unwrap());
    let pz_s35 = catalog
        .vehicle_catalog_entry("germany:G34_S35_captured")
        .unwrap()
        .id;
    let restricted_tog = catalog.vehicle_catalog_entry("uk:GB63_TOG_II").unwrap().id;
    let bz176 = catalog
        .vehicle_catalog_entry("china:Ch47_BZ_176")
        .unwrap()
        .id;
    let is3 = catalog.vehicle_catalog_entry("ussr:R19_IS-3").unwrap().id;
    let lowe_id = catalog
        .vehicle_catalog_entry("germany:G51_Lowe")
        .unwrap()
        .id;
    let test_tank_key = "ussr:R165_Object_703_II_2";
    let test_tank = catalog.vehicle_catalog_entry(test_tank_key).unwrap().id;
    let test_tank_price = catalog
        .store_offers()
        .iter()
        .find(|offer| offer.item == test_tank && offer.price.currency() == "gold")
        .expect("Test tank gold offer")
        .price
        .amount();
    let equipment = catalog
        .store_offers()
        .iter()
        .find(|offer| offer.name == "Improved Ventilation Class 3")
        .unwrap();
    let item = i64::from(equipment.item);
    let improved = catalog
        .store_offers()
        .iter()
        .find(|offer| offer.item == (111 << 8) | 0xf9)
        .filter(|offer| {
            offer.category == wot_server::accounts::test_support::StoreCategory::ImprovedEquipment
        })
        .expect("Improved Compressor from the client definitions");
    let improved_item = i64::from(improved.item);
    let improved_price = improved.price.amount();
    let credits = equipment.price.amount();
    sqlx::query(
        "UPDATE accounts.accounts SET credits = $2, gold = 12200, bonds = 6450 WHERE id = $1",
    )
    .bind(account_id)
    .bind(credits)
    .execute(&repository.connection_pool())
    .await
    .unwrap();
    let dispatch = ScriptDispatch::new(
        wot_server::player_sessions::test_support::mercury::script::load(&resources).unwrap(),
    );
    let mut gateway = Gateway::bind(
        &wot_server::server::test_support::composition(),
        GatewayConfig {
            training_room_visible: true,
            periphery_id: 250,
            login_bind: "127.0.0.1:0".parse().unwrap(),
            base_bind: "127.0.0.1:0".parse().unwrap(),
            advertised_base: None,
            private_key: None,
            max_clients: 2,
            max_pending: 4,
        },
        &resources,
        player_fixture::gateway_accounts(&repository, &catalog),
        wot_server::social::postgres::service(
            &wot_server::server::test_support::composition(),
            repository.connection_pool(),
            wot_server::accounts::test_support::directory(repository.connection_pool()),
        ),
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
                .strip_prefix("http://127.0.0.1:8081/shop/#")
                .unwrap()
                .to_owned();
            (peer, token, state)
        };
        let (mut peer, token, state) = connect();
        let expect_tech_tree_unlocks = |data: &Pickle| {
            let Pickle::Set(unlocks) = dict(&dict(data, "economics"), "unlocks") else {
                panic!("Expected Tech Tree economics unlocks");
            };
            assert!(unlocks.contains(&Pickle::from(pz_s35)));
            assert!(!unlocks.contains(&Pickle::from(restricted_tog)));
            assert!(!unlocks.contains(&Pickle::from(2065))); // Researchable Pz. II.
        };
        expect_tech_tree_unlocks(&state);
        let Pickle::I64(cache_revision) = dict(&state, "rev") else { panic!("Expected revision"); };
        peer.command(&dispatch, "doCmdInt3", vec![
            WireValue::Int16(809), WireValue::Int16(100), WireValue::Int64(cache_revision),
            WireValue::Int64(0), WireValue::Int64(0),
        ]);
        peer.until(&dispatch, |peer| peer.responses.contains(&(809, 0)));
        expect_tech_tree_unlocks(&ammunition_flow::receipt(&peer, 809));
        let (status, catalog) = request(Some(&token), None);
        assert_eq!(status, StatusCode::OK);
        let find_offer = |listing: &Value, item| -> Value {
            listing["offers"].as_array().unwrap().iter()
                .find(|offer| offer["item"] == item).unwrap().clone()
        };
        let offer = find_offer(&catalog, item);
        assert_eq!(offer["in_depot"], 0);
        assert_eq!(offer["price"], json!({"currency": "credits", "amount": credits}));
        assert!(catalog.get("credits").is_none());
        let cost = credits;
        let revision: i64 = catalog["revision"].as_str().unwrap().parse().unwrap();
        let order = json!({"item": item, "price": offer["price"], "revision": catalog["revision"]});
        let mut wrong_price = order.clone();
        wrong_price["price"]["amount"] = json!(cost - 1);
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
        assert_eq!(find_offer(&committed.1, item)["availability"], "insufficient_funds");
        assert_eq!(find_offer(&committed.1, item)["in_depot"], 1);
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
        let new_order = json!({"item": item, "price": offer["price"], "revision": committed.1["revision"]});
        assert_eq!(
            request(Some(&token), Some(&new_order)).0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        let offers = committed.1["offers"].as_array().unwrap();
        let locked = offers.iter().find(|offer|
            offer["category"] == "collectors_vehicles" && offer["availability"] == "research_required"
        ).unwrap();
        let locked_order = json!({"item": locked["item"], "price": locked["price"], "revision": committed.1["revision"]});
        assert_eq!(
            request(Some(&token), Some(&locked_order)).0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        for (category, count) in [
            ("premium_account", 8),
            ("collectors_vehicles", 92),
            ("crew_skins", 25),
            ("equipment", 42),
            ("consumables", 19),
            ("crew_directives", 9),
            ("bond_vehicles", 54),
            ("equipment_directives", 6),
        ] {
            assert_eq!(
                offers.iter().filter(|offer| offer["category"] == category).count(),
                count,
                "{category} offer count",
            );
        }
        let improved_offers = offers.iter().filter(|offer| offer["category"] == "improved_equipment");
        assert!(improved_offers.clone().count() > 8);
        assert!(improved_offers.clone().all(|offer| offer["price"]["currency"] == "bonds"));
        let offer_ids: std::collections::BTreeSet<_> = offers.iter().map(|offer| {
            (offer["item"].as_i64().unwrap(), offer["price"]["currency"].as_str().unwrap())
        }).collect();
        assert_eq!(offer_ids.len(), offers.len(), "Store offers are unique per item and currency");
        let lowe = offers.iter().find(|offer| offer["category"] == "premium_vehicles" && offer["item"] == lowe_id).unwrap();
        assert_eq!(lowe["price"], json!({"currency": "gold", "amount": 9500}));
        let tog_offer = offers.iter().find(|offer| offer["category"] == "bond_vehicles" && offer["name"] == "TOG II*").unwrap();
        assert!(offers.iter().any(|offer| {
            offer["item"] == tog_offer["item"] && offer["category"] == "premium_vehicles"
        }), "TOG II* has both bond and gold offers");
        let bz_offer = find_offer(&committed.1, i64::from(bz176));
        assert_eq!(bz_offer["category"], "premium_vehicles");
        assert_eq!(bz_offer["price"], json!({"currency": "gold", "amount": 8800}));

        let premium_offer = find_offer(&committed.1, -1);
        assert_eq!(premium_offer["price"], json!({"currency": "gold", "amount": 250}));
        let mut premium_order = json!({"item": -1, "price": premium_offer["price"], "revision": committed.1["revision"]});
        let before = std::time::UNIX_EPOCH.elapsed().unwrap().as_secs();
        let (status, activated) = request(Some(&token), Some(&premium_order));
        assert_eq!(status, StatusCode::OK);
        let expiry = activated["premium_expires_at"].as_u64().unwrap();
        assert!(expiry >= before + 86_400);
        assert!(expiry <= std::time::UNIX_EPOCH.elapsed().unwrap().as_secs() + 86_400);
        let premium_diff = next_update(&mut peer, &dispatch, revision + 2);
        assert_eq!(dict(&dict(&premium_diff, "premium"), "premMask"), Pickle::I64(2));
        assert_eq!(dict(&dict(&premium_diff, "stats"), "gold"), Pickle::I64(11950));
        assert_eq!(request(Some(&token), Some(&premium_order)).0, StatusCode::CONFLICT);
        premium_order["revision"] = activated["revision"].clone();
        let (status, extended) = request(Some(&token), Some(&premium_order));
        assert_eq!(status, StatusCode::OK);
        assert_eq!(extended["premium_expires_at"], expiry + 86_400);
        let premium_diff = next_update(&mut peer, &dispatch, revision + 3);
        let Pickle::Dict(premium) = dict(&premium_diff, "premium") else { panic!("Expected premium diff"); };
        assert_eq!(premium[&Pickle::I64(2)], Pickle::I64((expiry + 86_400) as i64));
        let bond_item = extended["offers"].as_array().unwrap().iter()
            .find(|offer| offer["category"] == "bond_vehicles" && offer["name"] == "TOG II*").unwrap();
        let bond_order = json!({"item": bond_item["item"], "price": bond_item["price"], "revision": extended["revision"]});
        let (status, purchased) = request(Some(&token), Some(&bond_order));
        assert_eq!(status, StatusCode::OK);
        assert_eq!(find_offer(&purchased, bond_item["item"].as_i64().unwrap())["availability"], "in_garage");
        let vehicle_diff = next_update(&mut peer, &dispatch, revision + 4);
        assert_eq!(dict(&dict(&vehicle_diff, "stats"), "crystal"), Pickle::I64(2450));
        let persisted = runtime.block_on(accounts.snapshot(account_id)).unwrap();
        assert_eq!(persisted.balances.bonds, 2450);
        assert!(persisted.vehicles.iter().any(|vehicle| vehicle.catalog_key == "uk:GB63_TOG_II"));
        assert_eq!(u64::from(persisted.progression.premium_expires_at), expiry + 86_400);
        let styles: [(_, _, _, _, _, &[u8]); 2] = [
            (850, "Hot TOG (TOG II*)", "uk:GB63_TOG_II", restricted_tog, 291, &[4, 16, 163, 2]),
            (860, "Montu (IS-3)", "ussr:R19_IS-3", is3, 111, &[4, 16, 111]),
        ];
        let mut fitted = purchased;
        for (request_id, name, vehicle_key, vehicle_type, style_id, outfit) in styles {
            let store_style = fitted["offers"].as_array().unwrap().iter()
                .find(|offer| offer["name"] == name).unwrap();
            let style_item = store_style["item"].as_i64().unwrap();
            let inventory_id = persisted.vehicles.iter().find(|vehicle| vehicle.catalog_key == vehicle_key)
                .unwrap().inventory_id;
            let rows = [(1, outfit), (2, outfit), (4, outfit), (15, outfit)];
            camouflage_flow::send_outfits(&mut peer, &dispatch, request_id, inventory_id, &rows, -1);
            let style_order = json!({"item": style_item, "price": store_style["price"], "revision": fitted["revision"]});
            let (status, styled) = request(Some(&token), Some(&style_order));
            assert_eq!(status, StatusCode::OK);
            assert_eq!(find_offer(&styled, style_item)["in_depot"], 1);

            let revision = styled["revision"].as_str().unwrap().parse::<i64>().unwrap();
            camouflage_flow::send_outfits(&mut peer, &dispatch, request_id + 1, 1, &rows, -1);
            camouflage_flow::send_outfits(&mut peer, &dispatch, request_id + 2, inventory_id, &rows, 0);
            let diff = next_update(&mut peer, &dispatch, revision + 1);
            assert_style_fitted(&diff, vehicle_type, style_id, outfit, true);
            camouflage_flow::send_outfits(&mut peer, &dispatch, request_id + 3, inventory_id, &rows, 0);
            fitted = request(Some(&token), None).1;
            assert_eq!(fitted["revision"], (revision + 1).to_string());
            assert_eq!(find_offer(&fitted, style_item)["in_depot"], 0);

            let empty = [(15, &[][..]), (1, &[][..]), (2, &[][..]), (4, &[][..])];
            camouflage_flow::send_outfits(&mut peer, &dispatch, request_id + 4, inventory_id, &empty, 0);
            let removed = request(Some(&token), None).1;
            assert_eq!(find_offer(&removed, style_item)["in_depot"], 1);
            camouflage_flow::send_outfits(&mut peer, &dispatch, request_id + 5, inventory_id, &rows, 0);
            fitted = request(Some(&token), None).1;
            assert_eq!(fitted["revision"], (revision + 3).to_string());
            assert_eq!(find_offer(&fitted, style_item)["in_depot"], 0);
            assert_style_traces(account_id, request_id, style_id);
        }

        let su_gold = fitted["offers"].as_array().unwrap().iter()
            .find(|offer| offer["category"] == "premium_vehicles" && offer["name"] == "SU-100Y").unwrap();
        assert_eq!(su_gold["price"], json!({"currency": "gold", "amount": 2900}));
        let su_offers = |listing: &Value| {
            listing["offers"].as_array().unwrap().iter()
                .filter(|offer| offer["item"] == su_gold["item"]).cloned().collect::<Vec<_>>()
        };
        let offers = su_offers(&fitted);
        assert_eq!(offers.len(), 2);
        assert!(offers.iter().any(|offer| offer["price"] == json!({"currency": "bonds", "amount": 5000})));
        let buy_su = json!({"item": su_gold["item"], "price": su_gold["price"], "revision": fitted["revision"]});
        let mut wrong_currency = buy_su.clone();
        wrong_currency["price"]["currency"] = json!("bonds");
        assert_eq!(request(Some(&token), Some(&wrong_currency)).0, StatusCode::CONFLICT);
        let (status, purchased_su) = request(Some(&token), Some(&buy_su));
        assert_eq!(status, StatusCode::OK);
        assert!(su_offers(&purchased_su).iter().all(|offer| offer["availability"] == "in_garage"));
        let su_revision = purchased_su["revision"].as_str().unwrap().parse::<i64>().unwrap();
        let diff = next_update(&mut peer, &dispatch, su_revision);
        assert_eq!(dict(&dict(&diff, "stats"), "gold"), Pickle::I64(8800));
        assert_eq!(dict(&dict(&diff, "stats"), "crystal"), Pickle::I64(0));
        super::depot_flow::send_array(&mut peer, &dispatch, 816, 301, &[1, bz176, 0, 0, -1, 0], -1);
        let buy_bz = json!({"item": bz176, "price": bz_offer["price"], "revision": purchased_su["revision"]});
        let (status, purchased_bz) = request(Some(&token), Some(&buy_bz));
        assert_eq!(status, StatusCode::OK);
        assert_eq!(find_offer(&purchased_bz, i64::from(bz176))["availability"], "in_garage");
        assert_eq!(request(Some(&token), Some(&buy_bz)).0, StatusCode::CONFLICT);
        let diff = next_update(&mut peer, &dispatch, su_revision + 1);
        assert_eq!(dict(&dict(&diff, "stats"), "gold"), Pickle::I64(0));
        premium_order["revision"] = purchased_bz["revision"].clone();
        assert_eq!(request(Some(&token), Some(&premium_order)).0, StatusCode::UNPROCESSABLE_ENTITY);

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
        for (_, _, _, vehicle_type, style_id, outfit) in styles {
            assert_style_fitted(&state, vehicle_type, style_id, outfit, false);
        }
        assert_eq!(dict(&dict(&state, "premium"), "premMask"), Pickle::I64(2));
        assert_eq!(request(Some(&fresh_token), None).1["premium_expires_at"], expiry + 86_400);
        assert_eq!(
            request(Some(&fresh_token), Some(&order)).0,
            StatusCode::CONFLICT
        );
        reconnected.disconnect();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while request(Some(&fresh_token), None).0 != StatusCode::UNAUTHORIZED {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(25));
        }

        let near_expiry = i64::try_from(std::time::UNIX_EPOCH.elapsed().unwrap().as_secs()).unwrap() + 5;
        runtime.block_on(
            sqlx::query("UPDATE accounts.accounts SET progression = jsonb_set(progression, '{premium_expires_at}', to_jsonb($2::bigint)), gold = $3, bonds = $4 WHERE id = $1")
                .bind(account_id)
                .bind(near_expiry)
                .bind(1250 + test_tank_price)
                .bind(improved_price + 100)
                .execute(&expiry_test_db)
        ).unwrap();
        let (mut expiring, expiry_token, state) = connect();
        assert_eq!(dict(&dict(&state, "premium"), "premMask"), Pickle::I64(2));
        let current = dict(&state, "rev");
        expiring.until(&dispatch, |peer| {
            peer.methods.iter().any(|(name, args)| {
                name.ends_with("update")
                    && dict(&dict(&ammunition_flow::pickle_value(&args[0]), "premium"), "premMask") == Pickle::I64(0)
            })
        });
        let (status, expired) = request(Some(&expiry_token), None);
        assert_eq!(status, StatusCode::OK);
        assert!(expired["premium_expires_at"].is_null());
        let persisted = runtime.block_on(accounts.snapshot(account_id)).unwrap();
        assert_eq!(current, Pickle::I64(persisted.revision));
        assert_eq!(i64::from(persisted.progression.premium_expires_at), near_expiry);
        assert_eq!(persisted.active_premium_until(), None);

        expiring.command(&dispatch, "doCmdInt3", vec![
            WireValue::Int16(813),
            WireValue::Int16(204),
            WireValue::Int64(1),
            WireValue::Int64(1),
            WireValue::Int64(0),
        ]);
        let native_diff = next_update(&mut expiring, &dispatch, persisted.revision + 1);
        assert_eq!(dict(&dict(&native_diff, "premium"), "premMask"), Pickle::I64(2));
        assert_eq!(dict(&dict(&native_diff, "stats"), "gold"), Pickle::I64(1000 + test_tank_price));
        assert!(!persisted.progression.unlocks.contains(&pz_s35));
        super::depot_flow::send_array(&mut expiring, &dispatch, 814, 301, &[1, pz_s35, 0, 0, -1, 0], 0);
        let diff = next_update(&mut expiring, &dispatch, persisted.revision + 2);
        assert_eq!(dict(&dict(&diff, "stats"), "gold"), Pickle::I64(test_tank_price));
        let (_, current_store) = request(Some(&expiry_token), None);
        assert_eq!(find_offer(&current_store, i64::from(pz_s35))["availability"], "in_garage");
        super::depot_flow::send_array(&mut expiring, &dispatch, 815, 301, &[1, pz_s35, 0, 0, -1, 0], -1);
        let saved = runtime.block_on(accounts.snapshot(account_id)).unwrap();
        assert_eq!(saved.revision, persisted.revision + 2);
        assert!(saved.vehicles.iter().any(|vehicle| vehicle.catalog_key == "germany:G34_S35_captured"));
        assert!(saved.vehicles.iter().any(|vehicle| vehicle.catalog_key == "china:Ch47_BZ_176"));
        let offer = find_offer(&current_store, improved_item);
        assert_eq!(offer["price"], json!({"currency": "bonds", "amount": improved_price}));
        let order = json!({
            "item": improved_item,
            "price": offer["price"],
            "revision": current_store["revision"],
        });
        let (status, purchased) = request(Some(&expiry_token), Some(&order));
        assert_eq!(status, StatusCode::OK);
        assert_eq!(find_offer(&purchased, improved_item)["in_depot"], 1);
        assert_eq!(request(Some(&expiry_token), Some(&order)).0, StatusCode::CONFLICT);
        let diff = next_update(&mut expiring, &dispatch, saved.revision + 1);
        assert_eq!(dict(&dict(&diff, "stats"), "crystal"), Pickle::I64(saved.balances.bonds - improved_price));
        assert_depot_item(&diff, improved_item);
        let persisted = runtime.block_on(accounts.snapshot(account_id)).unwrap();
        assert_eq!(persisted.balances.bonds, saved.balances.bonds - improved_price);
        assert_eq!(persisted.progression.depot.get(&(improved_item as i32)), Some(&1));
        assert_store_traces(account_id, improved_item, "bonds", improved_price);

        let item_id = i64::from(test_tank);
        let test_offer = find_offer(&purchased, item_id);
        assert_eq!(test_offer["availability"], "available");
        assert_eq!(test_offer["price"], json!({"currency": "gold", "amount": test_tank_price}));
        super::depot_flow::send_array(&mut expiring, &dispatch, 817, 301, &[1, test_tank, 0, 0, -1, 0], -1);
        let test_order = json!({"item": item_id, "price": test_offer["price"], "revision": purchased["revision"]});
        let (status, bought_test_tank) = request(Some(&expiry_token), Some(&test_order));
        assert_eq!(status, StatusCode::OK);
        assert_eq!(find_offer(&bought_test_tank, item_id)["availability"], "in_garage");
        assert_eq!(request(Some(&expiry_token), Some(&test_order)).0, StatusCode::CONFLICT);
        let diff = next_update(&mut expiring, &dispatch, persisted.revision + 1);
        assert_eq!(dict(&dict(&diff, "stats"), "gold"), Pickle::I64(0));
        let saved = runtime.block_on(accounts.snapshot(account_id)).unwrap();
        assert!(saved.vehicles.iter().any(|vehicle| vehicle.catalog_key == test_tank_key));
        assert_store_traces(account_id, item_id, "gold", test_tank_price);

        expiring.disconnect();
        assert_tech_tree_traces(account_id);
        assert_store_traces(account_id, item, "credits", cost);
        assert_store_traces(account_id, -1, "gold", 250);
        assert_store_traces(account_id, su_gold["item"].as_i64().unwrap(), "gold", 2900);
        assert_store_traces(account_id, i64::from(bz176), "gold", 8800);
    })
    .await;
    stop.store(true, Ordering::Relaxed);
    worker.await.unwrap().unwrap();
    sqlx::query("DELETE FROM accounts.accounts WHERE id = $1")
        .bind(account_id)
        .execute(&repository.connection_pool())
        .await
        .unwrap();
    exercise.unwrap();
}

pub async fn assert_catalog_access(router: &Router, token: &str, phase: u8, status: StatusCode) {
    let trace_id = battle_catalog_trace_id(phase);
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/shop/catalog")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header("traceparent", format!("00-{trace_id}-0123456789abcdef-01"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), status, "battle phase {phase}");
}

fn battle_catalog_trace_id(phase: u8) -> TraceId {
    TraceId::from(0xc0a321d7b8e000000000000000000000 | u128::from(phase))
}

pub fn assert_battle_catalog_traces(account: i64) {
    let spans = telemetry_capture::exporter().get_finished_spans().unwrap();
    let attribute = |span: &opentelemetry_sdk::trace::SpanData, name| {
        span.attributes
            .iter()
            .find(|attr| attr.key.as_str() == name)
            .map(|attr| attr.value.to_string())
    };
    for (phase, status, outcome, player) in [
        (1, "200", "success", Some(account)),
        (2, "401", "unauthorized", None),
        (3, "200", "success", Some(account)),
    ] {
        let trace_id = battle_catalog_trace_id(phase);
        let find = |name| {
            spans
                .iter()
                .find(|span| span.name == name && span.span_context.trace_id() == trace_id)
                .unwrap()
        };
        let http = find("store.http.request");
        let catalog = find("player_sessions.store.catalog_request");
        assert_eq!(http.parent_span_id, SpanId::from(0x0123456789abcdef));
        assert_eq!(catalog.parent_span_id, http.span_context.span_id());
        assert_eq!(
            attribute(http, "http.response.status_code").as_deref(),
            Some(status)
        );
        assert_eq!(
            attribute(catalog, "input.channel").as_deref(),
            Some("garage")
        );
        assert_eq!(
            attribute(catalog, "user.id"),
            player.map(|id| id.to_string())
        );
        assert_eq!(
            attribute(catalog, "output.outcome").as_deref(),
            Some(outcome)
        );
    }
}

async fn store_request(
    router: &Router,
    token: Option<&str>,
    order: Option<&Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder().uri(if order.is_some() {
        "/api/shop/orders"
    } else {
        "/api/shop/catalog"
    });
    if let Some(order) = order {
        let revision = order["revision"].as_str().unwrap().parse::<u128>().unwrap();
        request = request.header(
            "traceparent",
            format!("00-fbdc84ebd1325763{revision:016x}-0123456789abcdef-01"),
        );
    }
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
    let body = to_bytes(response.into_body(), 512 * 1024).await.unwrap();
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

fn assert_style_fitted(cache: &Pickle, vehicle_type: i32, style: i64, outfit: &[u8], diff: bool) {
    let vehicle = i64::from(vehicle_type);
    let inventory = dict(cache, "inventory");
    let lookup = |keys: &[i64]| {
        keys.iter().fold(&inventory, |value, key| {
            let Pickle::Dict(rows) = value else {
                panic!("Expected a customization inventory dictionary");
            };
            &rows[&Pickle::I64(*key)]
        })
    };
    assert_eq!(
        lookup(&[12, 2, vehicle]),
        &Pickle::Dict([(Pickle::I64(15), Pickle::Bytes(outfit.to_vec()))].into())
    );
    let Pickle::Dict(fitted) = lookup(&[12, 4, 4]) else {
        panic!("Expected fitted styles");
    };
    let style_key = if diff {
        Pickle::Tuple(vec![Pickle::I64(style), Pickle::from("_r")])
    } else {
        Pickle::I64(style)
    };
    assert_eq!(
        fitted[&style_key],
        Pickle::Dict([(Pickle::I64(vehicle), Pickle::I64(1))].into())
    );
}

fn assert_style_traces(account: i64, first_request: i16, style: i64) {
    let spans = telemetry_capture::exporter().get_finished_spans().unwrap();
    let has = |span: &opentelemetry_sdk::trace::SpanData, key, value: &str| {
        span.attributes
            .iter()
            .any(|attribute| attribute.key.as_str() == key && attribute.value.to_string() == value)
    };
    for (offset, outcome) in [(0, "rejected"), (2, "committed"), (3, "unchanged")] {
        let request = (first_request + offset).to_string();
        let apply = spans
            .iter()
            .find(|span| {
                span.name == "game.cosmetics.apply" && has(span, "input.request_id", &request)
            })
            .unwrap();
        let in_trace = |name| {
            spans.iter().find(|span| {
                span.name == name && span.span_context.trace_id() == apply.span_context.trace_id()
            })
        };
        assert!(has(apply, "input.outfits.count", "4"));
        let command = in_trace("player_sessions.commands.command").unwrap();
        let progression = in_trace("player_sessions.gateway.change_progression").unwrap();
        let plan = in_trace("accounts.progression.plan_progression").unwrap();
        let decision = in_trace("accounts.cosmetics.change").unwrap();
        let response = in_trace("player_sessions.gateway.progression_response").unwrap();
        assert_eq!(apply.parent_span_id, command.span_context.span_id());
        assert_eq!(progression.parent_span_id, apply.span_context.span_id());
        assert_eq!(plan.parent_span_id, progression.span_context.span_id());
        assert_eq!(decision.parent_span_id, plan.span_context.span_id());
        assert_eq!(response.parent_span_id, progression.span_context.span_id());
        assert!(has(decision, "user.id", &account.to_string()));
        assert!(has(decision, "input.style_id", &style.to_string()));
        assert!(has(decision, "input.items.count", "1"));
        assert!(has(response, "output.outcome", outcome));
        assert!(has(response, "output.delivery", "queued"));
        assert!(spans.iter().any(|span| {
            span.name == "mercury.response"
                && span.parent_span_id == response.span_context.span_id()
                && has(span, "input.request_id", &request)
                && has(
                    span,
                    "output.response_code",
                    if outcome == "rejected" { "-1" } else { "0" },
                )
                && has(span, "output.delivery", "sent")
        }));
        if outcome == "committed" {
            let save = in_trace("accounts.postgres.save_progression").unwrap();
            assert!(spans.iter().any(|span| {
                span.name == "postgresql.query"
                    && span.parent_span_id == save.span_context.span_id()
                    && has(span, "db.operation.name", "UPDATE")
                    && span
                        .attributes
                        .iter()
                        .any(|a| a.key.as_str() == "db.query.text")
            }));
            assert!(has(decision, "output.purchases.count", "0"));
        } else {
            assert!(in_trace("accounts.postgres.save_progression").is_none());
        }
        if outcome == "rejected" {
            assert_eq!(decision.status, opentelemetry::trace::Status::error(""));
        }
    }
}

fn next_update(peer: &mut Peer, dispatch: &ScriptDispatch, revision: i64) -> Pickle {
    peer.until(dispatch, |peer| {
        peer.methods.iter().any(|(name, args)| {
            name.ends_with("update")
                && dict(&ammunition_flow::pickle_value(&args[0]), "rev") == Pickle::I64(revision)
        })
    });
    ammunition_flow::update(peer, revision)
}

fn assert_tech_tree_traces(account_id: i64) {
    let spans = telemetry_capture::exporter().get_finished_spans().unwrap();
    let has = |span: &opentelemetry_sdk::trace::SpanData, key, value: &str| {
        span.attributes
            .iter()
            .any(|attr| attr.key.as_str() == key && attr.value.to_string() == value)
    };
    let command = |request_id| {
        spans
            .iter()
            .find(|span| {
                span.name == "game.command"
                    && has(span, "user.id", &account_id.to_string())
                    && has(span, "input.request_id", request_id)
            })
            .unwrap()
    };
    let sync = command("809");
    let garage = spans
        .iter()
        .find(|span| {
            span.name == "player_sessions.sync_garage"
                && span.span_context.trace_id() == sync.span_context.trace_id()
        })
        .unwrap();
    assert!(spans.iter().any(|span| {
        span.name == "accounts.tech_tree.economics"
            && span.parent_span_id == garage.span_context.span_id()
            && span.attributes.iter().any(|attribute| {
                attribute.key.as_str() == "output.unlocked_vehicle_count"
                    && attribute
                        .value
                        .to_string()
                        .parse::<u64>()
                        .is_ok_and(|count| count > 0)
            })
    }));
    for (request, outcome) in [("814", "committed"), ("815", "rejected")] {
        let trace = command(request).span_context.trace_id();
        assert!(spans.iter().any(|span| {
            span.name == "accounts.vehicles.buy" && span.span_context.trace_id() == trace
        }));
        assert!(spans.iter().any(|span| {
            span.name == "player_sessions.gateway.progression_response"
                && span.span_context.trace_id() == trace
                && has(span, "output.outcome", outcome)
                && has(span, "output.delivery", "queued")
        }));
        assert_eq!(
            spans.iter().any(|span| {
                span.name == "accounts.save_progression" && span.span_context.trace_id() == trace
            }),
            outcome == "committed"
        );
    }
}

fn assert_store_traces(account_id: i64, item: i64, currency: &str, amount: i64) {
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
        assert_eq!(
            root.parent_span_id,
            SpanId::from_hex("0123456789abcdef").unwrap()
        );
        assert_ne!(root.span_context.trace_id(), TraceId::INVALID);
        assert_eq!(
            root.span_context.trace_id(),
            operation.span_context.trace_id()
        );
        assert!(has(root, "http.route", "/api/shop/orders"));
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
    assert!(has(plan, "input.price.currency", currency));
    assert!(has(plan, "input.price.amount", &amount.to_string()));
    if item == -1 {
        let progression = child(plan, "accounts.progression.plan_progression");
        let premium = child(progression, "accounts.premium.extend");
        assert!(has(premium, "input.duration.days", "1"));
        assert!(has(premium, "input.price.gold", "250"));
        assert!(
            premium
                .attributes
                .iter()
                .any(|attr| attr.key.as_str() == "output.premium.expires_at")
        );
    }
    let commit = child(purchase, "accounts.garage.commit_progression");
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
            || span.name != "accounts.garage.commit_progression"
    ));
}
