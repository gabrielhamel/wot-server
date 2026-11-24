use super::{AdminState, garage::VehicleRecord, players::ApiError};
use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
};
use serde::Serialize;
use wot_accounts::contract::DummyLoadoutOptions;
use wot_player_sessions::contract::{
    AdminBattleError, AdminBattleSetup, AdminBattles, BattleLaunchReceipt, BattleOptions,
};

#[derive(Serialize)]
pub(super) struct Options {
    #[serde(flatten)]
    arena: BattleOptions,
    dummy_vehicles: Vec<VehicleRecord>,
}

fn service(state: &AdminState) -> Result<&AdminBattles, ApiError> {
    state
        .battles
        .as_ref()
        .ok_or_else(|| battle_error(AdminBattleError::Unavailable))
}

fn battle_error(error: AdminBattleError) -> ApiError {
    let status = match error {
        AdminBattleError::InvalidSetup | AdminBattleError::InvalidDummy => StatusCode::BAD_REQUEST,
        AdminBattleError::PlayerUnavailable => StatusCode::CONFLICT,
        AdminBattleError::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        AdminBattleError::PreparationFailed => StatusCode::INTERNAL_SERVER_ERROR,
    };
    let span = tracing::Span::current();
    span.record("output.outcome", error.outcome());
    if status.is_server_error() {
        span.record("otel.status_code", "ERROR");
        tracing::error!(outcome = error.outcome(), "Admin battle request failed");
    }
    ApiError::new(status, &error.to_string())
}

#[tracing::instrument(name = "admin.battle_options", skip_all, fields(
    ddd.bounded_context.name = "player_administration", input.operation = "options",
    output.maps.count = tracing::field::Empty, output.players.count = tracing::field::Empty,
    output.vehicles.count = tracing::field::Empty, output.outcome = tracing::field::Empty,
    otel.status_code = tracing::field::Empty,
), err)]
pub(super) async fn options(State(state): State<AdminState>) -> Result<Json<Options>, ApiError> {
    let arena = service(&state)?.options().await.map_err(battle_error)?;
    let dummy_vehicles: Vec<_> = state
        .catalog
        .grantable_vehicles()
        .map(VehicleRecord::from)
        .collect();
    let span = tracing::Span::current();
    span.record("output.maps.count", arena.maps.len());
    span.record("output.players.count", arena.players.len());
    span.record("output.vehicles.count", dummy_vehicles.len());
    span.record("output.outcome", "loaded");
    Ok(Json(Options {
        arena,
        dummy_vehicles,
    }))
}

#[tracing::instrument(name = "admin.dummy_loadout", skip_all, fields(
    ddd.bounded_context.name = "player_administration", input.vehicle_type = vehicle,
    output.outcome = tracing::field::Empty, otel.status_code = tracing::field::Empty,
), err)]
pub(super) async fn loadout(
    State(state): State<AdminState>,
    Path(vehicle): Path<i32>,
) -> Result<Json<DummyLoadoutOptions>, ApiError> {
    let span = tracing::Span::current();
    let permit = state
        .battle_loadout_slots
        .try_acquire_owned()
        .map_err(|_| battle_error(AdminBattleError::Unavailable))?;
    let options =
        tokio::task::spawn_blocking(wot_platform::observability::in_current_span(move || {
            let _permit = permit;
            state.catalog.dummy_loadout_options(vehicle)
        }))
        .await
        .map_err(loadout_error)?
        .map_err(loadout_error)?;
    let Some(options) = options else {
        span.record("output.outcome", "vehicle_unavailable");
        return Err(ApiError::new(StatusCode::NOT_FOUND, "Vehicle unavailable."));
    };
    span.record("output.outcome", "loaded");
    Ok(Json(options))
}

fn loadout_error(error: impl std::fmt::Display) -> ApiError {
    let span = tracing::Span::current();
    span.record("output.outcome", "loadout_failed");
    span.record("otel.status_code", "ERROR");
    tracing::error!(%error, "Could not load dummy vehicle options");
    ApiError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Could not load dummy vehicle options.",
    )
}

#[tracing::instrument(name = "admin.start_battle", skip_all, fields(
    ddd.bounded_context.name = "player_administration", input.arena_type_id = tracing::field::Empty,
    input.round_length.seconds = tracing::field::Empty, input.players.count = tracing::field::Empty,
    input.dummies.count = tracing::field::Empty, output.battle.id = tracing::field::Empty,
    output.players.handoff_count = tracing::field::Empty,
    output.outcome = tracing::field::Empty, otel.status_code = tracing::field::Empty,
), err)]
pub(super) async fn start(
    State(state): State<AdminState>,
    body: Result<Json<AdminBattleSetup>, JsonRejection>,
) -> Result<Json<BattleLaunchReceipt>, ApiError> {
    let span = tracing::Span::current();
    let Json(setup) = body.map_err(|error| {
        span.record("output.outcome", "invalid_payload");
        ApiError::new(error.status(), "Send a valid battle setup as JSON.")
    })?;
    span.record("input.arena_type_id", setup.arena_type);
    span.record("input.round_length.seconds", setup.duration_seconds);
    span.record("input.players.count", setup.players.len());
    span.record("input.dummies.count", setup.dummies.len());
    let requested = setup.players.len();
    let receipt = service(&state)?.start(setup).await.map_err(battle_error)?;
    span.record("output.battle.id", receipt.battle_id);
    span.record("output.players.handoff_count", receipt.players_handed_off);
    span.record(
        "output.outcome",
        if receipt.players_handed_off == requested {
            "launched"
        } else {
            "launched_partial"
        },
    );
    Ok(Json(receipt))
}
