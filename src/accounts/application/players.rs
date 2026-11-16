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

    pub async fn list(&self, pagination: PlayerPagination) -> anyhow::Result<PlayerPage> {
        self.repository.list(pagination).await
    }

    pub async fn create(&self, player: NewPlayer) -> Result<PlayerAccount, CreatePlayerError> {
        let (username, display_name, password) = player.into_parts();
        let hash = hash_password(password).await?;
        self.repository
            .provision(username, display_name, hash)
            .await?
            .ok_or(CreatePlayerError::Duplicate)
    }
}

pub(in crate::accounts) async fn hash_password(password: String) -> anyhow::Result<String> {
    use argon2::{
        Argon2, PasswordHasher,
        password_hash::{SaltString, rand_core::OsRng},
    };
    tokio::task::spawn_blocking(move || {
        Argon2::default()
            .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
            .map(|hash| hash.to_string())
            .map_err(|error| anyhow::anyhow!("Hash account password: {error}"))
    })
    .await?
}
