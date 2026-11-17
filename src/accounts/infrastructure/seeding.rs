use super::{
    client::GarageCatalog,
    postgres::{PostgresAccounts, read_snapshot},
};
use crate::accounts::AccountSnapshot;
use anyhow::{Result, ensure};
use argon2::{Argon2, PasswordHash, PasswordVerifier};
use sqlx::Row;

pub const DEMO_PLAYERS: [(&str, &str); 2] = [
    ("player1@demo.demo", "Player_1"),
    ("player2@demo.demo", "Player_2"),
];

#[derive(Debug)]
pub struct DemoSeedReport {
    pub username: String,
    pub account_id: i64,
    pub created: bool,
    pub changed: bool,
    pub tier_one_vehicles: usize,
}

impl PostgresAccounts {
    #[tracing::instrument(name = "accounts.seeding.seed_demo_players", skip_all, fields(ddd.bounded_context.name = "accounts", input.player_count = DEMO_PLAYERS.len()), err)]
    pub async fn seed_demo_players(&self, catalog: &GarageCatalog) -> Result<Vec<DemoSeedReport>> {
        let keys = catalog.tier_one_keys()?;
        let pool = self.connection_pool();
        let mut tx =
            crate::platform::observability::database("accounts", "BEGIN", pool.begin()).await?;
        let mut reports = Vec::new();
        for (username, display_name) in DEMO_PLAYERS {
            let hash = super::super::application::players::hash_password("demo".into()).await?;
            let created: Option<i64> = {
                let statement = "INSERT INTO accounts(username,display_name,password_hash) VALUES($1,$2,$3) ON CONFLICT(username) DO NOTHING RETURNING id";
                crate::platform::observability::database(
                    "accounts",
                    statement,
                    sqlx::query_scalar(statement)
                        .bind(username)
                        .bind(display_name)
                        .bind(&hash)
                        .fetch_optional(&mut *tx),
                )
                .await
            }?;
            let row = {
                let statement = "SELECT id, display_name, password_hash FROM accounts WHERE username=$1 FOR UPDATE";
                crate::platform::observability::database(
                    "accounts",
                    statement,
                    sqlx::query(statement).bind(username).fetch_one(&mut *tx),
                )
                .await
            }?;
            let id = row.try_get("id")?;
            ensure!(
                row.try_get::<String, _>("display_name")? == display_name,
                "Demo email {username} belongs to a different display name"
            );
            let stored_hash: String = row.try_get("password_hash")?;
            let valid_password = tokio::task::spawn_blocking(
                crate::platform::observability::in_current_span(move || {
                    PasswordHash::new(&stored_hash)
                        .is_ok_and(|hash| Argon2::default().verify_password(b"demo", &hash).is_ok())
                }),
            )
            .await?;
            ensure!(
                valid_password,
                "Demo email {username} has a different password; credentials were preserved"
            );
            let before = read_snapshot(&mut tx, id).await?;
            let mut next = before.clone();
            catalog.seed_tier_one_garage(&mut next, &keys)?;
            let changed = garage_changed(&before, &next);
            if changed {
                next.revision = before
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("Account revision limit reached"))?;
                persist_starter_garage(&mut tx, &next).await?;
            }
            reports.push(DemoSeedReport {
                username: username.into(),
                account_id: id,
                created: created.is_some(),
                changed,
                tier_one_vehicles: keys.len(),
            });
        }
        crate::platform::observability::database("accounts", "COMMIT", tx.commit()).await?;
        Ok(reports)
    }
}

fn garage_changed(before: &AccountSnapshot, next: &AccountSnapshot) -> bool {
    before.settings != next.settings
        || before.progression != next.progression
        || before.crew != next.crew
        || before.vehicles != next.vehicles
        || before.balances.slots != next.balances.slots
}

#[tracing::instrument(name = "accounts.seeding.persist_starter_garage", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = next.id, input.vehicle_count = next.vehicles.len()), err)]
pub(super) async fn persist_starter_garage(
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
            let statement = "INSERT INTO owned_vehicles(account_id,inventory_id,catalog_key,ammunition,has_crew) VALUES($1,$2,$3,$4,$5) ON CONFLICT(account_id,inventory_id) DO UPDATE SET ammunition=EXCLUDED.ammunition,has_crew=EXCLUDED.has_crew";
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
