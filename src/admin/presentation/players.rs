use super::AdminState;
use crate::accounts::{
    CreatePlayerError, NewPlayer, NewPlayerError, PlayerAccount, PlayerPagination,
};
use axum::{
    Json,
    extract::{
        Query, State,
        rejection::{JsonRejection, QueryRejection},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Serialize)]
pub(super) struct PlayerRecord {
    id: String,
    username: String,
    display_name: String,
}

impl From<PlayerAccount> for PlayerRecord {
    fn from(player: PlayerAccount) -> Self {
        Self {
            id: player.id.to_string(),
            username: player.username,
            display_name: player.display_name,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PaginationQuery {
    page: Option<u32>,
    per_page: Option<u32>,
}

#[derive(Serialize)]
pub(super) struct PlayerList {
    data: Vec<PlayerRecord>,
    total: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CreatePlayerRequest {
    username: String,
    display_name: String,
    password: String,
}

#[tracing::instrument(name = "admin.players.list", skip_all, fields(bounded_context = "admin", input.page = tracing::field::Empty, input.per_page = tracing::field::Empty), err)]
pub(super) async fn list(
    State(state): State<AdminState>,
    query: Result<Query<PaginationQuery>, QueryRejection>,
) -> Result<Json<PlayerList>, ApiError> {
    let Query(query) = query.map_err(|_| ApiError::invalid_pagination())?;
    tracing::Span::current().record("input.page", query.page.unwrap_or(1));
    tracing::Span::current().record("input.per_page", query.per_page.unwrap_or(25));
    let pagination = PlayerPagination::new(query.page.unwrap_or(1), query.per_page.unwrap_or(25))
        .ok_or_else(ApiError::invalid_pagination)?;
    let page = state
        .accounts
        .list(pagination)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(PlayerList {
        data: page.accounts.into_iter().map(PlayerRecord::from).collect(),
        total: page.total,
    }))
}

#[tracing::instrument(name = "admin.players.create", skip_all, fields(bounded_context = "admin", input.username.bytes = tracing::field::Empty, input.display_name.bytes = tracing::field::Empty), err)]
pub(super) async fn create(
    State(state): State<AdminState>,
    body: Result<Json<CreatePlayerRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<PlayerRecord>), ApiError> {
    let Json(body) = body.map_err(|error| {
        ApiError::new(
            error.status(),
            match error.status() {
                StatusCode::PAYLOAD_TOO_LARGE => "The player request exceeds 8 KiB.",
                StatusCode::UNSUPPORTED_MEDIA_TYPE => {
                    "Send the player request as application/json."
                }
                _ => "Send a JSON object with username, display_name and password strings.",
            },
        )
    })?;
    tracing::Span::current().record("input.username.bytes", body.username.len());
    tracing::Span::current().record("input.display_name.bytes", body.display_name.len());
    let player = NewPlayer::new(body.username, body.display_name, body.password)?;
    let permit = state.create_slots.try_acquire_owned().map_err(|_| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Player creation is busy. Try again shortly.",
        )
    })?;
    let result = tokio::spawn(tracing::Instrument::in_current_span(async move {
        let _permit = permit;
        state.accounts.create(player).await
    }))
    .await
    .map_err(ApiError::internal)?;

    match result {
        Ok(player) => Ok((StatusCode::CREATED, Json(player.into()))),
        Err(CreatePlayerError::Duplicate) => Err(ApiError::field(
            StatusCode::CONFLICT,
            "username",
            "That login is already in use.",
        )),
        Err(CreatePlayerError::Internal(error)) => Err(ApiError::internal(error)),
    }
}

pub(super) struct ApiError {
    status: StatusCode,
    message: String,
    field: Option<&'static str>,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.status, self.message)
    }
}

impl ApiError {
    pub(super) fn new(status: StatusCode, message: &str) -> Self {
        Self {
            status,
            message: message.into(),
            field: None,
        }
    }

    fn field(status: StatusCode, field: &'static str, message: &str) -> Self {
        Self {
            field: Some(field),
            ..Self::new(status, message)
        }
    }

    fn invalid_pagination() -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "Use a positive page number and a per_page between 1 and 100.",
        )
    }

    pub(super) fn internal(error: impl std::fmt::Display) -> Self {
        tracing::error!(%error, "Admin player operation failed");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The player operation failed.",
        )
    }
}

impl From<NewPlayerError> for ApiError {
    fn from(error: NewPlayerError) -> Self {
        let field = match error {
            NewPlayerError::Username => "username",
            NewPlayerError::DisplayName => "display_name",
            NewPlayerError::Password => "password",
        };
        Self::field(StatusCode::UNPROCESSABLE_ENTITY, field, &error.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut body = json!({ "message": self.message });
        if let Some(field) = self.field {
            body["errors"] = json!({ (field): self.message });
        }
        (self.status, Json(body)).into_response()
    }
}
