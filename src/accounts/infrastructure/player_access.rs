use super::players::PostgresPlayers;
use crate::{
    accounts::{PlayerAccessRepository, PlayerAccount, PlayerRole, RoleChange, is_admin_email},
    platform::observability::database,
};
use anyhow::Result;
use sqlx::{Postgres, Transaction};

pub(super) async fn lock_accounts(tx: &mut Transaction<'_, Postgres>) -> Result<()> {
    // Serialize emptiness and role checks with account inserts, updates, and deletes.
    let statement = "LOCK TABLE accounts.accounts IN SHARE ROW EXCLUSIVE MODE";
    database(
        "accounts",
        statement,
        sqlx::query(statement).execute(&mut **tx),
    )
    .await?;
    Ok(())
}

#[async_trait::async_trait]
impl PlayerAccessRepository for PostgresPlayers {
    #[tracing::instrument(name = "accounts.players.setup_status", skip_all, fields(ddd.bounded_context.name = "accounts", input.collection = "accounts"), err)]
    async fn setup_available(&self) -> Result<bool> {
        let statement = "SELECT NOT EXISTS (SELECT 1 FROM accounts.accounts)";
        Ok(database(
            "accounts",
            statement,
            sqlx::query_scalar(statement).fetch_one(&self.pool),
        )
        .await?)
    }

    #[tracing::instrument(name = "accounts.web.credentials", skip_all, fields(ddd.bounded_context.name = "accounts", input.username.bytes = email.len()), err)]
    async fn web_credentials(&self, email: &str) -> Result<Option<(i64, String)>> {
        let statement = "SELECT id, password_hash FROM accounts.accounts WHERE username = $1";
        Ok(database(
            "accounts",
            statement,
            sqlx::query_as(statement)
                .bind(email)
                .fetch_optional(&self.pool),
        )
        .await?)
    }

    #[tracing::instrument(name = "accounts.web.open_session", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = account_id, input.session.lifetime_seconds = lifetime_seconds), err)]
    async fn open_web_session(
        &self,
        account_id: i64,
        digest: &[u8; 32],
        lifetime_seconds: i64,
    ) -> Result<bool> {
        let statement = "DELETE FROM accounts.web_sessions WHERE expires_at <= now()";
        database(
            "accounts",
            statement,
            sqlx::query(statement).execute(&self.pool),
        )
        .await?;
        let statement =
            "INSERT INTO accounts.web_sessions(secret_hash, account_id, expires_at, role)
                         SELECT $1, id, now() + make_interval(secs => $3), role
                         FROM accounts.accounts WHERE id = $2 FOR SHARE";
        let result = database(
            "accounts",
            statement,
            sqlx::query(statement)
                .bind(digest.as_slice())
                .bind(account_id)
                .bind(lifetime_seconds as f64)
                .execute(&self.pool),
        )
        .await?;
        Ok(result.rows_affected() == 1)
    }

    #[tracing::instrument(name = "accounts.web.find_session", skip_all, fields(ddd.bounded_context.name = "accounts", input.collection = "web_sessions", output.outcome = tracing::field::Empty), err)]
    async fn find_web_session(&self, digest: &[u8; 32]) -> Result<Option<PlayerAccount>> {
        let statement = "SELECT a.id, a.username, a.display_name, a.role = 'admin'
                         FROM accounts.web_sessions s JOIN accounts.accounts a ON a.id = s.account_id
                         WHERE s.secret_hash = $1 AND s.expires_at > now() AND s.role = a.role";
        let result: Option<(i64, String, String, bool)> = database(
            "accounts",
            statement,
            sqlx::query_as(statement)
                .bind(digest.as_slice())
                .fetch_optional(&self.pool),
        )
        .await?;
        tracing::Span::current().record(
            "output.outcome",
            if result.is_some() {
                "authorized"
            } else {
                "rejected"
            },
        );
        Ok(
            result.map(|(id, username, display_name, admin)| PlayerAccount {
                id,
                username,
                display_name,
                role: if admin {
                    PlayerRole::Admin
                } else {
                    PlayerRole::Player
                },
            }),
        )
    }

