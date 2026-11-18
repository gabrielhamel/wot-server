use super::{
    client::GarageCatalog,
    postgres::{PostgresAccounts, read_snapshot},
};
use crate::accounts::{
    AccountSnapshot, PlayerAccount, PlayerPage, PlayerPagination, PlayerRepository, PlayerRole,
};
use anyhow::Result;
use sqlx::PgPool;
use std::sync::Arc;

#[derive(Clone)]
pub struct PostgresPlayers {
    pub(super) pool: PgPool,
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
    #[tracing::instrument(name = "accounts.players.list", skip_all, fields(ddd.bounded_context.name = "accounts", input.offset = pagination.offset(), input.limit = pagination.limit()), err)]
    async fn list(&self, pagination: PlayerPagination) -> Result<PlayerPage> {
        let mut tx =
            crate::platform::observability::database("accounts", "BEGIN", self.pool.begin())
                .await?;
        {
            let statement = "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY";
            crate::platform::observability::database(
                "accounts",
                statement,
                sqlx::query(statement).execute(&mut *tx),
            )
            .await
        }?;
        {
            let statement = "SET LOCAL statement_timeout = '5s'";
            crate::platform::observability::database(
                "accounts",
                statement,
                sqlx::query(statement).execute(&mut *tx),
            )
            .await
        }?;
        let total = {
            let statement = "SELECT count(*) FROM accounts";
            crate::platform::observability::database(
                "accounts",
                statement,
                sqlx::query_scalar(statement).fetch_one(&mut *tx),
            )
            .await
        }?;
        let rows: Vec<(i64, String, String, bool)> = {
            let statement = "SELECT id, username, display_name, role = 'admin' FROM accounts ORDER BY id DESC LIMIT $1 OFFSET $2";
            crate::platform::observability::database(
                "accounts",
                statement,
                sqlx::query_as(statement)
                    .bind(pagination.limit())
                    .bind(pagination.offset())
                    .fetch_all(&mut *tx),
            )
            .await
        }?;
        crate::platform::observability::database("accounts", "COMMIT", tx.commit()).await?;
        let accounts = rows
            .into_iter()
            .map(|(id, username, display_name, admin)| PlayerAccount {
                id,
                username,
                display_name,
                role: if admin {
                    PlayerRole::Admin
                } else {
                    PlayerRole::Player
                },
            })
            .collect();
        Ok(PlayerPage { accounts, total })
    }

    #[tracing::instrument(name = "accounts.players.provision", skip_all, fields(ddd.bounded_context.name = "accounts", input.username.bytes = username.len()), err)]
    async fn provision(
        &self,
        username: String,
        display_name: String,
        password_hash: String,
        role: PlayerRole,
    ) -> Result<Option<PlayerAccount>> {
        let mut tx =
            crate::platform::observability::database("accounts", "BEGIN", self.pool.begin())
                .await?;
        {
            let statement = "SET LOCAL statement_timeout = '5s'";
            crate::platform::observability::database(
                "accounts",
                statement,
                sqlx::query(statement).execute(&mut *tx),
            )
            .await
        }?;
        if role == PlayerRole::Admin {
            super::player_access::lock_accounts(&mut tx).await?;
        }
        let id = {
            let statement = "INSERT INTO accounts(username, display_name, password_hash, role) SELECT $1, $2, $3, $4 WHERE $4 = 'player' OR NOT EXISTS (SELECT 1 FROM accounts) ON CONFLICT(username) DO NOTHING RETURNING id";
            crate::platform::observability::database(
                "accounts",
                statement,
                sqlx::query_scalar(statement)
                    .bind(&username)
                    .bind(&display_name)
                    .bind(password_hash)
                    .bind(role.as_str())
                    .fetch_optional(&mut *tx),
            )
            .await
        }?;
        let Some(id) = id else {
            crate::platform::observability::database("accounts", "ROLLBACK", tx.rollback()).await?;
            return Ok(None);
        };

        let mut snapshot = read_snapshot(&mut tx, id).await?;
        let catalog = self.catalog.clone();
        let keys = self.starter_keys.clone();
        let snapshot = tokio::task::spawn_blocking(
            crate::platform::observability::in_current_span(move || {
                catalog.prepare_starter_garage(&mut snapshot, &keys)?;
                Ok::<_, anyhow::Error>(snapshot)
            }),
        )
        .await??;
        persist_starter_garage(&mut tx, &snapshot).await?;
        crate::platform::observability::database("accounts", "COMMIT", tx.commit()).await?;
        Ok(Some(PlayerAccount {
            id,
            username,
            display_name,
            role,
        }))
    }
}

#[tracing::instrument(name = "accounts.players.persist_starter_garage", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = next.id, input.vehicle_count = next.vehicles.len()), err)]
async fn persist_starter_garage(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    next: &AccountSnapshot,
) -> Result<()> {
    let id = next.id;
    {
        let statement = "UPDATE accounts SET revision=$2, settings=$3, progression=$4, crew_roster=$5, slots=$6 WHERE id=$1";
        crate::platform::observability::database(
            "accounts",
            statement,
            sqlx::query(statement)
                .bind(id)
                .bind(next.revision)
                .bind(sqlx::types::Json(&next.settings))
                .bind(sqlx::types::Json(&next.progression))
                .bind(sqlx::types::Json(&next.crew))
                .bind(next.balances.slots)
                .execute(&mut **tx),
        )
        .await
    }?;
    for owned in &next.vehicles {
        {
            let statement = "INSERT INTO owned_vehicles(account_id,inventory_id,catalog_key,ammunition,has_crew) VALUES($1,$2,$3,$4,$5)";
            crate::platform::observability::database(
                "accounts",
                statement,
                sqlx::query(statement)
                    .bind(id)
                    .bind(owned.inventory_id)
                    .bind(&owned.catalog_key)
                    .bind(owned.ammunition.as_ref().map(sqlx::types::Json))
                    .bind(owned.has_crew)
                    .execute(&mut **tx),
            )
            .await
        }?;
    }
    Ok(())
}
