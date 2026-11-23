use crate::{
    AccountNotFound, AccountRepository, AccountSnapshot, AmmunitionChange, Balances, CrewRoster,
    OwnedVehicle, ProgressionChange, ProgressionState, ShellStock, VehicleAmmunition,
};
use anyhow::{Context, Result};
use sqlx::{PgPool, Row};
use std::collections::BTreeMap;

#[derive(Clone)]
pub struct PostgresAccounts {
    pool: PgPool,
}

impl PostgresAccounts {
    pub fn connection_pool(&self) -> PgPool {
        self.pool.clone()
    }

    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    #[tracing::instrument(name = "accounts.postgres.catalog_keys", skip_all, fields(ddd.bounded_context.name = "accounts", input.collection = "account_vehicles"), err)]
    pub async fn catalog_keys(&self) -> Result<Vec<String>> {
        Ok({
            let statement =
                "SELECT DISTINCT catalog_key FROM accounts.owned_vehicles ORDER BY catalog_key";
            wot_platform::observability::database(
                "accounts",
                statement,
                sqlx::query_scalar(statement).fetch_all(&self.pool),
            )
            .await
        }?)
    }
}

#[async_trait::async_trait]
impl AccountRepository for PostgresAccounts {
    #[tracing::instrument(name = "accounts.postgres.credentials", skip_all, fields(ddd.bounded_context.name = "accounts", input.username.bytes = username.len()), err)]
    async fn credentials(&self, username: &str) -> Result<Option<(i64, String)>> {
        Ok({
            let statement = "SELECT id,password_hash FROM accounts.accounts WHERE username=$1";
            wot_platform::observability::database(
                "accounts",
                statement,
                sqlx::query_as(statement)
                    .bind(username)
                    .fetch_optional(&self.pool),
            )
            .await
        }?)
    }

    #[tracing::instrument(name = "accounts.postgres.snapshot", skip_all, fields(ddd.bounded_context.name = "accounts", input.id = id), err)]
    async fn snapshot(&self, id: i64) -> Result<AccountSnapshot> {
        let mut tx =
            wot_platform::observability::database("accounts", "BEGIN", self.pool.begin()).await?;
        {
            let statement = "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY";
            wot_platform::observability::database(
                "accounts",
                statement,
                sqlx::query(statement).execute(&mut *tx),
            )
            .await
        }?;
        let snapshot = read_snapshot(&mut tx, id).await?;
        wot_platform::observability::database("accounts", "COMMIT", tx.commit()).await?;
        Ok(snapshot)
    }

    #[tracing::instrument(name = "accounts.postgres.save_progression", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = change.snapshot.id, input.previous_revision = change.previous_revision), err)]
    async fn save_progression(&self, change: &ProgressionChange) -> Result<bool> {
        let snapshot = &change.snapshot;
        let mut tx =
            wot_platform::observability::database("accounts", "BEGIN", self.pool.begin()).await?;
        let updated = {
            let statement = "UPDATE accounts.accounts SET credits=$3, free_xp=$4, progression=$5, depot_shells=$6, revision=$7, gold=$8, berths=$9, crew_roster=$10, bonds=$11, slots=$12, equipment_components=$13 WHERE id=$1 AND revision=$2";
            wot_platform::observability::database(
                "accounts",
                statement,
                sqlx::query(statement)
                    .bind(snapshot.id)
                    .bind(change.previous_revision)
                    .bind(snapshot.balances.credits)
                    .bind(snapshot.balances.free_xp)
                    .bind(sqlx::types::Json(&snapshot.progression))
                    .bind(sqlx::types::Json(&snapshot.depot_shells))
                    .bind(snapshot.revision)
                    .bind(snapshot.balances.gold)
                    .bind(snapshot.balances.berths)
                    .bind(snapshot.crew.as_ref().map(sqlx::types::Json))
                    .bind(snapshot.balances.bonds)
                    .bind(snapshot.balances.slots)
                    .bind(snapshot.balances.equipment_components)
                    .execute(&mut *tx),
            )
            .await
        }?;
        if updated.rows_affected() == 0 {
            return Ok(false);
        }
        if let Some(inventory_id) = change.sold_vehicle {
            let deleted = {
                let statement =
                    "DELETE FROM accounts.owned_vehicles WHERE account_id=$1 AND inventory_id=$2";
                wot_platform::observability::database(
                    "accounts",
                    statement,
                    sqlx::query(statement)
                        .bind(snapshot.id)
                        .bind(inventory_id)
                        .execute(&mut *tx),
                )
                .await
            }?;
            anyhow::ensure!(
                deleted.rows_affected() == 1,
                "Sold vehicle no longer exists"
            );
        }
        for vehicle in &snapshot.vehicles {
            if change.acquired_vehicle == Some(vehicle.inventory_id) {
                {
                    let statement = "INSERT INTO accounts.owned_vehicles(account_id,inventory_id,catalog_key,ammunition,has_crew) VALUES($1,$2,$3,$4,$5)";
                    wot_platform::observability::database(
                        "accounts",
                        statement,
                        sqlx::query(statement)
                            .bind(snapshot.id)
                            .bind(vehicle.inventory_id)
                            .bind(&vehicle.catalog_key)
                            .bind(vehicle.ammunition.as_ref().map(sqlx::types::Json))
                            .bind(vehicle.has_crew)
                            .execute(&mut *tx),
                    )
                    .await
                }?;
                continue;
            }
            let updated = {
                let statement = "UPDATE accounts.owned_vehicles SET ammunition=$3 WHERE account_id=$1 AND inventory_id=$2";
                wot_platform::observability::database(
                    "accounts",
                    statement,
                    sqlx::query(statement)
                        .bind(snapshot.id)
                        .bind(vehicle.inventory_id)
                        .bind(vehicle.ammunition.as_ref().map(sqlx::types::Json))
                        .execute(&mut *tx),
                )
                .await
            }?;
            anyhow::ensure!(
                updated.rows_affected() == 1,
                "Owned vehicle no longer exists"
            );
        }
        wot_platform::observability::database("accounts", "COMMIT", tx.commit()).await?;
        Ok(true)
    }

