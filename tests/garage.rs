#[path = "support/ammunition_flow.rs"]
mod ammunition_flow;
#[path = "support/camouflage_flow.rs"]
mod camouflage_flow;
#[path = "support/consumables_flow.rs"]
mod consumables_flow;
#[path = "support/crew_flow.rs"]
mod crew_flow;
#[path = "support/depot_flow.rs"]
mod depot_flow;
#[path = "support/directives_flow.rs"]
mod directives_flow;
#[path = "support/equipment_flow.rs"]
mod equipment_flow;
#[path = "support/garage_peer.rs"]
mod garage_peer;
#[path = "support/grants_flow.rs"]
mod grants_flow;
#[path = "support/modules_flow.rs"]
mod modules_flow;
#[path = "support/vehicle_sales_flow.rs"]
mod vehicle_sales_flow;
#[path = "support/vehicles_flow.rs"]
mod vehicles_flow;
use garage_peer::Peer;
#[path = "support/player_fixture.rs"]
mod player_fixture;
#[path = "support/session_flow.rs"]
mod session_flow;
#[path = "support/social_flow.rs"]
mod social_flow;
#[path = "support/store_flow.rs"]
mod store_flow;
#[path = "support/telemetry_capture.rs"]
mod telemetry_capture;
#[path = "support/training_room_flow.rs"]
mod training_room_flow;
#[path = "support/web_flow.rs"]
mod web_flow;
#[path = "support/xmpp_flow.rs"]
mod xmpp_flow;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use wot_server::platform::client_formats::pickle::Value as Pickle;
use wot_server::player_sessions::test_support::mercury::app::{
    dispatch::ScriptDispatch,
    login::element::{LoginError, LoginResponse},
};
use wot_server::player_sessions::test_support::mercury::script::Value;
use wot_server::{
    accounts::test_support::{AccountRepository, Accounts, GarageCatalog, PostgresAccounts},
    player_sessions::{Gateway, GatewayConfig},
};

