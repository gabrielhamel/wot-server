use super::verify_password;
use crate::accounts::{
    NewPlayer, PlayerAccount, PlayerPage, PlayerPagination, PlayerRole, RoleChange,
};
use std::future::Future;
use thiserror::Error;

pub trait PlayerRepository: Clone + Send + Sync + 'static {
    fn list(
        &self,
        pagination: PlayerPagination,
    ) -> impl Future<Output = anyhow::Result<PlayerPage>> + Send;
    fn provision(
        &self,
        username: String,
        display_name: String,
        password_hash: String,
        role: PlayerRole,
    ) -> impl Future<Output = anyhow::Result<Option<PlayerAccount>>> + Send;
}

pub trait PlayerAccessRepository: PlayerRepository {
    fn setup_available(&self) -> impl Future<Output = anyhow::Result<bool>> + Send;
    fn admin_credentials(
        &self,
        email: &str,
    ) -> impl Future<Output = anyhow::Result<Option<(i64, String)>>> + Send;
    fn open_admin_session(
        &self,
        account_id: i64,
        digest: &[u8; 32],
        lifetime_seconds: i64,
    ) -> impl Future<Output = anyhow::Result<bool>> + Send;
    fn find_admin_session(
        &self,
        digest: &[u8; 32],
    ) -> impl Future<Output = anyhow::Result<Option<PlayerAccount>>> + Send;
    fn close_admin_session(
        &self,
        digest: &[u8; 32],
    ) -> impl Future<Output = anyhow::Result<()>> + Send;
    fn change_role(
        &self,
        actor_id: i64,
        target_id: i64,
        role: PlayerRole,
    ) -> impl Future<Output = anyhow::Result<RoleChange>> + Send;
}

#[derive(Debug, Error)]
pub enum CreatePlayerError {
    #[error("That login is already in use.")]
    Duplicate,
    #[error("First administrator setup is closed.")]
    SetupClosed,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

#[derive(Clone)]
pub struct PlayerAccounts<R> {
    repository: R,
}

impl<R: PlayerRepository> PlayerAccounts<R> {
    pub fn new(repository: R) -> Self {
        Self { repository }
    }

    #[tracing::instrument(name = "accounts.players.list", skip_all, fields(ddd.bounded_context.name = "accounts", input.offset = pagination.offset(), input.limit = pagination.limit()), err)]
    pub async fn list(&self, pagination: PlayerPagination) -> anyhow::Result<PlayerPage> {
        self.repository.list(pagination).await
    }

    #[tracing::instrument(name = "accounts.players.create", skip_all, fields(ddd.bounded_context.name = "accounts", input.username.bytes = tracing::field::Empty, input.display_name.bytes = tracing::field::Empty), err)]
    pub async fn create(&self, player: NewPlayer) -> Result<PlayerAccount, CreatePlayerError> {
        self.provision(player, PlayerRole::Player).await
    }

    #[tracing::instrument(name = "accounts.players.bootstrap_admin", skip_all, fields(ddd.bounded_context.name = "accounts", input.role = "admin", input.username.bytes = tracing::field::Empty, input.display_name.bytes = tracing::field::Empty), err)]
    pub async fn bootstrap_admin(
        &self,
        player: NewPlayer,
    ) -> Result<PlayerAccount, CreatePlayerError> {
        self.provision(player, PlayerRole::Admin).await
    }

    async fn provision(
        &self,
        player: NewPlayer,
        role: PlayerRole,
    ) -> Result<PlayerAccount, CreatePlayerError> {
        let (username, display_name, password) = player.into_parts();
        tracing::Span::current().record("input.username.bytes", username.len());
        tracing::Span::current().record("input.display_name.bytes", display_name.len());
        let hash = hash_password(password).await?;
        self.repository
            .provision(username, display_name, hash, role)
            .await?
            .ok_or(match role {
                PlayerRole::Player => CreatePlayerError::Duplicate,
                PlayerRole::Admin => CreatePlayerError::SetupClosed,
            })
    }
}

impl<R: PlayerAccessRepository> PlayerAccounts<R> {
    pub async fn setup_available(&self) -> anyhow::Result<bool> {
        self.repository.setup_available().await
    }

    #[tracing::instrument(name = "accounts.admin.authenticate", skip_all, fields(ddd.bounded_context.name = "accounts", input.username.bytes = email.len(), output.outcome = tracing::field::Empty), err)]
    pub async fn authenticate_admin(
        &self,
        email: &str,
        password: String,
    ) -> anyhow::Result<Option<i64>> {
        // Use the same Argon2 parameters for absent and present administrator credentials.
        const PLACEHOLDER_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$AAAAAAAAAAAAAAAAAAAAAA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let credentials = self.repository.admin_credentials(email).await?;
        let (account, hash) = match credentials {
            Some((id, hash)) => (Some(id), hash),
            None => (None, PLACEHOLDER_HASH.to_owned()),
        };
        let authenticated = if verify_password(password, hash).await? {
            account
        } else {
            None
        };
        tracing::Span::current().record(
            "output.outcome",
            if authenticated.is_some() {
                "authorized"
            } else {
                "rejected"
            },
        );
        Ok(authenticated)
    }

    pub async fn open_admin_session(
        &self,
        account_id: i64,
        digest: &[u8; 32],
        lifetime_seconds: i64,
    ) -> anyhow::Result<bool> {
        self.repository
            .open_admin_session(account_id, digest, lifetime_seconds)
            .await
    }

    pub async fn find_admin_session(
        &self,
        digest: &[u8; 32],
    ) -> anyhow::Result<Option<PlayerAccount>> {
        self.repository.find_admin_session(digest).await
    }

    pub async fn close_admin_session(&self, digest: &[u8; 32]) -> anyhow::Result<()> {
        self.repository.close_admin_session(digest).await
    }

    pub async fn change_role(
        &self,
        actor_id: i64,
        target_id: i64,
        role: PlayerRole,
    ) -> anyhow::Result<RoleChange> {
        self.repository.change_role(actor_id, target_id, role).await
    }
}

#[tracing::instrument(name = "accounts.players.hash_password", skip_all, fields(ddd.bounded_context.name = "accounts", input.algorithm = "argon2id"), err)]
async fn hash_password(password: String) -> anyhow::Result<String> {
    use argon2::{
        Argon2, PasswordHasher,
        password_hash::{
            SaltString,
            rand_core::{OsRng, RngCore},
        },
    };
    tokio::task::spawn_blocking(crate::platform::observability::in_current_span(move || {
        let mut salt_bytes = [0; 16];
        OsRng
            .try_fill_bytes(&mut salt_bytes)
            .map_err(|error| anyhow::anyhow!("Generate account password salt: {error}"))?;
        let salt = SaltString::encode_b64(&salt_bytes)
            .map_err(|error| anyhow::anyhow!("Encode account password salt: {error}"))?;
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|hash| hash.to_string())
            .map_err(|error| anyhow::anyhow!("Hash account password: {error}"))
    }))
    .await?
}
