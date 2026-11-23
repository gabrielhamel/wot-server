use super::{Peer, Pickle, Value, ammunition_flow, dict, player_fixture, telemetry_capture};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use wot_accounts::test_support::{
    Accounts, GarageCatalog, GrantCurrency, GrantError, PlayerGrant, PostgresAccounts,
    ProgressionError,
};
use wot_platform::mercury::app::{dispatch::ScriptDispatch, login::element::LoginResponse};
use wot_player_sessions::{
    Gateway, GatewayConfig,
    contract::{GrantDelivery, GrantRequestError},
};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires WG_TEST_DATABASE_URL and WG_RESOURCE_DIR; uses temporary accounts in that database"]
async fn admin_grants_reach_live_garage_and_survive_reconnect() {
    let exporter = telemetry_capture::exporter();
    let url = std::env::var("WG_TEST_DATABASE_URL").unwrap();
    let resources = wot_resources::ResourceBundle::open(std::path::Path::new(
        &std::env::var("WG_RESOURCE_DIR").unwrap(),
    ))
    .unwrap();
    let repository = PostgresAccounts::new(
        wot_server_runtime::test_support::connect_database(&url, 4)
            .await
            .unwrap(),
    );
    let accounts = Accounts::new(repository.clone());
    let username = format!(
        "grants_{}@example.test",
        std::time::UNIX_EPOCH.elapsed().unwrap().as_nanos()
    );
    let starter = "usa:A01_T1_Cunningham".to_owned();
    let id = player_fixture::insert_player(
        &repository,
        &username,
        "Grant test",
        "grants",
        std::slice::from_ref(&starter),
    )
    .await
    .unwrap();
    let catalog = Arc::new(GarageCatalog::load(resources.clone(), &[starter]).unwrap());
    let snapshot = accounts.snapshot(id).await.unwrap();
    for key in [
        "germany:G90_DW_II_MapsTraining_Dummy_HT_1",
        "france:F28_105_leFH18B2_IGR",
        "ussr:Observer",
    ] {
        let entry = catalog.vehicle_catalog_entry(key).unwrap();
        assert!(
            !catalog
                .grantable_vehicles()
                .any(|vehicle| vehicle.id == entry.id),
            "{key}"
        );
        assert!(
            matches!(
                catalog.plan_grant(snapshot.clone(), PlayerGrant::Vehicle { vehicle: entry.id }),
                Err(GrantError::Vehicle(ProgressionError::VehicleNotFound))
            ),
            "{key}"
        );
    }
    for key in [
        "germany:G98_WT_auf_E_100_Phonix",
        "germany:G195_HWK_40",
        "poland:Pl43_Kruk",
        "uk:GB132_Taurus_CAT",
    ] {
        assert!(
            catalog.grantable_vehicles().any(|tank| tank.key == key),
            "{key}"
        );
    }
    let test_tank = catalog
        .grantable_vehicles()
        .find(|tank| tank.key == "ussr:R165_Object_703_II_2")
        .expect("The hidden Store variant is offered in the admin vehicle picker");
    catalog
        .plan_grant(
            snapshot.clone(),
            PlayerGrant::Vehicle {
                vehicle: test_tank.id,
            },
        )
        .expect("The admin grant can add the hidden Store variant");
    let tank_key = "china:Ch65_Type59_IC4_3DSt";
    let tank = catalog
        .grantable_vehicles()
        .find(|tank| tank.key == tank_key)
        .unwrap()
        .id;
    let books: Vec<_> = catalog.grantable_crew_books().collect();
    assert!(
        books
            .iter()
            .any(|(_, name)| name.contains("Training Booklet") && name.contains("U.S.S.R."))
    );
    assert!(
        books
            .iter()
            .any(|(_, name)| name.contains("Universal Guide"))
    );
    assert!(
        books
            .iter()
            .any(|(_, name)| name.contains("Personal Training Manual"))
    );
    let book = books
        .iter()
        .find(|(_, name)| name.contains("Personal Training Manual"))
        .unwrap()
        .0;
    let experimental: Vec<_> = catalog.grantable_experimental_equipment().collect();
    for tier in ["T1", "T2", "T3"] {
        assert!(experimental.iter().any(|(_, name)| name.ends_with(tier)));
    }
    let equipment = experimental
        .iter()
        .find(|(_, name)| name.starts_with("Fire-Control System") && name.ends_with("T3"))
        .unwrap()
        .0;
    let bounty: Vec<_> = catalog.grantable_bounty_equipment().collect();
    let (bounty_basic, bounty_upgraded) = (13049, 13305);
    for (id, level) in [(bounty_basic, " · Basic"), (bounty_upgraded, " · Upgraded")] {
        let (_, name) = bounty.iter().find(|(item, _)| *item == id).unwrap();
        assert!(name.starts_with("Bounty ") && name.ends_with(level));
        assert!(!catalog.store_offers().iter().any(|offer| offer.item == id));
    }
    let improved_item = (111 << 8) | 0xf9;
    assert!(
        catalog
            .grantable_improved_equipment()
            .any(|(id, name)| id == improved_item && !name.is_empty())
    );
    let regular_equipment = catalog
        .store_offers()
        .iter()
        .find(|offer| offer.category == wot_accounts::test_support::StoreCategory::Equipment)
        .unwrap()
        .item;
    assert!(!experimental.iter().any(|(id, _)| *id == regular_equipment));
    let mut at_limit = snapshot.clone();
    at_limit.progression.depot.insert(book, i32::MAX as u32);
    assert!(matches!(
        catalog.plan_grant(at_limit, PlayerGrant::CrewBook { id: book, count: 1 }),
        Err(GrantError::DepotLimit)
    ));
    let script = ScriptDispatch::new(wot_platform::mercury::script::load(&resources).unwrap());
    let gateway = Gateway::bind(
        wot_battles::mercury::bind,
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
        wot_social::postgres::service(
            repository.connection_pool(),
            wot_accounts::test_support::directory(repository.connection_pool()),
        ),
        tokio::runtime::Handle::current(),
    )
    .unwrap();
    let (login, _) = gateway.addresses().unwrap();
    let grants = gateway.player_grants();
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let worker = tokio::task::spawn_blocking(move || gateway.run(worker_stop));
    let runtime = tokio::runtime::Handle::current();
    let test = tokio::task::spawn_blocking(move || {
        let currency = |currency, amount| PlayerGrant::Currency { currency, amount };
        let grant = |request| runtime.block_on(grants.grant(id, request));
        assert!(matches!(grant(currency(GrantCurrency::Gold, 0)), Err(GrantRequestError::Rejected(_))));
        assert!(matches!(grant(currency(GrantCurrency::Credits, i64::MAX)), Err(GrantRequestError::Rejected(_))));
        assert!(matches!(runtime.block_on(grants.grant(i64::MAX, currency(GrantCurrency::Gold, 1))), Err(GrantRequestError::NotFound)));
        assert_eq!(grant(currency(GrantCurrency::Gold, 7)).unwrap().1, GrantDelivery::OnNextSync);

        let mut peer = Peer::new(None);
        let LoginResponse::Success(ticket) = peer.authenticate(login, &username, "grants") else {
            panic!("Login did not return a handoff ticket");
        };
        let (after, delivery) = grant(currency(GrantCurrency::Bonds, 11)).unwrap();
        let after = wot_accounts::test_support::account_snapshot(&after);
        assert_eq!(delivery, GrantDelivery::OnNextSync);
        peer.handoff(ticket);
        peer.until(&script, |p| p.created && p.shown);
        let initial = sync(&mut peer, &script, 300);
        assert_eq!(dict(&initial, "rev"), Pickle::from(after.revision));
        assert_eq!(dict(&dict(&initial, "stats"), "crystal"), Pickle::from(after.balances.bonds));
        assert_eq!(dict(&dict(&initial, "stats"), "gold"), Pickle::from(after.balances.gold));

        let mut last_revision = after.revision;
        for (resource, key) in [(GrantCurrency::Credits, "credits"), (GrantCurrency::Gold, "gold"), (GrantCurrency::Bonds, "crystal"), (GrantCurrency::FreeXp, "freeXP"), (GrantCurrency::EquipmentComponents, "equipCoin")] {
            let (after, delivery) = grant(currency(resource, 31)).unwrap();
            let after = wot_accounts::test_support::account_snapshot(&after);
            assert_eq!(delivery, GrantDelivery::QueuedForGarage);
            peer.until(&script, |p| p.methods.iter().any(|(name, _)| name.ends_with("update")));
            let diff = ammunition_flow::update(&peer, after.revision);
            assert_eq!(dict(&diff, "prevRev"), Pickle::from(last_revision));
            assert_eq!(dict(&diff, "stats"), wot_platform::pickle!({(key): (wot_accounts::test_support::currency_balance(resource, &after.balances))}));
            last_revision = after.revision;
            peer.methods.clear();
        }

        assert!(matches!(
            grant(currency(GrantCurrency::EquipmentComponents, i64::from(i32::MAX))),
            Err(GrantRequestError::Rejected(GrantError::BalanceLimit))
        ));
        assert!(matches!(
            grant(PlayerGrant::CrewBook { id: equipment, count: 1 }),
            Err(GrantRequestError::Rejected(GrantError::UnknownCrewBook))
        ));
        assert!(matches!(
            grant(PlayerGrant::ExperimentalEquipment { id: regular_equipment, count: 1 }),
            Err(GrantRequestError::Rejected(GrantError::UnknownExperimentalEquipment))
        ));
        assert!(matches!(
            grant(PlayerGrant::CrewBook { id: book, count: 101 }),
            Err(GrantRequestError::Rejected(GrantError::InvalidDepotCount))
        ));
        for (request, item_type, id, quantity) in [
            (PlayerGrant::CrewBook { id: book, count: 2 }, 14, book, 2),
            (PlayerGrant::CrewBook { id: book, count: 3 }, 14, book, 5),
            (PlayerGrant::ExperimentalEquipment { id: equipment, count: 1 }, 9, equipment, 1),
            (PlayerGrant::BountyEquipment { id: bounty_basic, count: 2 }, 9, bounty_basic, 2),
            (PlayerGrant::BountyEquipment { id: bounty_upgraded, count: 1 }, 9, bounty_upgraded, 1),
            (PlayerGrant::ImprovedEquipment { id: improved_item, count: 1 }, 9, improved_item, 1),
        ] {
            let (after, delivery) = grant(request).unwrap();
            let after = wot_accounts::test_support::account_snapshot(&after);
            assert_eq!(delivery, GrantDelivery::QueuedForGarage);
            assert_eq!(after.revision, last_revision + 1);
            peer.until(&script, |p| p.methods.iter().any(|(name, _)| name.ends_with("update")));
            let diff = ammunition_flow::update(&peer, after.revision);
            assert_eq!(dict(&diff, "prevRev"), Pickle::from(last_revision));
            assert_eq!(depot_count(&diff, item_type, id), quantity);
            last_revision = after.revision;
            peer.methods.clear();
        }

        let purchase = |peer: &mut Peer, request| peer.command(&script, "doCmdInt3", vec![
            Value::Int16(request), Value::Int16(205), Value::Int64(1), Value::Int64(0), Value::Int64(0),
        ]);
        purchase(&mut peer, 301);
        let first_attempt = grant(PlayerGrant::Vehicle { vehicle: tank });
        peer.until(&script, |p| p.responses.iter().any(|&(request, _)| request == 301));
        let (after, delivery) = match first_attempt {
            Ok(result) => result,
            Err(GrantRequestError::Busy) => grant(PlayerGrant::Vehicle { vehicle: tank }).unwrap(),
            Err(error) => panic!("Grant failed: {error}"),
        };
        let battle_vehicle = wot_accounts::test_support::garage_queries(catalog.clone())
            .battle_vehicle(&after, 2)
            .unwrap()
            .expect("Granted vehicle can enter a Training Battle");
        assert!(battle_vehicle.health > 0);
        let decoded = wot_accounts::test_support::decode_outfits(
            &[15], [battle_vehicle.outfit.as_slice()].into_iter(), None,
        ).unwrap();
        assert_eq!(decoded[&wot_accounts::test_support::OutfitSeason::Common].style, Some(986));
        let outfit_row = wot_platform::pickle!({15: (Pickle::Bytes(battle_vehicle.outfit))});
        let after = wot_accounts::test_support::account_snapshot(&after);
        if peer.responses.contains(&(301, -5)) {
            purchase(&mut peer, 302);
            peer.until(&script, |p| p.responses.contains(&(302, 0)));
        } else {
            assert!(peer.responses.contains(&(301, 0)));
        }
        assert_eq!(delivery, GrantDelivery::QueuedForGarage);
        peer.until(&script, |p| p.methods.iter().filter(|(name, _)| name.ends_with("update")).count() >= 2);
        let diff = ammunition_flow::update(&peer, after.revision);
        let Pickle::Dict(inventory) = dict(&diff, "inventory") else { panic!("Inventory delta") };
        let tanks = &inventory[&Pickle::from(1)];
        let built_in_style = |customization: &Pickle| {
            let Pickle::Dict(tables) = customization else { panic!("Customization tables") };
            let Pickle::Dict(outfits) = &tables[&Pickle::from(2)] else { panic!("Vehicle outfits") };
            outfits.get(&Pickle::from(tank)).cloned()
        };
        assert_eq!(built_in_style(&inventory[&Pickle::from(12)]), Some(outfit_row.clone()));
        assert!(matches!(dict(tanks, "compDescr"), Pickle::Dict(rows) if rows.contains_key(&Pickle::from(2))));
        assert!(matches!(dict(tanks, "shells"), Pickle::Dict(rows) if rows.contains_key(&Pickle::from(2))));
        assert!(matches!(dict(tanks, "crew"), Pickle::Dict(rows) if rows.contains_key(&Pickle::from(2))));
        assert!(matches!(grant(PlayerGrant::Vehicle { vehicle: tank }), Err(GrantRequestError::Rejected(_))));

        let persisted = runtime.block_on(accounts.snapshot(id)).unwrap();
        assert_eq!(persisted.revision, last_revision + 2);
        assert_eq!(persisted.balances.slots, 101);
        assert_eq!(persisted.balances.equipment_components, 31);
        assert_eq!(persisted.progression.depot.get(&book), Some(&5));
        assert_eq!(persisted.progression.depot.get(&equipment), Some(&1));
        assert_eq!(persisted.balances.credits, 100_000_031 - 250_000);
        assert_eq!(persisted.vehicles[1].catalog_key, tank_key);
        assert!(persisted.vehicles[1].has_crew);
        assert!(persisted.vehicles[1].ammunition.as_ref().unwrap().loaded.iter().any(|&(_, count)| count > 0));
        peer.disconnect();
        let mut peer = Peer::new(None);
        peer.connect(login, &username, "grants");
        peer.until(&script, |p| p.created && p.shown);
        let cache = sync(&mut peer, &script, 303);
        assert_eq!(dict(&cache, "rev"), Pickle::from(persisted.revision));
        assert_eq!(dict(&dict(&cache, "stats"), "freeXP"), Pickle::from(persisted.balances.free_xp));
        assert_eq!(dict(&dict(&cache, "stats"), "equipCoin"), Pickle::from(31));
        assert_eq!(depot_count(&cache, 14, book), 5);
        assert_eq!(depot_count(&cache, 9, equipment), 1);
        assert_eq!(depot_count(&cache, 9, bounty_basic), 2);
        assert_eq!(depot_count(&cache, 9, bounty_upgraded), 1);
        let Pickle::Dict(rows) = dict(&cache, "inventory") else { panic!("Garage inventory") };
        assert_eq!(built_in_style(&rows[&Pickle::from(12)]), Some(outfit_row));
        let Pickle::Dict(owned) = dict(&rows[&Pickle::from(1)], "compDescr") else { panic!("Owned tanks") };
        let Pickle::Dict(granted) = dict(tanks, "compDescr") else { panic!("Granted tank") };
        assert_eq!(owned.len(), 2);
        assert_eq!(owned[&Pickle::from(2)], granted[&Pickle::from(2)]);
    }).await;
    stop.store(true, Ordering::Relaxed);
    worker.await.unwrap().unwrap();
    sqlx::query("DELETE FROM accounts.accounts WHERE id = $1")
        .bind(id)
        .execute(&repository.connection_pool())
        .await
        .unwrap();
    test.unwrap();
    let spans = exporter.get_finished_spans().unwrap();
    assert_depot_grant_trace(id, book, &spans);
    let vehicle_plans: Vec<_> = spans
        .iter()
        .filter(|span| {
            span.name == "accounts.grants.plan_grant"
                && span.attributes.iter().any(|attr| {
                    attr.key.as_str() == "input.grant"
                        && attr.value.as_str() == format!("Vehicle {{ vehicle: {tank} }}")
                })
        })
        .collect();
    assert!(
        vehicle_plans
            .iter()
            .any(|span| span.status == opentelemetry::trace::Status::Unset)
    );
    assert!(
        vehicle_plans
            .iter()
            .any(|span| matches!(span.status, opentelemetry::trace::Status::Error { .. }))
    );
    for plan in vehicle_plans {
        let gateway = spans
            .iter()
            .find(|span| span.span_context.span_id() == plan.parent_span_id)
            .unwrap();
        assert_eq!(gateway.name, "player_sessions.gateway.grant");
        assert!(spans.iter().any(|span| {
            span.name == "player_sessions.gateway.grant_response"
                && span.parent_span_id == gateway.span_context.span_id()
        }));
    }
}