    #[tracing::instrument(name = "accounts.postgres.save_ammunition", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = change.snapshot.id, input.previous_revision = change.previous_revision), err)]
    async fn save_ammunition(&self, change: &AmmunitionChange) -> Result<bool> {
        let snapshot = &change.snapshot;
        let ammunition = change
            .ammunition()
            .context("Missing proposed vehicle ammunition")?;
        let mut tx =
            wot_platform::observability::database("accounts", "BEGIN", self.pool.begin()).await?;
        let updated = {
            let statement = "UPDATE accounts.accounts SET credits=$3, gold=$4, depot_shells=$5, revision=$6 WHERE id=$1 AND revision=$2";
            wot_platform::observability::database(
                "accounts",
                statement,
                sqlx::query(statement)
                    .bind(snapshot.id)
                    .bind(change.previous_revision)
                    .bind(snapshot.balances.credits)
                    .bind(snapshot.balances.gold)
                    .bind(sqlx::types::Json(&snapshot.depot_shells))
                    .bind(snapshot.revision)
                    .execute(&mut *tx),
            )
            .await
        }?;
        if updated.rows_affected() == 0 {
            return Ok(false);
        }

        let vehicle = {
            let statement = "UPDATE accounts.owned_vehicles SET ammunition=$3 WHERE account_id=$1 AND inventory_id=$2";
            wot_platform::observability::database(
                "accounts",
                statement,
                sqlx::query(statement)
                    .bind(snapshot.id)
                    .bind(change.inventory_id)
                    .bind(sqlx::types::Json(ammunition))
                    .execute(&mut *tx),
            )
            .await
        }?;
        anyhow::ensure!(
            vehicle.rows_affected() == 1,
            "Owned vehicle no longer exists"
        );
        wot_platform::observability::database("accounts", "COMMIT", tx.commit()).await?;
        Ok(true)
    }

    #[tracing::instrument(name = "accounts.postgres.save_settings", skip_all, fields(ddd.bounded_context.name = "accounts", input.id = id, input.settings.count = settings.len()), err)]
    async fn save_settings(&self, id: i64, settings: BTreeMap<i32, i32>) -> Result<i64> {
        Ok({
            let statement = "UPDATE accounts.accounts SET settings=settings || $2::jsonb, revision=revision+1 WHERE id=$1 RETURNING revision";
            wot_platform::observability::database(
                "accounts",
                statement,
                sqlx::query_scalar(statement)
                    .bind(id)
                    .bind(sqlx::types::Json(settings))
                    .fetch_one(&self.pool),
            )
            .await
        }?)
    }
}

#[tracing::instrument(name = "accounts.postgres.read_snapshot", skip_all, fields(ddd.bounded_context.name = "accounts", input.id = id), err)]
pub(super) async fn read_snapshot(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: i64,
) -> Result<AccountSnapshot> {
    let statement = "SELECT display_name, revision, credits, gold, bonds, free_xp, slots, berths, settings, depot_shells, progression, crew_roster, equipment_components FROM accounts.accounts WHERE id=$1";
    let row = wot_platform::observability::database(
        "accounts",
        statement,
        sqlx::query(statement).bind(id).fetch_optional(&mut **tx),
    )
    .await?
    .ok_or(AccountNotFound(id))?;
    let vehicles = {
        let statement = "SELECT inventory_id,catalog_key,ammunition,has_crew FROM accounts.owned_vehicles WHERE account_id=$1 ORDER BY inventory_id";
        wot_platform::observability::database(
            "accounts",
            statement,
            sqlx::query(statement).bind(id).fetch_all(&mut **tx),
        )
        .await
    }?;
    let snapshot = AccountSnapshot {
        id,
        display_name: row.try_get("display_name")?,
        revision: row.try_get("revision")?,
        balances: Balances {
            credits: row.try_get("credits")?,
            gold: row.try_get("gold")?,
            bonds: row.try_get("bonds")?,
            equipment_components: row.try_get("equipment_components")?,
            free_xp: row.try_get("free_xp")?,
            slots: row.try_get("slots")?,
            berths: row.try_get("berths")?,
        },
        crew: row
            .try_get::<Option<sqlx::types::Json<CrewRoster>>, _>("crew_roster")?
            .map(|value| value.0),
        progression: row
            .try_get::<sqlx::types::Json<ProgressionState>, _>("progression")?
            .0,
        settings: row
            .try_get::<sqlx::types::Json<BTreeMap<i32, i32>>, _>("settings")?
            .0,
        depot_shells: row
            .try_get::<sqlx::types::Json<ShellStock>, _>("depot_shells")?
            .0,
        vehicles: vehicles
            .into_iter()
            .map(|vehicle| {
                Ok(OwnedVehicle {
                    has_crew: vehicle.try_get("has_crew")?,
                    inventory_id: vehicle.try_get("inventory_id")?,
                    catalog_key: vehicle.try_get("catalog_key")?,
                    ammunition: vehicle
                        .try_get::<Option<sqlx::types::Json<VehicleAmmunition>>, _>("ammunition")?
                        .map(|json| json.0),
                })
            })
            .collect::<Result<_>>()?,
    };
    Ok(snapshot)
}
