use super::{
    client::GarageCatalog,
    postgres::{PostgresAccounts, read_snapshot},
};
use crate::accounts::{
    AccountSnapshot, CreatePlayerError, PlayerAccount, PlayerPage, PlayerPagination,
    PlayerProvision, PlayerRepository, PlayerRole,
};
use crate::platform::observability::database;
use anyhow::Result;
use sqlx::{PgPool, Postgres, Transaction};
use std::sync::Arc;

const STARTING_CREDITS: i64 = 200_000;
const STARTING_GOLD: i64 = 500;

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

#[async_trait::async_trait]
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
            let statement = "SELECT count(*) FROM accounts.accounts";
            crate::platform::observability::database(
                "accounts",
                statement,
                sqlx::query_scalar(statement).fetch_one(&mut *tx),
            )
            .await
        }?;
        let rows: Vec<(i64, String, String, bool)> = {
            let statement = "SELECT id, username, display_name, role = 'admin' FROM accounts.accounts ORDER BY id DESC LIMIT $1 OFFSET $2";
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

    #[tracing::instrument(name = "accounts.registration.status", skip_all, fields(ddd.bounded_context.name = "accounts", input.role = "admin", output.registration_open = tracing::field::Empty), err)]
    async fn registration_enabled(&self) -> Result<bool> {
        let enabled: bool = database(
            "accounts",
            ADMIN_EXISTS,
            sqlx::query_scalar(ADMIN_EXISTS).fetch_one(&self.pool),
        )
        .await?;
        tracing::Span::current().record("output.registration_open", enabled);
        Ok(enabled)
    }

    #[tracing::instrument(name = "accounts.registration.check_name", skip_all, fields(ddd.bounded_context.name = "accounts", input.username.bytes = username.len(), output.username.available = tracing::field::Empty), err)]
    async fn registration_name_available(&self, username: &str) -> Result<bool> {
        let taken: bool = database(
            "accounts",
            DISPLAY_NAME_EXISTS,
            sqlx::query_scalar(DISPLAY_NAME_EXISTS)
                .bind(username)
                .fetch_one(&self.pool),
        )
        .await?;
        tracing::Span::current().record("output.username.available", !taken);
        Ok(!taken)
    }

    #[tracing::instrument(name = "accounts.players.provision", skip_all, fields(ddd.bounded_context.name = "accounts", input.username.bytes = username.len(), input.display_name.bytes = display_name.len(), input.role = mode.role().as_str(), input.starter.credits = STARTING_CREDITS, input.starter.gold = STARTING_GOLD, output.outcome = tracing::field::Empty), err)]
    async fn provision(
        &self,
        username: String,
        display_name: String,
        password_hash: String,
        mode: PlayerProvision,
    ) -> Result<PlayerAccount, CreatePlayerError> {
        let mut tx = database("accounts", "BEGIN", self.pool.begin())
            .await
            .map_err(anyhow::Error::from)?;
        let result = self
            .provision_in_transaction(&mut tx, &username, &display_name, password_hash, mode)
            .await;
        match result {
            Ok(id) => {
                database("accounts", "COMMIT", tx.commit())
                    .await
                    .map_err(anyhow::Error::from)?;
                tracing::Span::current().record("output.outcome", "created");
                Ok(PlayerAccount {
                    id,
                    username,
                    display_name,
                    role: mode.role(),
                })
            }
            Err(error) => {
                database("accounts", "ROLLBACK", tx.rollback())
                    .await
                    .map_err(anyhow::Error::from)?;
                tracing::Span::current().record("output.outcome", error.outcome());
                Err(error)
            }
        }
    }
}