fn depot_count(cache: &Pickle, item_type: i32, id: i32) -> i64 {
    let Pickle::Dict(inventory) = dict(cache, "inventory") else {
        panic!("Missing garage inventory");
    };
    let Pickle::Dict(stock) = &inventory[&Pickle::from(item_type)] else {
        panic!("Missing depot item table");
    };
    let Pickle::I64(quantity) = stock[&Pickle::from(id)] else {
        panic!("Missing depot stock");
    };
    quantity
}

fn assert_depot_grant_trace(account: i64, item: i32, spans: &[opentelemetry_sdk::trace::SpanData]) {
    let has = |span: &opentelemetry_sdk::trace::SpanData, key: &str, expected: &str| {
        span.attributes.iter().any(|attribute| {
            attribute.key.as_str() == key && attribute.value.to_string() == expected
        })
    };
    for (count, success) in [("3", true), ("101", false)] {
        let (gateway, plan, depot) = spans
            .iter()
            .filter(|span| {
                span.name == "player_sessions.gateway.grant"
                    && has(span, "user.id", &account.to_string())
            })
            .find_map(|gateway| {
                let plan = spans.iter().find(|plan| {
                    plan.name == "accounts.grants.plan_grant"
                        && plan.parent_span_id == gateway.span_context.span_id()
                })?;
                let depot = spans.iter().find(|span| {
                    span.name == "accounts.grants.grant_depot_stock"
                        && span.parent_span_id == plan.span_context.span_id()
                        && has(span, "input.item", &item.to_string())
                        && has(span, "input.count", count)
                })?;
                Some((gateway, plan, depot))
            })
            .unwrap();
        let in_trace = |name: &'static str| {
            spans.iter().filter(move |span| {
                span.name == name && span.span_context.trace_id() == gateway.span_context.trace_id()
            })
        };
        let request = in_trace("player_sessions.grants.grant").next().unwrap();
        let reply = in_trace("player_sessions.gateway.grant_response")
            .next()
            .unwrap();
        assert_eq!(gateway.parent_span_id, request.span_context.span_id());
        assert_eq!(reply.parent_span_id, gateway.span_context.span_id());
        assert!(has(
            reply,
            "output.success",
            if success { "true" } else { "false" }
        ));

        if success {
            assert!(has(depot, "output.owned.count", "5"));
            assert!(
                plan.attributes
                    .iter()
                    .any(|attr| attr.key.as_str() == "output.revision")
            );
            assert!(in_trace("postgresql.query").any(|span| has(
                span,
                "db.operation.name",
                "UPDATE"
            )));
            let publish = in_trace("player_sessions.publish_account_change")
                .next()
                .unwrap();
            assert_eq!(publish.parent_span_id, reply.span_context.span_id());
            assert!(in_trace("mercury.response").any(|span| {
                span.parent_span_id == publish.span_context.span_id()
                    && has(span, "rpc.method", "update")
                    && has(span, "output.delivery", "sent")
            }));
        } else {
            assert!(matches!(
                depot.status,
                opentelemetry::trace::Status::Error { .. }
            ));
            assert_eq!(in_trace("accounts.postgres.save_progression").count(), 0);
        }
    }
}

fn sync(peer: &mut Peer, script: &ScriptDispatch, request: i16) -> Pickle {
    peer.command(
        script,
        "doCmdInt3",
        vec![
            Value::Int16(request),
            Value::Int16(100),
            Value::Int64(0),
            Value::Int64(0),
            Value::Int64(0),
        ],
    );
    peer.until(script, |p| p.has_resource(request as u16));
    peer.resource(request as u16)
}
