use super::{
    AdminState, RequestSpan,
    players::{ApiError, PlayerRecord},
};
use crate::application::{self, SESSION_SECONDS, SessionSecret};
use axum::{
    Extension, Json,
    extract::{Path, Request, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use tracing::Instrument;
use wot_accounts::contract::{PlayerAccount, PlayerRole, RoleChange};
use wot_shared_kernel::AccountId;
use wot_store::contract::DashboardStorePlayer;

const COOKIE_NAME: &str = "wot_admin_session";

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(super) enum Session {
    SetupRequired,
    SignedOut {
        registration_open: bool,
    },
    Authenticated {
        player: PlayerRecord,
        store_enabled: bool,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Credentials {
    email: String,
    password: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RoleRequest {
    role: PlayerRole,
}

pub(super) async fn protect_writes(request: Request, next: Next) -> Response {
    if request.method() == Method::POST && !validate_write_header(request.headers()) {
        return ApiError::new(
            StatusCode::FORBIDDEN,
            "The admin request header is required.",
        )
        .into_response();
    }
    next.run(request).await
}

#[tracing::instrument(name = "admin.check_write", skip_all, fields(ddd.bounded_context.name = "player_administration", input.operation = "post", output.outcome = tracing::field::Empty))]
fn validate_write_header(headers: &HeaderMap) -> bool {
    let valid = headers.get("x-wot-admin").is_some_and(|value| value == "1");
    tracing::Span::current().record(
        "output.outcome",
        if valid { "authorized" } else { "rejected" },
    );
    valid
}

pub(super) async fn authenticate(
    State(state): State<AdminState>,
    mut request: Request,
    next: Next,
) -> Response {
    match lookup_session(&state, request.headers()).await {
        Ok(Some(admin)) if admin.role == PlayerRole::Admin => {
            request.extensions_mut().insert(admin);
            next.run(request).await
        }
        Ok(_) => {
            ApiError::new(StatusCode::UNAUTHORIZED, "Sign in as an administrator.").into_response()
        }
        Err(error) => error.into_response(),
    }
}

pub(super) async fn authenticate_store_player(
    State(state): State<AdminState>,
    mut request: Request,
    next: Next,
) -> Response {
    match lookup_session(&state, request.headers()).await {
        Ok(Some(player)) => {
            request
                .extensions_mut()
                .insert(DashboardStorePlayer(player.id));
            next.run(request).await
        }
        Ok(None) => (StatusCode::UNAUTHORIZED, "Sign in to open the Store.").into_response(),
        Err(error) => error.into_response(),
    }
}

#[tracing::instrument(name = "admin.authenticate", skip_all, fields(ddd.bounded_context.name = "player_administration", input.session.present = tracing::field::Empty, output.outcome = tracing::field::Empty, user.id = tracing::field::Empty), err)]
pub(super) async fn lookup_session(
    state: &AdminState,
    headers: &HeaderMap,
) -> Result<Option<PlayerAccount>, ApiError> {
    let secret = cookie_secret(headers);
    let span = tracing::Span::current();
    span.record("input.session.present", secret.is_some());
    let player = match secret {
        Some(secret) => state
            .accounts
            .find_web_session(&secret.digest())
            .await
            .map_err(ApiError::internal)?,
        None => None,
    };
    if let Some(player) = &player {
        span.record("user.id", player.id);
    }
    span.record(
        "output.outcome",
        if player.is_some() {
            "authorized"
        } else {
            "rejected"
        },
    );
    Ok(player)
}

#[tracing::instrument(name = "admin.session.status", skip_all, fields(ddd.bounded_context.name = "player_administration", input.operation = "status", output.registration_open = tracing::field::Empty, output.outcome = tracing::field::Empty), err)]
pub(super) async fn session(
    State(state): State<AdminState>,
    headers: HeaderMap,
) -> Result<Json<Session>, ApiError> {
    let session = if let Some(player) = lookup_session(&state, &headers).await? {
        Session::Authenticated {
            player: player.into(),
            store_enabled: state.store_enabled,
        }
    } else if state
        .accounts
        .setup_available()
        .await
        .map_err(ApiError::internal)?
    {
        Session::SetupRequired
    } else {
        let registration_open = state
            .accounts
            .registration_enabled()
            .await
            .map_err(ApiError::internal)?;
        tracing::Span::current().record("output.registration_open", registration_open);
        Session::SignedOut { registration_open }
    };
    tracing::Span::current().record(
        "output.outcome",
        match &session {
            Session::SetupRequired => "setup_required",
            Session::SignedOut { .. } => "signed_out",
            Session::Authenticated { .. } => "authenticated",
        },
    );
    Ok(Json(session))
}

#[tracing::instrument(name = "admin.sign_in", skip_all, fields(ddd.bounded_context.name = "player_administration", input.username.bytes = tracing::field::Empty, output.outcome = tracing::field::Empty), err)]
pub(super) async fn sign_in(
    State(state): State<AdminState>,
    Extension(RequestSpan(request_span)): Extension<RequestSpan>,
    body: Result<Json<Credentials>, JsonRejection>,
) -> Result<impl IntoResponse, ApiError> {
    let Json(body) =
        body.map_err(|error| ApiError::new(error.status(), "Send an email and password as JSON."))?;
    request_span.record("input.username.bytes", body.email.len());
    tracing::Span::current().record("input.username.bytes", body.email.len());
    let permit = state.sign_in_slots.try_acquire_owned().map_err(|_| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Sign-in is busy. Try again shortly.",
        )
    })?;
    let secret = tokio::spawn(
        async move {
            let _permit = permit;
            application::sign_in(&state.accounts, &body.email, body.password).await
        }
        .in_current_span(),
    )
    .await
    .map_err(ApiError::internal)?
    .map_err(ApiError::internal)?;
    let Some(secret) = secret else {
        tracing::Span::current().record("output.outcome", "rejected");
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "Invalid email or password.",
        ));
    };
    tracing::Span::current().record("output.outcome", "authorized");
    Ok((
        StatusCode::NO_CONTENT,
        [(
            header::SET_COOKIE,
            session_cookie(secret.value(), SESSION_SECONDS, state.secure_cookies)?,
        )],
    ))
}

#[tracing::instrument(name = "admin.sign_out", skip_all, fields(ddd.bounded_context.name = "player_administration", input.session.present = tracing::field::Empty), err)]
pub(super) async fn sign_out(
    State(state): State<AdminState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let secret = cookie_secret(&headers);
    tracing::Span::current().record("input.session.present", secret.is_some());
    if let Some(secret) = secret {
        state
            .accounts
            .close_web_session(&secret.digest())
            .await
            .map_err(ApiError::internal)?;
    }
    Ok((
        StatusCode::NO_CONTENT,
        [(
            header::SET_COOKIE,
            session_cookie("", 0, state.secure_cookies)?,
        )],
    ))
}

#[tracing::instrument(name = "admin.players.change_role", skip_all, fields(ddd.bounded_context.name = "player_administration", user.id = actor.id, input.account_id = tracing::field::Empty, input.role = tracing::field::Empty, output.outcome = tracing::field::Empty), err)]
pub(super) async fn change_role(
    State(state): State<AdminState>,
    Extension(actor): Extension<PlayerAccount>,
    Extension(RequestSpan(request_span)): Extension<RequestSpan>,
    Path(id): Path<String>,
    body: Result<Json<RoleRequest>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let id = id
        .parse()
        .ok()
        .and_then(AccountId::new)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "Player not found."))?
        .get();
    let Json(body) = body.map_err(|error| {
        ApiError::new(error.status(), "Choose the Player or Administrator role.")
    })?;
    let span = tracing::Span::current();
    request_span.record("input.account_id", id);
    request_span.record("input.role", body.role.as_str());
    span.record("input.account_id", id);
    span.record("input.role", body.role.as_str());
    let result = state
        .accounts
        .change_role(actor.id, id, body.role)
        .await
        .map_err(ApiError::internal)?;
    span.record("output.outcome", result.as_str());
    match result {
        RoleChange::Updated | RoleChange::Unchanged => Ok(StatusCode::NO_CONTENT),
        RoleChange::ActorNotAdmin => Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "Sign in as an administrator.",
        )),
        RoleChange::OwnRole => Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "You cannot change your own role.",
        )),
        RoleChange::PlayerNotFound => {
            Err(ApiError::new(StatusCode::NOT_FOUND, "Player not found."))
        }
        RoleChange::EmailRequired => Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "This player's game login must be an email address to become an administrator.",
        )),
        RoleChange::LastAdmin => Err(ApiError::new(
            StatusCode::CONFLICT,
            "The server must have at least one administrator.",
        )),
    }
}

fn cookie_secret(headers: &HeaderMap) -> Option<SessionSecret> {
    let mut values = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .filter_map(|(name, value)| (name == COOKIE_NAME).then_some(value));
    let secret = SessionSecret::parse(values.next()?)?;
    values.next().is_none().then_some(secret)
}

fn session_cookie(secret: &str, max_age: i64, secure: bool) -> Result<HeaderValue, ApiError> {
    let secure = if secure { "; Secure" } else { "" };
    HeaderValue::from_str(&format!("{COOKIE_NAME}={secret}; Path=/api/admin; Max-Age={max_age}; HttpOnly; SameSite=Strict{secure}"))
        .map_err(ApiError::internal)
}
