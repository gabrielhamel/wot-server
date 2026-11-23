use super::verify_password;
use crate::{NewPlayer, PlayerAccount, PlayerPage, PlayerPagination, PlayerRole, RoleChange};
use std::sync::Arc;
use thiserror::Error;

#[async_trait::async_trait]
pub trait PlayerRepository: Send + Sync + 'static {
    async fn list(&self, pagination: PlayerPagination) -> anyhow::Result<PlayerPage>;
    async fn provision(
        &self,
        username: String,
        display_name: String,
        password_hash: String,
        mode: PlayerProvision,
    ) -> Result<PlayerAccount, CreatePlayerError>;
    async fn registration_enabled(&self) -> anyhow::Result<bool>;
    async fn registration_name_available(&self, username: &str) -> anyhow::Result<bool>;
}

#[async_trait::async_trait]
pub trait PlayerAccessRepository: PlayerRepository {
    async fn setup_available(&self) -> anyhow::Result<bool>;
    async fn web_credentials(&self, email: &str) -> anyhow::Result<Option<(i64, String)>>;
    async fn open_web_session(
        &self,
        account_id: i64,
        digest: &[u8; 32],
        lifetime_seconds: i64,
    ) -> anyhow::Result<bool>;
    async fn find_web_session(&self, digest: &[u8; 32]) -> anyhow::Result<Option<PlayerAccount>>;
    async fn close_web_session(&self, digest: &[u8; 32]) -> anyhow::Result<()>;
    async fn change_role(
        &self,
        actor_id: i64,
        target_id: i64,
        role: PlayerRole,
    ) -> anyhow::Result<RoleChange>;
}

#[derive(Debug, Error)]
pub enum CreatePlayerError {
    #[error("That login is already in use.")]
    Duplicate,
    #[error("That username is already taken.")]
    DuplicateDisplayName,
    #[error("Registration opens after the first administrator is created.")]
    RegistrationClosed,
    #[error("First administrator setup is closed.")]
    SetupClosed,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl CreatePlayerError {
    pub fn outcome(&self) -> &'static str {
        match self {
            Self::Duplicate => "duplicate_login",
            Self::DuplicateDisplayName => "duplicate_username",
            Self::RegistrationClosed => "registration_closed",
            Self::SetupClosed => "conflict",
            Self::Internal(_) => "failed",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PlayerProvision {
    Managed,
    FirstAdmin,
    Registration,
}

impl PlayerProvision {
    pub fn role(self) -> PlayerRole {
        match self {
            Self::Managed | Self::Registration => PlayerRole::Player,
            Self::FirstAdmin => PlayerRole::Admin,
        }
    }
}

#[derive(Clone)]
pub struct PlayerAccess {
    repository: Arc<dyn PlayerAccessRepository>,
}

impl PlayerAccess {
    pub(crate) fn new(repository: impl PlayerAccessRepository) -> Self {
        Self {
            repository: Arc::new(repository),
        }
    }

    #[tracing::instrument(name = "accounts.players.list", skip_all, fields(ddd.bounded_context.name = "accounts", input.offset = pagination.offset(), input.limit = pagination.limit()), err)]
    pub async fn list(&self, pagination: PlayerPagination) -> anyhow::Result<PlayerPage> {
        self.repository.list(pagination).await
    }

    #[tracing::instrument(name = "accounts.players.create", skip_all, fields(ddd.bounded_context.name = "accounts", input.username.bytes = tracing::field::Empty, input.display_name.bytes = tracing::field::Empty), err)]
    pub async fn create(&self, player: NewPlayer) -> Result<PlayerAccount, CreatePlayerError> {
        self.provision(player, PlayerProvision::Managed).await
    }

    #[tracing::instrument(name = "accounts.players.bootstrap_admin", skip_all, fields(ddd.bounded_context.name = "accounts", input.role = "admin", input.username.bytes = tracing::field::Empty, input.display_name.bytes = tracing::field::Empty), err)]
    pub async fn bootstrap_admin(
        &self,
        player: NewPlayer,
    ) -> Result<PlayerAccount, CreatePlayerError> {
        self.provision(player, PlayerProvision::FirstAdmin).await
    }

    #[tracing::instrument(name = "accounts.registration.create", skip_all, fields(ddd.bounded_context.name = "accounts", input.username.bytes = tracing::field::Empty, input.display_name.bytes = tracing::field::Empty, input.role = "player"), err)]
    pub async fn register(&self, player: NewPlayer) -> Result<PlayerAccount, CreatePlayerError> {
        self.provision(player, PlayerProvision::Registration).await
    }

    pub async fn registration_enabled(&self) -> anyhow::Result<bool> {
        self.repository.registration_enabled().await
    }

    pub async fn registration_name_available(&self, username: &str) -> anyhow::Result<bool> {
        self.repository.registration_name_available(username).await
    }

    async fn provision(
        &self,
        player: NewPlayer,
        mode: PlayerProvision,
    ) -> Result<PlayerAccount, CreatePlayerError> {
        let (username, display_name, password) = player.into_parts();
        tracing::Span::current().record("input.username.bytes", username.len());
        tracing::Span::current().record("input.display_name.bytes", display_name.len());
        let hash = hash_password(password).await?;
        self.repository
            .provision(username, display_name, hash, mode)
            .await
    }
    pub async fn setup_available(&self) -> anyhow::Result<bool> {
        self.repository.setup_available().await
    }

    #[tracing::instrument(name = "accounts.web.authenticate", skip_all, fields(ddd.bounded_context.name = "accounts", input.username.bytes = email.len(), output.outcome = tracing::field::Empty), err)]
    pub async fn authenticate_web(
        &self,
        email: &str,
        password: String,
    ) -> anyhow::Result<Option<i64>> {
        // Use the same Argon2 parameters for absent and present web credentials.
        const PLACEHOLDER_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$AAAAAAAAAAAAAAAAAAAAAA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let credentials = self.repository.web_credentials(email).await?;
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

    pub async fn open_web_session(
        &self,
        account_id: i64,
        digest: &[u8; 32],
        lifetime_seconds: i64,
    ) -> anyhow::Result<bool> {
        self.repository
            .open_web_session(account_id, digest, lifetime_seconds)
            .await
    }

    pub async fn find_web_session(
        &self,
        digest: &[u8; 32],
    ) -> anyhow::Result<Option<PlayerAccount>> {
        self.repository.find_web_session(digest).await
    }

    pub async fn close_web_session(&self, digest: &[u8; 32]) -> anyhow::Result<()> {
        self.repository.close_web_session(digest).await
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
    tokio::task::spawn_blocking(wot_platform::observability::in_current_span(move || {
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
