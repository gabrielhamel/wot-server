use crate::accounts::{
    AccountNotFound, AccountRepository, AccountSnapshot, AmmunitionChange, Balances, CrewRoster,
    OwnedVehicle, ProgressionChange, ProgressionState, ShellStock, VehicleAmmunition,
};
use anyhow::{Context, Result};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use std::{collections::BTreeMap, time::Duration};

#[derive(Clone)]
pub struct PostgresAccounts {
    pool: PgPool,
}

impl PostgresAccounts {
    pub fn connection_pool(&self) -> PgPool {
        self.pool.clone()
    }

    pub async fn connect(url: &str, connections: u32) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(connections)
            .acquire_timeout(Duration::from_secs(5))
            .connect(url)
            .await
            .context("Connect to PostgreSQL")?;
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .context("Migrate account database")?;
        Ok(Self { pool })
    }

    pub async fn seed(
        &self,
        username: &str,
        display_name: &str,
        password: String,
        vehicles: &[String],
    ) -> Result<()> {
        let hash = super::super::application::players::hash_password(password).await?;
        let mut tx = self.pool.begin().await?;
        let id: Option<i64> = sqlx::query_scalar("INSERT INTO accounts(username, display_name, password_hash) VALUES($1, $2, $3) ON CONFLICT(username) DO NOTHING RETURNING id")
            .bind(username).bind(display_name).bind(hash).fetch_optional(&mut *tx).await?;
        if let Some(id) = id {
            for (index, key) in vehicles.iter().enumerate() {
                sqlx::query("INSERT INTO owned_vehicles(account_id, inventory_id, catalog_key) VALUES($1,$2,$3)")
                    .bind(id).bind(index as i64 + 1).bind(key).execute(&mut *tx).await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn catalog_keys(&self) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar(
            "SELECT DISTINCT catalog_key FROM owned_vehicles ORDER BY catalog_key",
        )
        .fetch_all(&self.pool)
        .await?)
    }
}

impl AccountRepository for PostgresAccounts {
    async fn credentials(&self, username: &str) -> Result<Option<(i64, String)>> {
        Ok(
            sqlx::query_as("SELECT id,password_hash FROM accounts WHERE username=$1")
                .bind(username)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    async fn snapshot(&self, id: i64) -> Result<AccountSnapshot> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *tx)
            .await?;
        let snapshot = read_snapshot(&mut tx, id).await?;
        tx.commit().await?;
        Ok(snapshot)
    }

    async fn save_progression(&self, change: &ProgressionChange) -> Result<bool> {
        let snapshot = &change.snapshot;
        let mut tx = self.pool.begin().await?;
        let updated = sqlx::query("UPDATE accounts SET credits=$3, free_xp=$4, progression=$5, depot_shells=$6, revision=$7, gold=$8, berths=$9, crew_roster=$10, bonds=$11, slots=$12, equipment_components=$13 WHERE id=$1 AND revision=$2")
            .bind(snapshot.id).bind(change.previous_revision).bind(snapshot.balances.credits)
            .bind(snapshot.balances.free_xp).bind(sqlx::types::Json(&snapshot.progression))
            .bind(sqlx::types::Json(&snapshot.depot_shells)).bind(snapshot.revision).bind(snapshot.balances.gold)
            .bind(snapshot.balances.berths).bind(snapshot.crew.as_ref().map(sqlx::types::Json))
            .bind(snapshot.balances.bonds).bind(snapshot.balances.slots)
            .bind(snapshot.balances.equipment_components).execute(&mut *tx).await?;
        if updated.rows_affected() == 0 {
            return Ok(false);
        }
        if let Some(inventory_id) = change.sold_vehicle {
            let deleted =
                sqlx::query("DELETE FROM owned_vehicles WHERE account_id=$1 AND inventory_id=$2")
                    .bind(snapshot.id)
                    .bind(inventory_id)
                    .execute(&mut *tx)
                    .await?;
            anyhow::ensure!(
                deleted.rows_affected() == 1,
                "Sold vehicle no longer exists"
            );
        }
        for vehicle in &snapshot.vehicles {
            if change.acquired_vehicle == Some(vehicle.inventory_id) {
                sqlx::query("INSERT INTO owned_vehicles(account_id,inventory_id,catalog_key,ammunition,has_crew) VALUES($1,$2,$3,$4,$5)")
                    .bind(snapshot.id).bind(vehicle.inventory_id).bind(&vehicle.catalog_key)
                    .bind(vehicle.ammunition.as_ref().map(sqlx::types::Json)).bind(vehicle.has_crew).execute(&mut *tx).await?;
                continue;
            }
            let updated = sqlx::query(
                "UPDATE owned_vehicles SET ammunition=$3 WHERE account_id=$1 AND inventory_id=$2",
            )
            .bind(snapshot.id)
            .bind(vehicle.inventory_id)
            .bind(vehicle.ammunition.as_ref().map(sqlx::types::Json))
            .execute(&mut *tx)
            .await?;
            anyhow::ensure!(
                updated.rows_affected() == 1,
                "Owned vehicle no longer exists"
            );
        }
        tx.commit().await?;
        Ok(true)
    }

    async fn save_ammunition(&self, change: &AmmunitionChange) -> Result<bool> {
        let snapshot = &change.snapshot;
        let ammunition = change
            .ammunition()
            .context("Missing proposed vehicle ammunition")?;
        let mut tx = self.pool.begin().await?;
        let updated = sqlx::query(
            "UPDATE accounts SET credits=$3, gold=$4, depot_shells=$5, revision=$6 WHERE id=$1 AND revision=$2",
        )
        .bind(snapshot.id)
        .bind(change.previous_revision)
        .bind(snapshot.balances.credits)
        .bind(snapshot.balances.gold)
        .bind(sqlx::types::Json(&snapshot.depot_shells))
        .bind(snapshot.revision)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() == 0 {
            return Ok(false);
        }

        let vehicle = sqlx::query(
            "UPDATE owned_vehicles SET ammunition=$3 WHERE account_id=$1 AND inventory_id=$2",
        )
        .bind(snapshot.id)
        .bind(change.inventory_id)
        .bind(sqlx::types::Json(ammunition))
        .execute(&mut *tx)
        .await?;
        anyhow::ensure!(
            vehicle.rows_affected() == 1,
            "Owned vehicle no longer exists"
        );
        tx.commit().await?;
        Ok(true)
    }

    async fn save_settings(&self, id: i64, settings: BTreeMap<i32, i32>) -> Result<i64> {
        Ok(sqlx::query_scalar("UPDATE accounts SET settings=settings || $2::jsonb, revision=revision+1 WHERE id=$1 RETURNING revision")
            .bind(id).bind(sqlx::types::Json(settings)).fetch_one(&self.pool).await?)
    }
}

pub(super) async fn read_snapshot(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: i64,
) -> Result<AccountSnapshot> {
    let row = sqlx::query("SELECT display_name, revision, credits, gold, bonds, free_xp, slots, berths, settings, depot_shells, progression, crew_roster, equipment_components FROM accounts WHERE id=$1")
            .bind(id).fetch_optional(&mut **tx).await?.ok_or(AccountNotFound(id))?;
    let vehicles = sqlx::query("SELECT inventory_id,catalog_key,ammunition,has_crew FROM owned_vehicles WHERE account_id=$1 ORDER BY inventory_id")
            .bind(id).fetch_all(&mut **tx).await?;
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
