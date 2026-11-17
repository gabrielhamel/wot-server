use anyhow::{Context, Result, ensure};
use sqlx::{ConnectOptions, PgPool, postgres::PgConnectOptions};
use std::{
    path::Path,
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};
use wot_server::accounts::{
    AccountRepository, Accounts, DEMO_PLAYERS, GarageCatalog, PostgresAccounts,
};
use wot_server::platform::client_formats::pickle::{self, Value};

#[tokio::test]
#[ignore = "Requires WG_TEST_DATABASE_URL with CREATE DATABASE permission and WG_RESOURCE_DIR"]
async fn standalone_seed_creates_ready_garages_and_preserves_existing_state() -> Result<()> {
    let url = std::env::var("WG_TEST_DATABASE_URL")?;
    let resources = std::env::var("WG_RESOURCE_DIR")?;
    let admin = PgPool::connect(&url).await?;
    let name = format!(
        "seed_test_{}_{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    );
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&admin)
        .await?;
    let options = PgConnectOptions::from_str(&url)?.database(&name);
    let result = verify_seed(options.to_url_lossy().as_str(), Path::new(&resources)).await;
    sqlx::query(&format!("DROP DATABASE {name} WITH (FORCE)"))
        .execute(&admin)
        .await?;
    result
}

fn run_command(url: &str, resources: &Path) -> Result<()> {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_seed"))
        .arg("--database-url")
        .arg(url)
        .arg("--resource-dir")
        .arg(resources)
        .output()?;
    ensure!(
        output.status.success(),
        "Seed failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

async fn verify_seed(url: &str, resources: &Path) -> Result<()> {
    run_command(url, resources)?;
    let catalog = GarageCatalog::load(wot_resources::ResourceBundle::open(resources)?, &[])?;
    let keys = catalog.tier_one_keys()?;
    ensure!(keys.len() == 11, "Expected 11 tier I tech tree tanks");
    let british: Vec<_> = keys.iter().filter(|key| key.starts_with("uk:")).collect();
    ensure!(
        british == ["uk:GB03_Cruiser_Mk_I"],
        "Only Cruiser I should be seeded for Britain"
    );
    let repository = PostgresAccounts::connect(url, 2).await?;
    let accounts = Accounts::new(repository.clone());
    for (username, display_name) in DEMO_PLAYERS {
        let snapshot = accounts
            .authenticate(username, "demo".into())
            .await?
            .context("Demo login failed")?;
        ensure!(snapshot.display_name == display_name);
        ensure!(
            accounts
                .authenticate(username, "wrong".into())
                .await?
                .is_none()
        );
        ensure!(snapshot.vehicles.len() == keys.len());
        ensure!(!snapshot.settings.is_empty());
        let roster = snapshot.crew.as_ref().context("Crew must be persisted")?;
        for owned in &snapshot.vehicles {
            ensure!(keys.contains(&owned.catalog_key));
            ensure!(owned.has_crew);
            ensure!(
                owned
                    .ammunition
                    .as_ref()
                    .is_some_and(|ammo| ammo.loaded.iter().any(|&(_, count)| count > 0))
            );
            let stock = snapshot
                .progression
                .installed
                .get(&owned.inventory_id)
                .context("Stock modules missing")?;
            ensure!(
                stock.len() == 6
                    && stock
                        .values()
                        .all(|item| snapshot.progression.unlocks.contains(item))
            );
            let seated: Vec<_> = roster
                .members
                .values()
                .filter(|member| {
                    member
                        .seat
                        .is_some_and(|seat| seat.vehicle == owned.inventory_id)
                })
                .collect();
            ensure!(!seated.is_empty());
            ensure!(
                seated.iter().all(|member| member.dismissed_at.is_none()
                    && member.training.efficiency_xp == 100_000)
            );
        }
        ensure!(snapshot.progression.unlocks.len() >= keys.len());
        let prepared = catalog.prepare(snapshot)?;
        let cache = pickle::from_reader(flate2::read::ZlibDecoder::new(prepared.cache.as_slice()))?;
        let inventory = entry(&cache, Value::from("inventory"))?;
        let vehicles = entry(inventory, Value::from(1))?;
        let crew = entry(vehicles, Value::from("crew"))?;
        let Value::Dict(crew) = crew else {
            anyhow::bail!("Missing projected crew table")
        };
        ensure!(crew.len() == keys.len());
        for seats in crew.values() {
            let Value::List(seats) = seats else {
                anyhow::bail!("Missing projected seats")
            };
            ensure!(
                !seats.is_empty() && seats.iter().all(|seat| *seat != Value::None),
                "Incomplete battle crew in projected garage"
            );
        }
    }
    let pool = repository.connection_pool();
    sqlx::query("UPDATE accounts SET credits=12345, settings=settings || '{\"500\":42}'::jsonb WHERE username=$1")
        .bind(DEMO_PLAYERS[0].0).execute(&pool).await?;
    let before: Vec<(i64, serde_json::Value)> =
        sqlx::query_as("SELECT id, to_jsonb(accounts) FROM accounts ORDER BY id")
            .fetch_all(&pool)
            .await?;
    run_command(url, resources)?;
    let after: Vec<(i64, serde_json::Value)> =
        sqlx::query_as("SELECT id, to_jsonb(accounts) FROM accounts ORDER BY id")
            .fetch_all(&pool)
            .await?;
    ensure!(
        before == after,
        "Repeat seed changed account state, credentials or revision"
    );

    let player = accounts
        .authenticate(DEMO_PLAYERS[0].0, "demo".into())
        .await?
        .context("Missing player")?;
    let first = player.vehicles.first().context("Missing tank")?;
    sqlx::query("DELETE FROM owned_vehicles WHERE account_id=$1 AND inventory_id=$2")
        .bind(player.id)
        .bind(first.inventory_id)
        .execute(&pool)
        .await?;
    sqlx::query("UPDATE accounts SET display_name='Collision' WHERE username=$1")
        .bind(DEMO_PLAYERS[1].0)
        .execute(&pool)
        .await?;
    ensure!(repository.seed_demo_players(&catalog).await.is_err());
    ensure!(
        repository.snapshot(player.id).await?.vehicles.len() == keys.len() - 1,
        "Failed batch should roll back first player's garage additions"
    );
    sqlx::query("UPDATE accounts SET display_name=$2 WHERE username=$1")
        .bind(DEMO_PLAYERS[1].0)
        .bind(DEMO_PLAYERS[1].1)
        .execute(&pool)
        .await?;
    run_command(url, resources)?;
    ensure!(repository.snapshot(player.id).await?.vehicles.len() == keys.len());
    sqlx::query("UPDATE accounts SET password_hash='invalid' WHERE username=$1")
        .bind(DEMO_PLAYERS[1].0)
        .execute(&pool)
        .await?;
    ensure!(
        repository.seed_demo_players(&catalog).await.is_err(),
        "Credential collision must fail"
    );
    let preserved: String =
        sqlx::query_scalar("SELECT password_hash FROM accounts WHERE username=$1")
            .bind(DEMO_PLAYERS[1].0)
            .fetch_one(&pool)
            .await?;
    ensure!(
        preserved == "invalid",
        "Existing credentials were overwritten"
    );
    pool.close().await;
    Ok(())
}

fn entry(value: &Value, key: Value) -> Result<&Value> {
    let Value::Dict(entries) = value else {
        anyhow::bail!("Expected a dictionary")
    };
    entries
        .get(&key)
        .context("Missing projected dictionary key")
}