impl PostgresPlayers {
    async fn provision_in_transaction(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        username: &str,
        display_name: &str,
        password_hash: String,
        mode: PlayerProvision,
    ) -> Result<i64, CreatePlayerError> {
        let statement = "SET LOCAL statement_timeout = '5s'";
        database(
            "accounts",
            statement,
            sqlx::query(statement).execute(&mut **tx),
        )
        .await
        .map_err(anyhow::Error::from)?;
        // Serialize name checks and inserts across web workers without locking existing accounts.
        let statement = "SELECT pg_advisory_xact_lock(hashtextextended('accounts.provision', 0))";
        database(
            "accounts",
            statement,
            sqlx::query(statement).execute(&mut **tx),
        )
        .await
        .map_err(anyhow::Error::from)?;
        if mode == PlayerProvision::FirstAdmin {
            super::player_access::lock_accounts(tx).await?;
        }
        let id = create_account(tx, username, display_name, password_hash, mode).await?;

        let mut snapshot = read_snapshot(tx, id).await?;
        let catalog = self.catalog.clone();
        let keys = self.starter_keys.clone();
        let snapshot = tokio::task::spawn_blocking(
            crate::platform::observability::in_current_span(move || {
                catalog.prepare_starter_garage(&mut snapshot, &keys)?;
                Ok::<_, anyhow::Error>(snapshot)
            }),
        )
        .await
        .map_err(anyhow::Error::from)??;
        persist_starter_garage(tx, &snapshot).await?;
        Ok(id)
    }
}

const ADMIN_EXISTS: &str = "SELECT EXISTS (SELECT 1 FROM accounts.accounts WHERE role = 'admin')";
const DISPLAY_NAME_EXISTS: &str =
    "SELECT EXISTS (SELECT 1 FROM accounts.accounts WHERE lower(display_name) = lower($1))";

async fn create_account(
    tx: &mut Transaction<'_, Postgres>,
    username: &str,
    display_name: &str,
    password_hash: String,
    mode: PlayerProvision,
) -> Result<i64, CreatePlayerError> {
    if mode == PlayerProvision::Registration {
        let enabled: bool = database(
            "accounts",
            ADMIN_EXISTS,
            sqlx::query_scalar(ADMIN_EXISTS).fetch_one(&mut **tx),
        )
        .await
        .map_err(anyhow::Error::from)?;
        if !enabled {
            return Err(CreatePlayerError::RegistrationClosed);
        }
    }
    if mode != PlayerProvision::FirstAdmin {
        let statement =
            "SELECT EXISTS (SELECT 1 FROM accounts.accounts WHERE lower(username) = lower($1))";
        let email_taken: bool = database(
            "accounts",
            statement,
            sqlx::query_scalar(statement)
                .bind(username)
                .fetch_one(&mut **tx),
        )
        .await
        .map_err(anyhow::Error::from)?;
        if email_taken {
            return Err(CreatePlayerError::Duplicate);
        }
        let name_taken: bool = database(
            "accounts",
            DISPLAY_NAME_EXISTS,
            sqlx::query_scalar(DISPLAY_NAME_EXISTS)
                .bind(display_name)
                .fetch_one(&mut **tx),
        )
        .await
        .map_err(anyhow::Error::from)?;
        if name_taken {
            return Err(CreatePlayerError::DuplicateDisplayName);
        }
    }

    let statement = "INSERT INTO accounts.accounts(username, display_name, password_hash, role, credits, gold) SELECT $1, $2, $3, $4, $5, $6 WHERE $4 = 'player' OR NOT EXISTS (SELECT 1 FROM accounts.accounts) ON CONFLICT(username) DO NOTHING RETURNING id";
    database(
        "accounts",
        statement,
        sqlx::query_scalar(statement)
            .bind(username)
            .bind(display_name)
            .bind(password_hash)
            .bind(mode.role().as_str())
            .bind(STARTING_CREDITS)
            .bind(STARTING_GOLD)
            .fetch_optional(&mut **tx),
    )
    .await
    .map_err(anyhow::Error::from)?
    .ok_or(match mode {
        PlayerProvision::FirstAdmin => CreatePlayerError::SetupClosed,
        PlayerProvision::Managed | PlayerProvision::Registration => CreatePlayerError::Duplicate,
    })
}

#[tracing::instrument(name = "accounts.players.persist_starter_garage", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = next.id, input.vehicle_count = next.vehicles.len()), err)]
async fn persist_starter_garage(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    next: &AccountSnapshot,
) -> Result<()> {
    let id = next.id;
    {
        let statement = "UPDATE accounts.accounts SET revision=$2, settings=$3, progression=$4, crew_roster=$5, slots=$6 WHERE id=$1";
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
            let statement = "INSERT INTO accounts.owned_vehicles(account_id,inventory_id,catalog_key,ammunition,has_crew) VALUES($1,$2,$3,$4,$5)";
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
