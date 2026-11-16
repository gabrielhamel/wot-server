use super::{
    client::GarageCatalog,
    postgres::{PostgresAccounts, read_snapshot},
    seeding::persist_starter_garage,
};
use crate::accounts::{PlayerAccount, PlayerPage, PlayerPagination, PlayerRepository};
use anyhow::Result;
use sqlx::PgPool;
use std::sync::Arc;

#[derive(Clone)]
pub struct PostgresPlayers {
    pool: PgPool,
    catalog: Arc<GarageCatalog>,
    starter_keys: Arc<[String]>,
}

impl PostgresPlayers {
    pub fn new(repository: &PostgresAccounts, catalog: Arc<GarageCatalog>) -> Result<Self> {
        let starter_keys = catalog.tier_one_keys()?.into();
        Ok(Self {
            pool: repository.connection_pool(),
            catalog,
            starter_keys,
        })
    }
}

impl PlayerRepository for PostgresPlayers {
    async fn list(&self, pagination: PlayerPagination) -> Result<PlayerPage> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SET LOCAL statement_timeout = '5s'")
            .execute(&mut *tx)
            .await?;
        let total = sqlx::query_scalar("SELECT count(*) FROM accounts")
            .fetch_one(&mut *tx)
            .await?;
        let rows: Vec<(i64, String, String)> = sqlx::query_as(
            "SELECT id, username, display_name FROM accounts ORDER BY id DESC LIMIT $1 OFFSET $2",
        )
        .bind(pagination.limit())
        .bind(pagination.offset())
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        let accounts = rows
            .into_iter()
            .map(|(id, username, display_name)| PlayerAccount {
                id,
                username,
                display_name,
            })
            .collect();
        Ok(PlayerPage { accounts, total })
    }

    async fn provision(
        &self,
        username: String,
        display_name: String,
        password_hash: String,
    ) -> Result<Option<PlayerAccount>> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET LOCAL statement_timeout = '5s'")
            .execute(&mut *tx)
            .await?;
        let id = sqlx::query_scalar(
            "INSERT INTO accounts(username, display_name, password_hash) VALUES($1, $2, $3) ON CONFLICT(username) DO NOTHING RETURNING id",
        )
        .bind(&username)
        .bind(&display_name)
        .bind(password_hash)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(id) = id else {
            return Ok(None);
        };

        let mut snapshot = read_snapshot(&mut tx, id).await?;
        let catalog = self.catalog.clone();
        let keys = self.starter_keys.clone();
        let snapshot = tokio::task::spawn_blocking(move || {
            catalog.seed_tier_one_garage(&mut snapshot, &keys)?;
            Ok::<_, anyhow::Error>(snapshot)
        })
        .await??;
        persist_starter_garage(&mut tx, &snapshot).await?;
        tx.commit().await?;
        Ok(Some(PlayerAccount {
            id,
            username,
            display_name,
        }))
    }
}
