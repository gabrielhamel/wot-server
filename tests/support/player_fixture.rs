use anyhow::Result;
use argon2::{
    Argon2, PasswordHasher,
    password_hash::{SaltString, rand_core::OsRng},
};
use wot_server::accounts::test_support::PostgresAccounts;

pub async fn insert_player(
    repository: &PostgresAccounts,
    username: &str,
    display_name: &str,
    password: &str,
    vehicles: &[String],
) -> Result<i64> {
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
        .map_err(|error| anyhow::anyhow!("Hash fixture password: {error}"))?
        .to_string();
    let pool = repository.connection_pool();
    let mut tx = pool.begin().await?;
    let id = sqlx::query_scalar(
        "INSERT INTO accounts.accounts(username, display_name, password_hash) VALUES($1,$2,$3) RETURNING id",
    )
    .bind(username)
    .bind(display_name)
    .bind(hash)
    .fetch_one(&mut *tx)
    .await?;
    for (index, key) in vehicles.iter().enumerate() {
        sqlx::query(
            "INSERT INTO accounts.owned_vehicles(account_id, inventory_id, catalog_key) VALUES($1,$2,$3)",
        )
        .bind(id)
        .bind(i64::try_from(index)? + 1)
        .bind(key)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(id)
}

pub fn gateway_accounts(
    repository: &PostgresAccounts,
    catalog: &std::sync::Arc<wot_server::accounts::test_support::GarageCatalog>,
) -> wot_server::player_sessions::GatewayAccounts {
    use wot_server::accounts::test_support::{
        Accounts, game_accounts, garage_commands, garage_queries,
    };

    let accounts = Accounts::new(repository.clone());
    wot_server::player_sessions::GatewayAccounts {
        accounts: game_accounts(accounts.clone()),
        catalog: garage_queries(catalog.clone()),
        garage: garage_commands(accounts, catalog.clone()),
    }
}
