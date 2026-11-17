use super::{Peer, Pickle, Value, ammunition_flow, dict};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use wot_server::client_gateway::mercury::app::{
    dispatch::ScriptDispatch, login::element::LoginResponse,
};
use wot_server::{
    accounts::{
        Accounts, GarageCatalog, GrantCurrency, GrantError, PlayerGrant, PostgresAccounts,
        ProgressionError,
    },
    client_gateway::{Gateway, GatewayConfig, GrantDelivery, GrantRequestError},
};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires WG_TEST_DATABASE_URL and WG_RESOURCE_DIR; uses temporary accounts in that database"]
async fn admin_grants_reach_live_garage_and_survive_reconnect() {
    let url = std::env::var("WG_TEST_DATABASE_URL").unwrap();
    let resources = wot_resources::ResourceBundle::open(std::path::Path::new(
        &std::env::var("WG_RESOURCE_DIR").unwrap(),
    ))
    .unwrap();
    let repository = PostgresAccounts::connect(&url, 4).await.unwrap();
    let accounts = Accounts::new(repository.clone());
    let username = format!(
        "grants_{}@example.test",
        std::time::UNIX_EPOCH.elapsed().unwrap().as_nanos()
    );
    let starter = "usa:A01_T1_Cunningham".to_owned();
    repository
        .seed(
            &username,
            "Grant test",
            "grants".into(),
            std::slice::from_ref(&starter),
        )
        .await
        .unwrap();
    let (id,): (i64,) = sqlx::query_as("SELECT id FROM accounts WHERE username = $1")
        .bind(&username)
        .fetch_one(&repository.connection_pool())
        .await
        .unwrap();
    let catalog = Arc::new(GarageCatalog::load(resources.clone(), &[starter]).unwrap());
    let snapshot = accounts.snapshot(id).await.unwrap();
    for key in [
        "germany:G90_DW_II_MapsTraining_Dummy_HT_1",
        "france:F28_105_leFH18B2_IGR",
        "germany:G98_Waffentrager_E100_WO",
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
    assert!(
        catalog
            .grantable_vehicles()
            .any(|tank| tank.key == "germany:G98_WT_auf_E_100_Phonix")
    );
    let tank = catalog
        .grantable_vehicles()
        .find(|tank| tank.key == "ussr:R111_ISU130")
        .unwrap()
        .id;
    let script =
        ScriptDispatch::new(wot_server::client_gateway::mercury::script::load(&resources).unwrap());
    let gateway = Gateway::bind(
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
        assert_eq!(delivery, GrantDelivery::OnNextSync);
        peer.handoff(ticket);
        peer.until(&script, |p| p.created && p.shown);
        let initial = sync(&mut peer, &script, 300);
        assert_eq!(dict(&initial, "rev"), Pickle::from(after.revision));
        assert_eq!(dict(&dict(&initial, "stats"), "crystal"), Pickle::from(after.balances.bonds));
        assert_eq!(dict(&dict(&initial, "stats"), "gold"), Pickle::from(after.balances.gold));

        let mut last_revision = after.revision;
        for (resource, key) in [(GrantCurrency::Credits, "credits"), (GrantCurrency::Gold, "gold"), (GrantCurrency::Bonds, "crystal"), (GrantCurrency::FreeXp, "freeXP")] {
            let (after, delivery) = grant(currency(resource, 31)).unwrap();
            assert_eq!(delivery, GrantDelivery::QueuedForGarage);
            peer.until(&script, |p| p.methods.iter().any(|(name, _)| name.ends_with("update")));
            let diff = ammunition_flow::update(&peer, after.revision);
            assert_eq!(dict(&diff, "prevRev"), Pickle::from(last_revision));
            assert_eq!(dict(&diff, "stats"), wot_server::pickle!({(key): (resource.balance(&after.balances))}));
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
        assert!(matches!(dict(tanks, "compDescr"), Pickle::Dict(rows) if rows.contains_key(&Pickle::from(2))));
        assert!(matches!(dict(tanks, "shells"), Pickle::Dict(rows) if rows.contains_key(&Pickle::from(2))));
        assert!(matches!(dict(tanks, "crew"), Pickle::Dict(rows) if rows.contains_key(&Pickle::from(2))));
        assert!(matches!(grant(PlayerGrant::Vehicle { vehicle: tank }), Err(GrantRequestError::Rejected(_))));

        let persisted = runtime.block_on(accounts.snapshot(id)).unwrap();
        assert_eq!(persisted.revision, last_revision + 2);
        assert_eq!(persisted.balances.slots, 101);
        assert_eq!(persisted.balances.credits, 100_000_031 - 250_000);
        assert_eq!(persisted.vehicles[1].catalog_key, "ussr:R111_ISU130");
        assert!(persisted.vehicles[1].has_crew);
        assert!(persisted.vehicles[1].ammunition.as_ref().unwrap().loaded.iter().any(|&(_, count)| count > 0));
        peer.disconnect();
        let mut peer = Peer::new(None);
        peer.connect(login, &username, "grants");
        peer.until(&script, |p| p.created && p.shown);
        let cache = sync(&mut peer, &script, 303);
        assert_eq!(dict(&cache, "rev"), Pickle::from(persisted.revision));
        assert_eq!(dict(&dict(&cache, "stats"), "freeXP"), Pickle::from(persisted.balances.free_xp));
        let Pickle::Dict(rows) = dict(&cache, "inventory") else { panic!("Garage inventory") };
        let Pickle::Dict(owned) = dict(&rows[&Pickle::from(1)], "compDescr") else { panic!("Owned tanks") };
        let Pickle::Dict(granted) = dict(tanks, "compDescr") else { panic!("Granted tank") };
        assert_eq!(owned.len(), 2);
        assert_eq!(owned[&Pickle::from(2)], granted[&Pickle::from(2)]);
    }).await;
    stop.store(true, Ordering::Relaxed);
    worker.await.unwrap().unwrap();
    sqlx::query("DELETE FROM accounts WHERE id = $1")
        .bind(id)
        .execute(&repository.connection_pool())
        .await
        .unwrap();
    test.unwrap();
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
