use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PlayerRole {
    Player,
    Admin,
}

impl PlayerRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Player => "player",
            Self::Admin => "admin",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum RoleChange {
    Updated,
    Unchanged,
    ActorNotAdmin,
    OwnRole,
    PlayerNotFound,
    EmailRequired,
    LastAdmin,
}

impl RoleChange {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Updated => "updated",
            Self::Unchanged => "unchanged",
            Self::ActorNotAdmin => "actor_not_admin",
            Self::OwnRole => "own_role",
            Self::PlayerNotFound => "player_not_found",
            Self::EmailRequired => "email_required",
            Self::LastAdmin => "last_admin",
        }
    }
}

pub fn is_admin_email(username: &str) -> bool {
    (1..=128).contains(&username.len())
        && !username.starts_with('{')
        && !username
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
        && username.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty() && !domain.is_empty() && !domain.contains('@')
        })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerAccount {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    pub role: PlayerRole,
}

pub struct NewPlayer {
    username: String,
    display_name: String,
    password: String,
}

#[derive(Debug, Error)]
pub enum NewPlayerError {
    #[error("Enter a login of 1 to 128 bytes with no whitespace or control characters.")]
    Username,
    #[error(
        "Enter a display name of 1 to 128 characters with no control characters or surrounding whitespace."
    )]
    DisplayName,
    #[error("Enter a password of 1 to 1024 bytes with no control characters.")]
    Password,
}

impl NewPlayer {
    pub fn new(
        username: String,
        display_name: String,
        password: String,
    ) -> Result<Self, NewPlayerError> {
        if !(1..=128).contains(&username.len())
            || username.starts_with('{')
            || username
                .chars()
                .any(|c| c.is_whitespace() || c.is_control())
        {
            return Err(NewPlayerError::Username);
        }
        if !(1..=128).contains(&display_name.chars().count())
            || display_name.trim() != display_name
            || display_name.chars().any(char::is_control)
        {
            return Err(NewPlayerError::DisplayName);
        }
        if !(1..=1024).contains(&password.len()) || password.chars().any(char::is_control) {
            return Err(NewPlayerError::Password);
        }
        Ok(Self {
            username,
            display_name,
            password,
        })
    }

    pub(crate) fn into_parts(self) -> (String, String, String) {
        (self.username, self.display_name, self.password)
    }
}

pub struct PlayerPage {
    pub accounts: Vec<PlayerAccount>,
    pub total: i64,
}

pub struct PlayerPagination {
    offset: i64,
    limit: i64,
}

impl PlayerPagination {
    pub fn new(page: u32, per_page: u32) -> Option<Self> {
        if page == 0 || !(1..=100).contains(&per_page) {
            return None;
        }
        let limit = i64::from(per_page);
        Some(Self {
            offset: i64::from(page - 1) * limit,
            limit,
        })
    }

    pub(crate) fn offset(&self) -> i64 {
        self.offset
    }

    pub(crate) fn limit(&self) -> i64 {
        self.limit
    }
}
