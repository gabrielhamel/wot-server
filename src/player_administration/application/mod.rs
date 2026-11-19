use crate::accounts::{PlayerAccessRepository, PlayerAccounts, is_player_login};
use argon2::password_hash::rand_core::{OsRng, RngCore};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

pub(super) const SESSION_SECONDS: i64 = 60 * 24 * 60 * 60;

pub(super) struct SessionSecret(String);

impl SessionSecret {
    pub fn parse(value: &str) -> Option<Self> {
        if value.len() != 43 || URL_SAFE_NO_PAD.decode(value).ok()?.len() != 32 {
            return None;
        }
        Some(Self(value.to_owned()))
    }

    fn generate() -> anyhow::Result<Self> {
        let mut bytes = [0; 32];
        OsRng
            .try_fill_bytes(&mut bytes)
            .map_err(|error| anyhow::anyhow!("Generate web session secret: {error}"))?;
        Ok(Self(URL_SAFE_NO_PAD.encode(bytes)))
    }

    pub fn value(&self) -> &str {
        &self.0
    }

    pub fn digest(&self) -> [u8; 32] {
        Sha256::digest(self.0.as_bytes()).into()
    }
}

#[tracing::instrument(name = "admin.verify_credentials", skip_all, fields(ddd.bounded_context.name = "player_administration", input.username.bytes = login.len(), output.outcome = tracing::field::Empty, user.id = tracing::field::Empty), err)]
pub(super) async fn sign_in<R: PlayerAccessRepository>(
    players: &PlayerAccounts<R>,
    login: &str,
    password: String,
) -> anyhow::Result<Option<SessionSecret>> {
    let span = tracing::Span::current();
    if !is_player_login(login)
        || !(1..=1024).contains(&password.len())
        || password.chars().any(char::is_control)
    {
        span.record("output.outcome", "rejected");
        return Ok(None);
    }
    let Some(id) = players.authenticate_web(login, password).await? else {
        span.record("output.outcome", "rejected");
        return Ok(None);
    };
    let secret = SessionSecret::generate()?;
    if !players
        .open_web_session(id, &secret.digest(), SESSION_SECONDS)
        .await?
    {
        span.record("output.outcome", "rejected");
        return Ok(None);
    }
    span.record("user.id", id);
    span.record("output.outcome", "authorized");
    Ok(Some(secret))
}
