use super::{Peer, Pickle, Value, dict, equipment_flow, player_fixture, telemetry_capture};
use opentelemetry_sdk::trace::SpanData;
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use wot_server::{
    accounts::{Accounts, GarageCatalog, PostgresAccounts},
    player_sessions::{
        Gateway, GatewayConfig,
        mercury::{app::dispatch::ScriptDispatch, script::StringValue},
    },
};

pub fn assert_kicked(peer: &mut Peer, script: &ScriptDispatch) {
    peer.until(script, |p| {
        p.methods
            .iter()
            .any(|(name, _)| name.ends_with("onKickedFromServer"))
    });
    let (_, args) = peer
        .methods
        .iter()
        .find(|(name, _)| name.ends_with("onKickedFromServer"))
        .unwrap();
    assert_eq!(
        *args,
        vec![
            Value::String(StringValue::String(
                "#system_messages:login_to_other_game_wot".into()
            )),
            Value::UInt8(1),
            Value::UInt32(0),
        ]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires WG_TEST_DATABASE_URL and WG_RESOURCE_DIR"]
async fn replacement_waits_for_account_write_and_loads_its_revision() {
    let spans = telemetry_capture::exporter();
    let url = std::env::var("WG_TEST_DATABASE_URL").unwrap();
    let directory = std::env::var("WG_RESOURCE_DIR").unwrap();
    let resources = wot_resources::ResourceBundle::open(Path::new(&directory)).unwrap();
    let repository =
        PostgresAccounts::new(wot_server::server::connect_database(&url, 6).await.unwrap());
    let pool = repository.connection_pool();
    let username = format!(
        "takeover_{}",
        std::time::UNIX_EPOCH.elapsed().unwrap().as_nanos()
    );
    let starter = ["usa:A01_T1_Cunningham".to_owned()];
    let account_id = player_fixture::insert_player(
        &repository,
        &username,
        "Replacement test",
        "takeover",
        &starter,
    )
    .await
    .unwrap();
    let catalog = Arc::new(GarageCatalog::load(resources.clone(), &starter).unwrap());
    let script = ScriptDispatch::new(
        wot_server::player_sessions::mercury::script::load(&resources).unwrap(),
    );
    let gateway = Gateway::bind(
        GatewayConfig {
            training_room_visible: true,
            login_bind: "127.0.0.1:0".parse().unwrap(),
            base_bind: "127.0.0.1:0".parse().unwrap(),
            advertised_base: None,
            private_key: None,
            max_clients: 1,
            max_pending: 4,
        },
        &resources,
        catalog.clone(),
        Accounts::new(repository.clone()).into(),
        wot_server::accounts::GarageWrites::new(Accounts::new(repository.clone()), catalog.clone()),
        wot_server::social::postgres::service(
            repository.connection_pool(),
            wot_server::accounts::postgres::directory(repository.connection_pool()),
        ),
        tokio::runtime::Handle::current(),
    )
    .unwrap();
    let (login, _) = gateway.addresses().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let worker = tokio::task::spawn_blocking(move || gateway.run(worker_stop));

    let mut lock = pool.begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM accounts.accounts WHERE id = $1 FOR UPDATE")
        .bind(account_id)
        .execute(&mut *lock)
        .await
        .unwrap();
    let (sent, requested) = tokio::sync::oneshot::channel();
    let (start, proceed) = std::sync::mpsc::channel();
    let exercise = tokio::task::spawn_blocking(move || {
        let mut original = Peer::new(None);
        original.connect(login, &username, "takeover");
        original.until(&script, |p| p.created && p.shown);
        original.command(
            &script,
            "doCmdIntArr",
            vec![
                Value::Int16(3200),
                Value::Int16(1600),
                Value::Seq(vec![Value::Int32(12), Value::Int32(22)]),
            ],
        );
        sent.send(()).unwrap();
        proceed.recv_timeout(Duration::from_secs(10)).unwrap();
        let mut replacement = Peer::new(None);
        replacement.connect(login, &username, "takeover");
        replacement.until(&script, |p| p.created && p.shown);
        let state = equipment_flow::full_cache(&mut replacement, &script);
        assert_eq!(
            dict(&state, "intUserSettings"),
            wot_server::pickle!({12: 22})
        );
        assert_eq!(dict(&state, "rev"), Pickle::I64(2));
        assert_kicked(&mut original, &script);
        replacement.disconnect();
    });

    let wait_pool = pool.clone();
    let wait_result = async move {
        requested.await.unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let blocking_status =
            "SELECT EXISTS (SELECT 1 FROM pg_stat_activity WHERE $1 = ANY(pg_blocking_pids(pid)))";
        while !sqlx::query_scalar::<_, bool>(blocking_status)
            .bind(pid)
            .fetch_one(&wait_pool)
            .await
            .unwrap()
        {
            assert!(
                Instant::now() < deadline,
                "Garage write never waited on the account lock"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        start.send(()).unwrap();
        while !spans
            .get_finished_spans()
            .unwrap()
            .iter()
            .any(|span| is_deferred(span, account_id))
        {
            assert!(
                Instant::now() < deadline,
                "Replacement did not wait for the account write"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    let waited = tokio::spawn(wait_result).await;
    lock.commit().await.unwrap();
    let result = exercise.await;
    stop.store(true, Ordering::Relaxed);
    worker.await.unwrap().unwrap();
    sqlx::query("DELETE FROM accounts.accounts WHERE id=$1")
        .bind(account_id)
        .execute(&pool)
        .await
        .unwrap();
    waited.unwrap();
    result.unwrap();

    let spans = spans.get_finished_spans().unwrap();
    let find_child = |parent: &SpanData, name| {
        spans
            .iter()
            .find(|span| {
                span.name == name
                    && span.span_context.trace_id() == parent.span_context.trace_id()
                    && span.parent_span_id == parent.span_context.span_id()
            })
            .unwrap_or_else(|| panic!("Missing {name} under {}", parent.name))
    };
    let wait = spans
        .iter()
        .find(|span| is_deferred(span, account_id))
        .unwrap();
    let login = spans
        .iter()
        .find(|span| {
            span.name == "game.authenticate" && span.span_context.span_id() == wait.parent_span_id
        })
        .expect("Missing authentication parent");
    let admit = spans
        .iter()
        .find(|span| {
            span.name == "player_sessions.admit"
                && span.parent_span_id == login.span_context.span_id()
                && has(span, "output.outcome", "replaced")
        })
        .expect("Missing replacement admission");
    assert!(has(admit, "input.revision", "2"));
    let replaced = find_child(admit, "player_sessions.replace_session");
    let kick = find_child(replaced, "mercury.response");
    assert!(has(kick, "rpc.method", "onKickedFromServer") && has(kick, "output.delivery", "sent"));
    let reply = find_child(admit, "mercury.login.response");
    assert!(has(reply, "output.response_code", "1") && has(reply, "output.delivery", "sent"));
}

fn has(span: &SpanData, key: &str, value: &str) -> bool {
    span.attributes
        .iter()
        .any(|attr| attr.key.as_str() == key && attr.value.to_string() == value)
}

fn is_deferred(span: &SpanData, account_id: i64) -> bool {
    span.name == "player_sessions.admit"
        && has(span, "user.id", &account_id.to_string())
        && has(span, "output.outcome", "waiting_for_write")
}