fn dict(value: &Pickle, name: &str) -> Pickle {
    let Pickle::Dict(dict) = value else {
        panic!("Not a dict")
    };
    dict[&Pickle::Bytes(name.as_bytes().to_vec())].clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires WG_TEST_DATABASE_URL and WG_RESOURCE_DIR; uses temporary accounts in that database"]
async fn postgres_to_encrypted_garage_with_isolated_clients() {
    exercise_garage(false, false).await;
    exercise_garage(true, true).await;
}

async fn exercise_garage(encrypted_login: bool, training_room_visible: bool) {
    telemetry_capture::exporter();
    let url = std::env::var("WG_TEST_DATABASE_URL").expect("WG_TEST_DATABASE_URL");
    let resources = wot_resources::ResourceBundle::open(std::path::Path::new(
        &std::env::var("WG_RESOURCE_DIR").expect("WG_RESOURCE_DIR"),
    ))
    .unwrap();
    let repository = PostgresAccounts::new(
        wot_server::server::test_support::connect_database(&url, 8)
            .await
            .unwrap(),
    );
    let prefix = format!(
        "test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let first = if encrypted_login {
        format!("{prefix}_a")
    } else {
        format!("{prefix}_a@example.test")
    };
    let second = format!("{prefix}_b@example.test");
    let has_exterior_styles = resources.has_file("scripts/item_defs/customization/styles/list.xml");
    let training_room_available =
        training_room_visible && resources.has_file("scripts/arena_defs/_list_.xml");
    let has_extension_styles =
        resources.has_file("last_stand/scripts/item_defs/customization/styles/list.xml");
    let has_inscriptions = has_extension_styles
        && resources.has_file("scripts/item_defs/customization/personal_numbers/list.xml");
    let has_effects = has_inscriptions
        && resources.has_file("scripts/item_defs/customization/modifications/list.xml");
    let has_decals = has_effects
        && resources.has_file("scripts/item_defs/customization/projection_decals/list.xml");
    let has_paints =
        has_decals && resources.has_file("scripts/item_defs/customization/paints/list.xml");
    let has_attachments = has_extension_styles
        && resources.has_file("scripts/item_defs/customization/attachments/list.xml");
    let has_stat_trackers = has_attachments
        && resources.has_file("scripts/item_defs/customization/stat_trackers/list.xml");
    let cosmetic_fixtures = camouflage_flow::CosmeticFixtures {
        has_exterior_styles,
        has_extension_styles,
        has_inscriptions,
        has_effects,
        has_decals,
        has_paints,
        has_attachments,
        has_stat_trackers,
    };
    let tank_a = "usa:A01_T1_Cunningham".to_string();
    let tank_b = "germany:G12_Ltraktor".to_string();
    player_fixture::insert_player(
        &repository,
        &first,
        "TestPlayerA",
        "password-a",
        std::slice::from_ref(&tank_a),
    )
    .await
    .unwrap();
    player_fixture::insert_player(
        &repository,
        &second,
        "TestPlayerB",
        "password-b",
        std::slice::from_ref(&tank_b),
    )
    .await
    .unwrap();
    let equipment_player = format!("{prefix}_equipment@example.test");
    player_fixture::insert_player(
        &repository,
        &equipment_player,
        "EquipmentPlayer",
        "password-equipment",
        &[
            "ussr:R04_T-34".into(),
            tank_a.clone(),
            "ussr:R111_ISU130".into(),
            "poland:Pl14_4TP".into(),
            "france:F01_RenaultFT".into(),
        ],
    )
    .await
    .unwrap();
    let accounts = Accounts::new(repository.clone());
    let equipment_account = accounts
        .authenticate(&equipment_player, "password-equipment".into())
        .await
        .unwrap()
        .unwrap()
        .id;
    assert!(
        accounts
            .authenticate(&first, "wrong-password".into())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        accounts
            .authenticate("missing-account", "password-a".into())
            .await
            .unwrap()
            .is_none()
    );
    let a = accounts
        .authenticate(&first, "password-a".into())
        .await
        .unwrap()
        .unwrap();
    let b = accounts
        .authenticate(&second, "password-b".into())
        .await
        .unwrap()
        .unwrap();
    assert_ne!(a.id, b.id);
    assert_eq!(a.vehicles[0].catalog_key, tank_a);
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    sqlx::query("UPDATE accounts.accounts SET progression = jsonb_set(progression, '{depot}', $2), equipment_components = 150, bonds = 10000 WHERE id=$1")
        .bind(equipment_account)
        .bind(sqlx::types::Json(equipment_flow::reward_stock()))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE accounts.accounts SET progression = jsonb_set(progression, '{unlocks}', to_jsonb(ARRAY[$2::integer])), free_xp = 1000000 WHERE id=$1")
        .bind(b.id)
        .bind(vehicles_flow::M14_41)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE accounts.accounts SET credits=1234 WHERE id=$1")
        .bind(a.id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE accounts.owned_vehicles SET inventory_id=42 WHERE account_id=$1")
        .bind(a.id)
        .execute(&pool)
        .await
        .unwrap();
    if has_extension_styles {
        sqlx::query("INSERT INTO accounts.owned_vehicles (account_id, inventory_id, catalog_key) VALUES ($1, 43, 'uk:GB158_Executor')")
            .bind(a.id)
            .execute(&pool)
            .await
            .unwrap();
    }
    assert!(
        repository
            .credentials(&first)
            .await
            .unwrap()
            .unwrap()
            .1
            .starts_with("$argon2id$")
    );
    let (r1, r2) = tokio::join!(
        accounts.save_settings(a.id, BTreeMap::from([(10, 20)])),
        accounts.save_settings(a.id, BTreeMap::from([(11, 21)]))
    );
    assert_ne!(r1.unwrap(), r2.unwrap());
    let stored = repository.snapshot(a.id).await.unwrap();
    let training_vehicle_id = stored.vehicles[0].inventory_id;
    assert_eq!(stored.revision, 3);
    assert_eq!(stored.settings, BTreeMap::from([(10, 20), (11, 21)]));
    assert!(repository.snapshot(b.id).await.unwrap().settings.is_empty());
    let reconnected = PostgresAccounts::new(
        wot_server::server::test_support::connect_database(&url, 2)
            .await
            .unwrap(),
    );
    assert_eq!(
        reconnected.snapshot(a.id).await.unwrap().settings,
        stored.settings
    );
    let catalog = Arc::new(GarageCatalog::load(resources.clone(), &[tank_a, tank_b]).unwrap());
    let has_camouflage = resources.has_file("scripts/item_defs/customization/camouflages/list.xml");
    let script = ScriptDispatch::new(
        wot_server::player_sessions::test_support::mercury::script::load(&resources).unwrap(),
    );
    let private_key = encrypted_login
        .then(|| Arc::new(rsa::RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048).unwrap()));
    let public_key = private_key
        .as_ref()
        .map(|key| Arc::new(rsa::RsaPublicKey::from(&**key)));
    let garage_capacity = if training_room_available { 3 } else { 2 };
    let gateway = Gateway::bind(
        &wot_server::server::test_support::composition(),
        GatewayConfig {
            login_bind: "127.0.0.1:0".parse().unwrap(),
            base_bind: "0.0.0.0:0".parse().unwrap(),
            advertised_base: encrypted_login.then(|| "198.51.100.1:30017".parse().unwrap()),
            private_key,
            max_clients: garage_capacity,
            max_pending: 4,
            training_room_visible,
            periphery_id: 250,
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
    let (login, base) = gateway.addresses().unwrap();
    let grants = gateway.player_grants();
    let runtime = tokio::runtime::Handle::current();
    let chat_address = std::net::SocketAddr::new("127.0.0.1".parse().unwrap(), base.port());
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let worker = tokio::task::spawn_blocking(move || gateway.run(worker_stop));
    let first_peer_name = first.clone();
    let result = tokio::task::spawn_blocking(move || {
        assert!(matches!(
            Peer::login(login, &first_peer_name, "wrong", public_key.clone()),
            LoginResponse::Error(LoginError::InvalidPassword, _)
        ));
        let mut first_peer = Peer::new(public_key.clone());
        first_peer.connect(login, &first_peer_name, "password-a");
        first_peer.until(&script, |p| p.created && p.shown);
        assert_eq!(
            first_peer.training_room_visible,
            Some(training_room_visible)
        );
        for (args, result) in [(&[2, 7, 0, 50, 0][..], -1), (&[2, 7, 0, 0, 50][..], 1)] {
            first_peer.command(
                &script,
                "doCmdIntArr",
                vec![
                    Value::Int16(200),
                    Value::Int16(520),
                    Value::Seq(args.iter().copied().map(Value::Int32).collect()),
                ],
            );
            first_peer.until(&script, |p| p.responses.contains(&(200, result)));
        }
        first_peer.until(&script, |p| p.has_resource(200));
        assert_eq!(first_peer.resource(200), wot_server::pickle!((2, 0, [])));
        training_room_flow::exercise(
            &mut first_peer,
            &script,
            a.id,
            training_vehicle_id,
            training_room_available,
        );
        let mut second_peer = Peer::new(public_key.clone());
        second_peer.connect(login, &second, "password-b");
        second_peer.until(&script, |p| p.created && p.shown);
        let mut invite_peer = if training_room_available {
            let mut peer = Peer::new(public_key.clone());
            peer.connect(login, &equipment_player, "password-equipment");
            peer.until(&script, |p| p.created && p.shown);
            training_room_flow::exercise_shared(
                &mut first_peer,
                &mut second_peer,
                &script,
                a.id,
                b.id,
                &mut peer,
                equipment_account,
            );
            Some(peer)
        } else {
            None
        };
        social_flow::exercise(&mut first_peer, &mut second_peer, &script, a.id, b.id);
        xmpp_flow::exercise(
            &mut first_peer,
            &mut second_peer,
            &script,
            chat_address,
            a.id,
            b.id,
        );
        assert!(matches!(
            Peer::login(login, &first_peer_name, "wrong", public_key.clone()),
            LoginResponse::Error(LoginError::InvalidPassword, _)
        ));
        first_peer.command(
            &script,
            "doCmdInt3",
            vec![
                Value::Int16(220),
                Value::Int16(100),
                Value::Int64(0),
                Value::Int64(0),
                Value::Int64(0),
            ],
        );
        first_peer.until(&script, |p| p.has_resource(220));
        let mut pending_peer = Peer::new(public_key.clone());
        let LoginResponse::Success(superseded) =
            pending_peer.authenticate(login, &first_peer_name, "password-a")
        else {
            panic!("The second login must succeed at capacity");
        };
        session_flow::assert_kicked(&mut first_peer, &script);
        let mut replacement = Peer::new(public_key.clone());
        replacement.connect(login, &first_peer_name, "password-a");
        replacement.until(&script, |p| p.created && p.shown);
        pending_peer.assert_handoff_rejected(&superseded);
        if let Some(peer) = &mut invite_peer {
            peer.disconnect();
        }
        let mut first_peer = replacement;
        if training_room_available {
            training_room_flow::assert_room_gone(&mut first_peer, &script, training_vehicle_id);
            training_room_flow::assert_room_gone(&mut second_peer, &script, training_vehicle_id);
            training_room_flow::assert_empty_list(&mut second_peer, &script);
        }
        let sync = |id, cmd| {
            vec![
                Value::Int16(id),
                Value::Int16(cmd),
                Value::Int64(0),
                Value::Int64(0),
                Value::Int64(0),
            ]
        };
        first_peer.command(&script, "doCmdInt3", sync(221, 100));
        second_peer.command(&script, "doCmdInt3", sync(221, 100));
        first_peer.until(&script, |p| p.has_resource(221));
        second_peer.until(&script, |p| p.has_resource(221));
        let first_cache = first_peer.resource(221);
        let second_cache = second_peer.resource(221);
        assert_eq!(dict(&first_cache, "rev"), Pickle::I64(3));
        assert_eq!(dict(&second_cache, "rev"), Pickle::I64(1));
        assert_eq!(
            dict(&dict(&first_cache, "stats"), "credits"),
            Pickle::I64(1234)
        );
        let Pickle::Dict(inventory) = dict(&first_cache, "inventory") else {
            panic!("inventory dict")
        };
        let Pickle::Dict(descriptors) = dict(&inventory[&Pickle::I64(1)], "compDescr") else {
            panic!("vehicle descriptors")
        };
        assert!(descriptors.contains_key(&Pickle::I64(42)));
        assert_ne!(
            dict(&first_cache, "inventory"),
            dict(&second_cache, "inventory")
        );
        first_peer.command(
            &script,
            "doCmdInt3",
            vec![
                Value::Int16(224),
                Value::Int16(100),
                Value::Int64(3),
                Value::Int64(0),
                Value::Int64(0),
            ],
        );
        first_peer.until(&script, |p| p.responses.contains(&(224, 0)));
        assert!(
            !first_peer.has_resource(224),
            "Current revision must finish sync, not start another full stream"
        );
        first_peer.command(
            &script,
            "doCmdIntArr",
            vec![
                Value::Int16(225),
                Value::Int16(1600),
                Value::Seq(vec![Value::Int32(12), Value::Int32(22)]),
            ],
        );
        first_peer.until(&script, |p| p.responses.contains(&(225, 0)));
        first_peer.command(&script, "doCmdInt3", sync(226, 100));
        first_peer.until(&script, |p| p.has_resource(226));
        assert_eq!(dict(&first_peer.resource(226), "rev"), Pickle::I64(4));
        first_peer.command(&script, "doCmdInt3", sync(222, 300));
        first_peer.command(&script, "doCmdInt3", sync(223, 600));
        first_peer.until(&script, |p| p.has_resource(222) && p.has_resource(223));
        let shop = first_peer.resource(222);
        assert_eq!(
            dict(&shop, "slotsPrices"),
            wot_server::pickle!((12, [("credits", 250000)]))
        );
        assert_eq!(first_peer.resource(223), wot_server::pickle!((1, [])));
        first_peer.command(&script, "doCmdInt3", sync(227, 12345));
        first_peer.until(&script, |p| p.responses.contains(&(227, -1)));
        let module_cache = modules_flow::exercise(&mut second_peer, &script);
        let module_cache = vehicles_flow::exercise(&mut second_peer, &script, &module_cache);
        let module_cache = crew_flow::exercise(&mut second_peer, &script, &module_cache);
        let module_cache = vehicles_flow::buy_slot(
            &mut first_peer,
            &mut second_peer,
            &script,
            &module_cache,
            &shop,
        );
        let module_cache =
            vehicles_flow::buy_with_consumables(&mut second_peer, &script, &module_cache);
        let mut last_cache = ammunition_flow::exercise(&mut first_peer, &script);
        if has_camouflage {
            last_cache =
                camouflage_flow::exercise(&mut first_peer, &script, &last_cache, cosmetic_fixtures);
        }
        if has_attachments {
            last_cache = camouflage_flow::exercise_attachments(
                &mut first_peer,
                &script,
                &last_cache,
                |grant| runtime.block_on(grants.grant(a.id, grant)),
            );
        }
        if has_stat_trackers {
            last_cache = camouflage_flow::exercise_stat_tracker(
                &mut first_peer,
                &script,
                &last_cache,
                |grant| runtime.block_on(grants.grant(a.id, grant)),
            );
        }
        xmpp_flow::exercise_game_disconnect(&mut first_peer, &script, chat_address, a.id);
        social_flow::wait_presence(&mut second_peer, &script, a.id, false);
        std::thread::sleep(std::time::Duration::from_millis(250));
        let mut reconnect = Peer::new(public_key.clone());
        reconnect.connect(login, &first_peer_name, "password-a");
        reconnect.until(&script, |p| p.created && p.shown);
        social_flow::wait_presence(&mut second_peer, &script, a.id, true);
        second_peer.disconnect();
        std::thread::sleep(std::time::Duration::from_millis(250));
        let mut module_reconnect = Peer::new(public_key.clone());
        module_reconnect.connect(login, &second, "password-b");
        module_reconnect.until(&script, |p| p.created && p.shown);
        module_reconnect.command(&script, "doCmdInt3", sync(415, 100));
        module_reconnect.until(&script, |p| p.has_resource(415));
        assert_eq!(module_reconnect.resource(415), module_cache);
        module_reconnect.disconnect();
        reconnect.command(&script, "doCmdInt3", sync(221, 100));
        reconnect.until(&script, |p| p.has_resource(221));
        assert_eq!(reconnect.resource(221), last_cache);
        reconnect.disconnect();
        std::thread::sleep(std::time::Duration::from_millis(250));
        let mut equipment_peer = Peer::new(public_key.clone());
        equipment_peer.connect(login, &equipment_player, "password-equipment");
        equipment_peer.until(&script, |p| p.created && p.shown);
        let equipment_cache = equipment_flow::exercise(&mut equipment_peer, &script);
        let depot_cache = depot_flow::exercise(&mut equipment_peer, &script, &equipment_cache);
        let directive_cache = directives_flow::exercise(&mut equipment_peer, &script, &depot_cache);
        let consumable_cache =
            consumables_flow::exercise(&mut equipment_peer, &script, &directive_cache);
        let consumable_cache =
            crew_flow::accelerated_training(&mut equipment_peer, &script, &consumable_cache);
        let sold_cache =
            vehicle_sales_flow::exercise(&mut equipment_peer, &script, &consumable_cache);
        let special_cache =
            equipment_flow::exercise_special_equipment(&mut equipment_peer, &script, &sold_cache);
        let disassembled =
            equipment_flow::exercise_disassembly(&mut equipment_peer, &script, &special_cache);
        let bounty =
            equipment_flow::exercise_bounty_upgrade(&mut equipment_peer, &script, &disassembled);
        let final_cache =
            equipment_flow::exercise_experimental_upgrade(&mut equipment_peer, &script, &bounty);
        equipment_peer.disconnect();
        std::thread::sleep(std::time::Duration::from_millis(250));
        let mut equipment_reconnect = Peer::new(public_key);
        equipment_reconnect.connect(login, &equipment_player, "password-equipment");
        equipment_reconnect.until(&script, |p| p.created && p.shown);
        assert_eq!(
            equipment_flow::full_cache(&mut equipment_reconnect, &script),
            final_cache
        );
        equipment_reconnect.disconnect();
    })
    .await;
    stop.store(true, Ordering::Relaxed);
    worker.await.unwrap().unwrap();
    if result.is_ok() {
        telemetry_capture::assert_player_spans(a.id, &first, "TestPlayerA");
        telemetry_capture::assert_training_room_bootstrap(training_room_visible);
        training_room_flow::assert_traces(a.id, b.id, training_room_available);
        telemetry_capture::assert_operation_traces(a.id, b.id, garage_capacity);
        telemetry_capture::assert_equipment_traces(equipment_account);
        if has_camouflage {
            camouflage_flow::assert_traces(a.id, cosmetic_fixtures);
        }
    }
    let fresh_pool = wot_server::server::test_support::connect_database(&url, 2)
        .await
        .unwrap();
    let fresh_social = wot_server::social::postgres::service(
        &wot_server::server::test_support::composition(),
        fresh_pool.clone(),
        wot_server::accounts::test_support::directory(fresh_pool),
    );
    if result.is_ok() {
        equipment_flow::verify_unaffordable_upgrade(
            &catalog,
            repository.snapshot(equipment_account).await.unwrap(),
        );
        vehicle_sales_flow::verify_storage(&repository, &catalog, equipment_account).await;
        let stored = repository.snapshot(a.id).await.unwrap();
        assert_eq!(
            stored.revision,
            if has_camouflage {
                11 + i64::from(has_extension_styles)
                    + 5 * i64::from(has_inscriptions)
                    + 3 * i64::from(has_effects)
                    + 4 * i64::from(has_decals)
                    + 3 * i64::from(has_paints)
                    + 4 * i64::from(has_attachments)
                    + 4 * i64::from(has_stat_trackers)
            } else {
                7
            }
        );
        assert_eq!(
            PostgresAccounts::new(
                wot_server::server::test_support::connect_database(&url, 2)
                    .await
                    .unwrap()
            )
            .snapshot(a.id)
            .await
            .unwrap()
            .vehicles,
            stored.vehicles
        );
        let modules = repository.snapshot(b.id).await.unwrap();
        assert_eq!(modules.revision, 37);
        assert_eq!(modules.balances.slots, 101);
        assert_eq!(modules.vehicles.len(), 3);
        assert_eq!(modules.vehicles[1].catalog_key, "germany:G06_PzII");
        assert!(modules.vehicles[1].has_crew);
        assert_eq!(modules.vehicles[2].catalog_key, "italy:It06_M14_41");
        assert_eq!(
            modules.progression.vehicle_consumables(3).layout,
            [vehicles_flow::SMALL_REPAIR_KIT, 0, 0],
        );
        let reloaded_catalog =
            GarageCatalog::load(resources.clone(), &["germany:G12_Ltraktor".into()]).unwrap();
        assert_eq!(
            reloaded_catalog.prepare(modules.clone()).unwrap().cache,
            catalog.prepare(modules.clone()).unwrap().cache
        );
        assert!(modules.progression.unlocks.contains(&515604));
        assert_eq!(
            modules.progression.installed[&1][&wot_server::accounts::test_support::ModuleKind::Gun],
            515604
        );
        assert!(
            modules.vehicles[0]
                .ammunition
                .as_ref()
                .unwrap()
                .loaded
                .iter()
                .all(|&(_, n)| n == 0)
        );
        let mut full_barracks = modules.clone();
        full_barracks.balances.berths = 0;
        assert!(matches!(
            catalog.plan_progression(
                full_barracks,
                wot_server::accounts::test_support::ProgressionCommand::Crew(
                    wot_server::accounts::test_support::CrewCommand::Hire {
                        vehicle: 3089,
                        role: 1,
                        course: wot_server::accounts::test_support::CrewRecruitment::Gold,
                    },
                ),
            ),
            Err(wot_server::accounts::test_support::ProgressionError::NoFreeBerth)
        ));
        use wot_server::accounts::test_support::{ProgressionCommand, ProgressionError};
        let mut free_slot = modules.clone();
        free_slot.balances.slots = 11;
        free_slot.balances.credits = 0;
        let free_slot = catalog
            .plan_progression(free_slot, ProgressionCommand::BuyVehicleSlot)
            .unwrap()
            .snapshot;
        assert_eq!(free_slot.balances.slots, 12);
        assert_eq!(free_slot.balances.credits, 0);
        for slots in [1024, i64::MAX] {
            let mut full_garage = modules.clone();
            full_garage.balances.slots = slots;
            assert!(matches!(
                catalog.plan_progression(full_garage, ProgressionCommand::BuyVehicleSlot),
                Err(ProgressionError::VehicleSlotLimit)
            ));
        }
        let module_change = catalog
            .plan_progression(
                modules,
                wot_server::accounts::test_support::ProgressionCommand::Buy {
                    item: 515604,
                    count: 1,
                },
            )
            .unwrap();
        let (first_commit, second_commit) = tokio::join!(
            repository.save_progression(&module_change),
            repository.save_progression(&module_change)
        );
        assert_ne!(first_commit.unwrap(), second_commit.unwrap());
        let fresh = PostgresAccounts::new(
            wot_server::server::test_support::connect_database(&url, 2)
                .await
                .unwrap(),
        )
        .snapshot(b.id)
        .await
        .unwrap();
        assert_eq!(fresh.progression, module_change.snapshot.progression);
        assert_eq!(fresh.balances, module_change.snapshot.balances);
        // Simulate an ownership insert conflict after the account debit.
        let mut conflicting_source = fresh.clone();
        conflicting_source
            .vehicles
            .retain(|v| v.catalog_key != "germany:G06_PzII");
        conflicting_source
            .crew
            .as_mut()
            .unwrap()
            .members
            .retain(|_, member| member.seat.is_none_or(|seat| seat.vehicle != 2));
        let conflicting_purchase = catalog
            .plan_progression(
                conflicting_source.clone(),
                wot_server::accounts::test_support::ProgressionCommand::BuyVehicle {
                    vehicle: 2065,
                    options: wot_server::accounts::test_support::VehiclePurchaseOptions {
                        crew: None,
                        ammunition: false,
                    },
                    consumables: Vec::new(),
                },
            )
            .unwrap();
        let diff = catalog
            .progression_diff(&conflicting_source, &conflicting_purchase)
            .unwrap();
        let Pickle::Dict(inventory) = dict(&diff, "inventory") else {
            panic!("Expected purchased vehicle inventory");
        };
        let Pickle::Dict(crew) = dict(&inventory[&Pickle::from(1)], "crew") else {
            panic!("Expected purchased vehicle crew slots");
        };
        assert_eq!(
            crew[&Pickle::from(conflicting_purchase.acquired_vehicle.unwrap())],
            wot_server::pickle!([None, None, None])
        );
        assert!(
            repository
                .save_progression(&conflicting_purchase)
                .await
                .is_err()
        );
        let unchanged = repository.snapshot(b.id).await.unwrap();
        assert_eq!(unchanged.balances, fresh.balances);
        assert_eq!(unchanged.revision, fresh.revision);
        assert_eq!(unchanged.vehicles, fresh.vehicles);

        let lost_ownership = catalog
            .plan_progression(
                fresh.clone(),
                wot_server::accounts::test_support::ProgressionCommand::Buy {
                    item: 515604,
                    count: 1,
                },
            )
            .unwrap();
        sqlx::query("DELETE FROM accounts.owned_vehicles WHERE account_id=$1")
            .bind(b.id)
            .execute(&pool)
            .await
            .unwrap();
        assert!(repository.save_progression(&lost_ownership).await.is_err());
        let rejected = repository.snapshot(b.id).await.unwrap();
        assert_eq!(rejected.revision, fresh.revision);
        assert_eq!(rejected.progression, fresh.progression);
        assert_eq!(rejected.balances, fresh.balances);

        let ammunition = stored.vehicles[0].ammunition.as_ref().unwrap();
        let mut desired: Vec<_> = ammunition
            .loaded
            .iter()
            .map(
                |&(id, count)| wot_server::accounts::test_support::ShellSelection {
                    id,
                    count,
                    alternate_price: false,
                },
            )
            .collect();
        desired
            .iter_mut()
            .find(|shell| shell.count > 0)
            .unwrap()
            .count -= 1;
        let change = catalog.plan_ammunition(stored, 42, &desired).unwrap();
        let (first_commit, repeated_commit) = tokio::join!(
            repository.save_ammunition(&change),
            repository.save_ammunition(&change),
        );
        assert_ne!(first_commit.unwrap(), repeated_commit.unwrap());
        let committed = repository.snapshot(a.id).await.unwrap();
        assert_eq!(committed.revision, change.previous_revision + 1);
        assert_eq!(committed.vehicles, change.snapshot.vehicles);
        assert_eq!(committed.depot_shells, change.snapshot.depot_shells);
        assert_eq!(committed.balances, change.snapshot.balances);

        // A lost ownership row must roll back the preceding account update.
        desired
            .iter_mut()
            .find(|shell| shell.count > 0)
            .unwrap()
            .count -= 1;
        let orphaned_change = catalog
            .plan_ammunition(committed.clone(), 42, &desired)
            .unwrap();
        sqlx::query("DELETE FROM accounts.owned_vehicles WHERE account_id=$1")
            .bind(a.id)
            .execute(&pool)
            .await
            .unwrap();
        assert!(repository.save_ammunition(&orphaned_change).await.is_err());
        let after_rejection = repository.snapshot(a.id).await.unwrap();
        assert_eq!(after_rejection.revision, committed.revision);
        assert_eq!(after_rejection.balances, committed.balances);
        assert_eq!(after_rejection.depot_shells, committed.depot_shells);

        assert!(fresh_social.roster(a.id).await.unwrap().is_empty());
        let contacts = fresh_social.roster(b.id).await.unwrap();
        assert_eq!(contacts.len(), 1);
        assert_eq!(contacts[0].player.id, a.id);
        assert!(!contacts[0].ignored);
        let conversations = fresh_social.restore(a.id).await.unwrap();
        assert_eq!(conversations.len(), 1);
        assert_eq!(conversations[0].peer(a.id).unwrap().id, b.id);
        assert_eq!(conversations[0].history.len(), 3);
        assert_eq!(conversations[0].history[0].sender.id, a.id);
        assert_eq!(conversations[0].history[1].text, "Online reply");
        assert_eq!(conversations[0].history[2].sender.id, b.id);
        assert_eq!(conversations[0].history[2].text, "Offline reply");
        assert!(
            fresh_social
                .send(a.id, conversations[0].id, " ")
                .await
                .is_err()
        );
        assert!(
            fresh_social
                .send(a.id, conversations[0].id, &"x".repeat(2049))
                .await
                .is_err()
        );
        for index in 0..105 {
            fresh_social
                .send(a.id, conversations[0].id, &format!("retained-{index}"))
                .await
                .unwrap();
        }
        let stored = fresh_social.restore(b.id).await.unwrap();
        assert_eq!(stored[0].history.len(), 100);
        assert_eq!(stored[0].history[0].text, "retained-5");
        fresh_social
            .change(
                b.id,
                a.id,
                wot_server::social::contract::ContactChange::Ignore,
            )
            .await
            .unwrap();
        assert!(
            fresh_social
                .send(a.id, conversations[0].id, "Must not persist")
                .await
                .is_err()
        );
        assert!(fresh_social.restore(b.id).await.unwrap().is_empty());
        fresh_social
            .change(
                b.id,
                a.id,
                wot_server::social::contract::ContactChange::RemoveIgnored,
            )
            .await
            .unwrap();
        assert_eq!(
            fresh_social.restore(b.id).await.unwrap()[0]
                .history
                .last()
                .unwrap()
                .text,
            "retained-104"
        );
    }
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    sqlx::query("DELETE FROM accounts.accounts WHERE id = ANY($1)")
        .bind(vec![a.id, b.id, equipment_account])
        .execute(&pool)
        .await
        .unwrap();
    result.unwrap();
}

#[tokio::test]
#[ignore = "Requires WG_TEST_DATABASE_URL; closes an isolated pool to verify database error telemetry"]
async fn database_failure_records_sql_and_error_status() {
    telemetry_capture::exporter();
    telemetry_capture::assert_database_failure(&std::env::var("WG_TEST_DATABASE_URL").unwrap())
        .await;
}
