use serde::{Deserialize, Serialize};
use thiserror::Error;
use unicode_properties::{GeneralCategory, GeneralCategoryGroup, UnicodeGeneralCategory};

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

pub fn is_player_login(username: &str) -> bool {
    (1..=128).contains(&username.len())
        && !username.starts_with('{')
        && !username
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
}

pub fn is_admin_email(username: &str) -> bool {
    is_player_login(username)
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
        if !is_player_login(&username) {
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

#[derive(Debug, Error)]
pub enum RegistrationError {
    #[error("Enter a valid email address of up to 128 characters.")]
    Email,
    #[error("Use 3 to 24 letters, numbers or underscores for your username.")]
    Username,
    #[error(
        "Use 8 to 128 characters, including at least one digit, one uppercase letter, one lowercase letter, and one symbol. Control characters are not allowed."
    )]
    Password,
    #[error("The passwords do not match.")]
    PasswordConfirmation,
}

impl NewPlayer {
    #[tracing::instrument(name = "accounts.registration.validate", skip_all, fields(ddd.bounded_context.name = "accounts", input.username.bytes = display_name.len(), input.email.bytes = email.len()), err)]
    pub fn from_registration(
        email: String,
        display_name: String,
        password: String,
        confirm_password: &str,
    ) -> Result<Self, RegistrationError> {
        if !is_registration_email(&email) {
            return Err(RegistrationError::Email);
        }
        if !is_registration_username(&display_name) {
            return Err(RegistrationError::Username);
        }
        if !is_registration_password(&password) {
            return Err(RegistrationError::Password);
        }
        if password != confirm_password {
            return Err(RegistrationError::PasswordConfirmation);
        }
        Ok(Self {
            username: email,
            display_name,
            password,
        })
    }
}

fn is_registration_password(password: &str) -> bool {
    let characters = password.chars();
    let categories = characters.clone().map(char::general_category);
    (8..=128).contains(&characters.count())
        && !password.chars().any(char::is_control)
        && categories
            .clone()
            .any(|category| category == GeneralCategory::DecimalNumber)
        && categories
            .clone()
            .any(|category| category == GeneralCategory::UppercaseLetter)
        && categories
            .clone()
            .any(|category| category == GeneralCategory::LowercaseLetter)
        && password.chars().any(|character| {
            matches!(
                character.general_category_group(),
                GeneralCategoryGroup::Punctuation | GeneralCategoryGroup::Symbol
            )
        })
}

pub fn is_registration_username(username: &str) -> bool {
    (3..=24).contains(&username.len())
        && username
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn is_registration_email(email: &str) -> bool {
    if !is_admin_email(email) {
        return false;
    }
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    local.len() <= 64
        && local.split('.').all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || b"!#$%&'*+-/=?^_`{|}~".contains(&byte)
                })
        })
        && domain.contains('.')
        && domain.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
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