    #[tracing::instrument(name = "accounts.web.close_session", skip_all, fields(ddd.bounded_context.name = "accounts", input.collection = "web_sessions", output.session_count = tracing::field::Empty), err)]
    async fn close_web_session(&self, digest: &[u8; 32]) -> Result<()> {
        let statement = "DELETE FROM accounts.web_sessions WHERE secret_hash = $1";
        let result = database(
            "accounts",
            statement,
            sqlx::query(statement)
                .bind(digest.as_slice())
                .execute(&self.pool),
        )
        .await?;
        tracing::Span::current().record("output.session_count", result.rows_affected());
        Ok(())
    }

    #[tracing::instrument(name = "accounts.players.change_role", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = actor_id, input.account_id = target_id, input.role = role.as_str(), output.outcome = tracing::field::Empty), err)]
    async fn change_role(
        &self,
        actor_id: i64,
        target_id: i64,
        role: PlayerRole,
    ) -> Result<RoleChange> {
        let mut tx = database("accounts", "BEGIN", self.pool.begin()).await?;
        let outcome = change_role_in_transaction(&mut tx, actor_id, target_id, role).await;
        match outcome {
            Ok(RoleChange::Updated) => database("accounts", "COMMIT", tx.commit()).await?,
            _ => database("accounts", "ROLLBACK", tx.rollback()).await?,
        }
        let outcome = outcome?;
        tracing::Span::current().record("output.outcome", outcome.as_str());
        Ok(outcome)
    }
}

async fn change_role_in_transaction(
    tx: &mut Transaction<'_, Postgres>,
    actor_id: i64,
    target_id: i64,
    role: PlayerRole,
) -> Result<RoleChange> {
    let statement = "SET LOCAL statement_timeout = '5s'";
    database(
        "accounts",
        statement,
        sqlx::query(statement).execute(&mut **tx),
    )
    .await?;
    lock_accounts(tx).await?;
    let statement =
        "SELECT EXISTS (SELECT 1 FROM accounts.accounts WHERE id = $1 AND role = 'admin')";
    let actor_is_admin: bool = database(
        "accounts",
        statement,
        sqlx::query_scalar(statement)
            .bind(actor_id)
            .fetch_one(&mut **tx),
    )
    .await?;
    if !actor_is_admin {
        return Ok(RoleChange::ActorNotAdmin);
    }
    if actor_id == target_id {
        return Ok(RoleChange::OwnRole);
    }
    let statement = "SELECT username, role FROM accounts.accounts WHERE id = $1";
    let target: Option<(String, String)> = database(
        "accounts",
        statement,
        sqlx::query_as(statement)
            .bind(target_id)
            .fetch_optional(&mut **tx),
    )
    .await?;
    let Some((email, current_role)) = target else {
        return Ok(RoleChange::PlayerNotFound);
    };
    if current_role == role.as_str() {
        return Ok(RoleChange::Unchanged);
    }
    if role == PlayerRole::Admin && !is_admin_email(&email) {
        return Ok(RoleChange::EmailRequired);
    }
    let statement = "UPDATE accounts.accounts SET role = $2 WHERE id = $1
                     AND ($2 = 'admin' OR EXISTS (SELECT 1 FROM accounts.accounts WHERE id <> $1 AND role = 'admin'))";
    let result = database(
        "accounts",
        statement,
        sqlx::query(statement)
            .bind(target_id)
            .bind(role.as_str())
            .execute(&mut **tx),
    )
    .await?;
    if result.rows_affected() == 0 {
        return Ok(RoleChange::LastAdmin);
    }
    let statement = "DELETE FROM accounts.web_sessions WHERE account_id = $1";
    database(
        "accounts",
        statement,
        sqlx::query(statement).bind(target_id).execute(&mut **tx),
    )
    .await?;
    Ok(RoleChange::Updated)
}
