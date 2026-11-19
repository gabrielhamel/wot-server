use super::{
    AdminState, RequestSpan,
    players::{ApiError, PlayerRecord, creation_error},
};
use crate::accounts::contract::{NewPlayer, RegistrationError, is_registration_username};
use axum::{
    Extension, Json,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use tracing::Instrument;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RegistrationRequest {
    email: String,
    #[serde(rename = "username")]
    display_name: String,
    password: String,
    confirm_password: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UsernameRequest {
    #[serde(rename = "username")]
    display_name: String,
}

#[derive(Serialize)]
pub(super) struct UsernameAvailability {
    available: bool,
}

async fn require_registration(state: &AdminState) -> Result<(), ApiError> {
    if state
        .accounts
        .registration_enabled()
        .await
        .map_err(ApiError::internal)?
    {
        Ok(())
    } else {
        tracing::Span::current().record("output.outcome", "registration_closed");
        Err(ApiError::new(
            StatusCode::CONFLICT,
            "Registration opens after the first administrator is created.",
        ))
    }
}

#[tracing::instrument(name = "web_panel.registration.check_username", skip_all, fields(ddd.bounded_context.name = "player_administration", input.username.bytes = tracing::field::Empty, output.outcome = tracing::field::Empty, output.username.available = tracing::field::Empty), err)]
pub(super) async fn check_username(
    State(state): State<AdminState>,
    body: Result<Json<UsernameRequest>, JsonRejection>,
) -> Result<Json<UsernameAvailability>, ApiError> {
    let Json(body) =
        body.map_err(|error| ApiError::new(error.status(), "Send a username as JSON."))?;
    let span = tracing::Span::current();
    span.record("input.username.bytes", body.display_name.len());
    if !is_registration_username(&body.display_name) {
        return Err(registration_error(
            RegistrationError::Username,
            "email",
            "username",
        ));
    }
    let _permit = state.username_check_slots.try_acquire().map_err(|_| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Username checks are busy. Try again shortly.",
        )
    })?;
    require_registration(&state).await?;
    let available = state
        .accounts
        .registration_name_available(&body.display_name)
        .await
        .map_err(ApiError::internal)?;
    span.record("output.username.available", available);
    Ok(Json(UsernameAvailability { available }))
}

#[tracing::instrument(name = "web_panel.registration.create", skip_all, fields(ddd.bounded_context.name = "player_administration", input.username.bytes = tracing::field::Empty, input.email.bytes = tracing::field::Empty, output.outcome = tracing::field::Empty, user.id = tracing::field::Empty), err)]
pub(super) async fn register(
    State(state): State<AdminState>,
    Extension(RequestSpan(request_span)): Extension<RequestSpan>,
    body: Result<Json<RegistrationRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<PlayerRecord>), ApiError> {
    let Json(body) = body.map_err(|error| {
        ApiError::new(
            error.status(),
            "Send an email, username and both password fields as JSON.",
        )
    })?;
    let span = tracing::Span::current();
    for current in [&span, &request_span] {
        current.record("input.username.bytes", body.display_name.len());
        current.record("input.email.bytes", body.email.len());
    }
    let permit = state
        .create_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "Player creation is busy. Try again shortly.",
            )
        })?;
    require_registration(&state).await?;
    let player = NewPlayer::from_registration(
        body.email,
        body.display_name,
        body.password,
        &body.confirm_password,
    )
    .map_err(|error| registration_error(error, "email", "username"))?;

    let result = tokio::spawn(
        async move {
            let _permit = permit;
            state.accounts.register(player).await
        }
        .in_current_span(),
    )
    .await
    .map_err(ApiError::internal)?;
    match result {
        Ok(player) => {
            span.record("user.id", player.id);
            span.record("output.outcome", "created");
            Ok((StatusCode::CREATED, Json(player.into())))
        }
        Err(error) => {
            span.record("output.outcome", error.outcome());
            Err(creation_error(error, "email", "username"))
        }
    }
}

pub(super) fn registration_error(
    error: RegistrationError,
    email_field: &'static str,
    username_field: &'static str,
) -> ApiError {
    let (field, outcome) = match error {
        RegistrationError::Email => (email_field, "invalid_email"),
        RegistrationError::Username => (username_field, "invalid_username"),
        RegistrationError::DisplayName => (username_field, "invalid_display_name"),
        RegistrationError::Password => ("password", "invalid_password"),
        RegistrationError::PasswordConfirmation => ("confirm_password", "password_mismatch"),
    };
    tracing::Span::current().record("output.outcome", outcome);
    ApiError::field(StatusCode::UNPROCESSABLE_ENTITY, field, &error.to_string())
}
