use crate::accounts::{NewPlayer, PlayerAccount, PlayerPage, PlayerPagination};
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
    ) -> impl Future<Output = anyhow::Result<Option<PlayerAccount>>> + Send;
}

#[derive(Debug, Error)]
pub enum CreatePlayerError {
    #[error("That login is already in use.")]
    Duplicate,
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

    #[tracing::instrument(name = "accounts.application.players.list", skip_all, fields(code.layer = "application", wot.context = "accounts", input.offset = pagination.offset(), input.limit = pagination.limit()), err)]
    pub async fn list(&self, pagination: PlayerPagination) -> anyhow::Result<PlayerPage> {
        self.repository.list(pagination).await
    }

    #[tracing::instrument(name = "accounts.application.players.create", skip_all, fields(code.layer = "application", wot.context = "accounts", input.username.bytes = tracing::field::Empty, input.display_name.bytes = tracing::field::Empty), err)]
    pub async fn create(&self, player: NewPlayer) -> Result<PlayerAccount, CreatePlayerError> {
        let (username, display_name, password) = player.into_parts();
        tracing::Span::current().record("input.username.bytes", username.len());
        tracing::Span::current().record("input.display_name.bytes", display_name.len());
        let hash = hash_password(password).await?;
        self.repository
            .provision(username, display_name, hash)
            .await?
            .ok_or(CreatePlayerError::Duplicate)
    }
}

#[tracing::instrument(name = "accounts.application.players.hash_password", skip_all, fields(code.layer = "application", wot.context = "accounts", input.algorithm = "argon2id"), err)]
pub(in crate::accounts) async fn hash_password(password: String) -> anyhow::Result<String> {
    use argon2::{
        Argon2, PasswordHasher,
        password_hash::{SaltString, rand_core::OsRng},
    };
    tokio::task::spawn_blocking(crate::observability::in_current_span(move || {
        Argon2::default()
            .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
            .map(|hash| hash.to_string())
            .map_err(|error| anyhow::anyhow!("Hash account password: {error}"))
    }))
    .await?
}
