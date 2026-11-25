use anyhow::{Context, Result};
use sqlx::{
    PgConnection, PgPool,
    migrate::{Migrate, Migrator},
    postgres::PgPoolOptions,
};
use std::time::Duration;

use wot_platform::observability::database;

#[tracing::instrument(name = "server.connect_database", skip_all, fields(ddd.bounded_context.name = "server", input.pool.max_connections = connections), err)]
pub async fn connect_database(url: &str, connections: u32) -> Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(connections)
        .acquire_timeout(Duration::from_secs(5))
        .connect(url)
        .await
        .context("Connect to PostgreSQL")?;
    migrate(&pool).await?;
    Ok(pool)
}

#[tracing::instrument(name = "server.migrate_database", skip_all, fields(ddd.bounded_context.name = "server"), err)]
async fn migrate(pool: &PgPool) -> Result<()> {
    let mut conn = pool.acquire().await?;
    // A failed or cancelled migration may leave session settings or a SQLx lock.
    conn.close_on_drop();
    // SQLx's PostgreSQL lock is reentrant on this connection. Hold it across
    // both schema initializations.
    conn.lock().await?;
    for statement in [
        "CREATE SCHEMA IF NOT EXISTS accounts",
        "CREATE SCHEMA IF NOT EXISTS social",
    ] {
        database(
            "server",
            statement,
            sqlx::query(statement).execute(&mut *conn),
        )
        .await?;
    }
    migrate_context(&mut conn, "accounts", wot_accounts::postgres::migrations()).await?;
    migrate_context(&mut conn, "social", wot_social::postgres::migrations()).await?;
    conn.unlock().await?;
    Ok(())
}

#[tracing::instrument(name = "server.migrate_context", skip_all, fields(ddd.bounded_context.name = "server", input.schema = schema), err)]
async fn migrate_context(
    conn: &mut PgConnection,
    schema: &'static str,
    migrator: &Migrator,
) -> Result<()> {
    let statement = "SELECT pg_catalog.set_config('search_path', $1, false)";
    database(
        "server",
        statement,
        sqlx::query(statement).bind(schema).execute(&mut *conn),
    )
    .await?;
    migrator
        .run_direct(conn)
        .await
        .with_context(|| format!("Migrate {schema} schema"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::{ConnectOptions, postgres::PgConnectOptions};
    use wot_social::contract::ContactChange;

    #[tokio::test]
    #[ignore = "Requires WOT_TEST_DATABASE_URL with permission to create a test database"]
    async fn initializes_context_storage() {
        let url = std::env::var("WOT_TEST_DATABASE_URL").unwrap();
        let pool = PgPool::connect(&url).await.unwrap();
        let name = format!(
            "context_storage_{}",
            std::time::UNIX_EPOCH.elapsed().unwrap().as_nanos()
        );
        sqlx::query(&format!("CREATE DATABASE {name}"))
            .execute(&pool)
            .await
            .unwrap();
        let options = url.parse::<PgConnectOptions>().unwrap().database(&name);
        let database_url = options.to_url_lossy().to_string();
        let initialized = connect_database(&database_url, 1).await.unwrap();
        let ids: Vec<i64> = sqlx::query_scalar("INSERT INTO accounts.accounts(username, display_name, password_hash) VALUES ('alice', 'Alice', 'fixture'), ('bob', 'Bob', 'fixture') RETURNING id")
            .fetch_all(&initialized).await.unwrap();
        let [alice, bob] = ids.as_slice() else {
            panic!("Expected two accounts")
        };
        let social = wot_social::postgres::service(
            initialized.clone(),
            wot_accounts::postgres::directory(initialized.clone()),
        );
        social
            .change(*alice, *bob, ContactChange::Friend)
            .await
            .unwrap();
        let room = social.open(*alice, *bob).await.unwrap();
        let sent = social
            .send(*alice, room.id, "Saved before restart")
            .await
            .unwrap();
        let message = sent.history.last().unwrap().id;
        initialized.close().await;

        let restarted = connect_database(&database_url, 1).await.unwrap();
        let social = wot_social::postgres::service(
            restarted.clone(),
            wot_accounts::postgres::directory(restarted.clone()),
        );
        let roster = social.roster(*alice).await.unwrap();
        assert_eq!(roster[0].player.name, "Bob");
        let rooms = social.restore(*bob).await.unwrap();
        assert_eq!(rooms[0].id, room.id);
        assert_eq!(rooms[0].history[0].id, message);
        assert_eq!(rooms[0].history[0].text, "Saved before restart");
        assert!(
            social
                .change(*alice, i64::MAX, ContactChange::Friend)
                .await
                .is_err()
        );
        assert!(social.open(*alice, i64::MAX).await.is_err());

        let cross_context_keys: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_constraint c JOIN pg_class src ON src.oid=c.conrelid JOIN pg_class dst ON dst.oid=c.confrelid WHERE c.contype='f' AND src.relnamespace IN ('accounts'::regnamespace, 'social'::regnamespace) AND src.relnamespace<>dst.relnamespace",
        ).fetch_one(&restarted).await.unwrap();
        assert_eq!(cross_context_keys, 0);
        let legacy_objects_absent: bool =
            sqlx::query_scalar("SELECT to_regclass('public._sqlx_migrations') IS NULL AND to_regclass('public.account_directory') IS NULL")
                .fetch_one(&restarted)
                .await
                .unwrap();
        assert!(legacy_objects_absent);
        let histories: (i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM accounts._sqlx_migrations), (SELECT count(*) FROM social._sqlx_migrations)").fetch_one(&restarted).await.unwrap();
        assert_eq!(histories, (1, 1));
        restarted.close().await;
        sqlx::query(&format!("DROP DATABASE {name}"))
            .execute(&pool)
            .await
            .unwrap();
    }
}
