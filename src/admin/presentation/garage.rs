use super::{AdminState, players::ApiError};
use crate::{
    access::{GrantDelivery, GrantRequestError},
    accounts::{
        AccountNotFound, AccountSnapshot, GarageCatalog, GrantCurrency, GrantError, PlayerGrant,
        ProgressionError, VehicleCatalogEntry,
    },
};
use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub(super) struct VehicleRecord {
    id: i32,
    key: String,
    name: String,
    tier: Option<i64>,
}

impl From<VehicleCatalogEntry<'_>> for VehicleRecord {
    fn from(entry: VehicleCatalogEntry<'_>) -> Self {
        Self {
            id: entry.id,
            key: entry.key.to_owned(),
            name: entry.name.to_owned(),
            tier: entry.tier,
        }
    }
}

#[derive(Serialize)]
struct GarageVehicle {
    key: String,
    name: String,
    tier: Option<i64>,
}

#[derive(Serialize)]
pub(super) struct Garage {
    display_name: String,
    credits: String,
    gold: String,
    bonds: String,
    free_xp: String,
    slots: i64,
    vehicles: Vec<GarageVehicle>,
}

impl Garage {
    fn from_snapshot(snapshot: AccountSnapshot, catalog: &GarageCatalog) -> Self {
        let mut vehicles: Vec<_> = snapshot
            .vehicles
            .into_iter()
            .map(|vehicle| {
                let entry = catalog.vehicle_catalog_entry(&vehicle.catalog_key);
                let name = entry
                    .as_ref()
                    .map_or(vehicle.catalog_key.as_str(), |entry| entry.name)
                    .to_owned();
                GarageVehicle {
                    tier: entry.and_then(|entry| entry.tier),
                    key: vehicle.catalog_key,
                    name,
                }
            })
            .collect();
        vehicles.sort_unstable_by(|a, b| a.name.cmp(&b.name).then_with(|| a.key.cmp(&b.key)));
        Self {
            display_name: snapshot.display_name,
            credits: snapshot.balances.credits.to_string(),
            gold: snapshot.balances.gold.to_string(),
            bonds: snapshot.balances.bonds.to_string(),
            free_xp: snapshot.balances.free_xp.to_string(),
            slots: snapshot.balances.slots,
            vehicles,
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum GrantBody {
    Currency { currency: Currency, amount: i64 },
    Vehicle { vehicle: i32 },
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Currency {
    Credits,
    Gold,
    Bonds,
    FreeXp,
}

#[derive(Serialize)]
pub(super) struct GrantResponse {
    garage: Garage,
    delivery: &'static str,
}

fn player_id(id: &str) -> Result<i64, ApiError> {
    id.parse::<i64>()
        .ok()
        .filter(|&id| id > 0)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "Player not found."))
}

#[tracing::instrument(name = "admin.garage.show", skip_all, fields(wot.context = "admin", input.id.bytes = id.len()), err)]
pub(super) async fn show(
    State(state): State<AdminState>,
    Path(id): Path<String>,
) -> Result<Json<Garage>, ApiError> {
    let snapshot = state
        .garage_accounts
        .snapshot(player_id(&id)?)
        .await
        .map_err(|error| {
            if error.is::<AccountNotFound>() {
                ApiError::new(StatusCode::NOT_FOUND, "Player not found.")
            } else {
                ApiError::internal(error)
            }
        })?;
    Ok(Json(Garage::from_snapshot(snapshot, &state.catalog)))
}

#[tracing::instrument(name = "admin.garage.catalog", skip_all, fields(wot.context = "admin", input.collection = "vehicles"))]
pub(super) async fn catalog(State(state): State<AdminState>) -> Json<Vec<VehicleRecord>> {
    let mut vehicles: Vec<_> = state
        .catalog
        .grantable_vehicles()
        .map(VehicleRecord::from)
        .collect();
    vehicles.sort_unstable_by(|a, b| a.name.cmp(&b.name).then_with(|| a.key.cmp(&b.key)));
    Json(vehicles)
}

#[tracing::instrument(name = "admin.garage.grant", skip_all, fields(wot.context = "admin", input.id.bytes = id.len(), input.grant = tracing::field::Empty), err)]
pub(super) async fn grant(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    body: Result<Json<GrantBody>, JsonRejection>,
) -> Result<Json<GrantResponse>, ApiError> {
    let id = player_id(&id)?;
    let Json(body) = body.map_err(|error| {
        ApiError::new(
            error.status(),
            match error.status() {
                StatusCode::PAYLOAD_TOO_LARGE => "The grant request exceeds 8 KiB.",
                StatusCode::UNSUPPORTED_MEDIA_TYPE => "Send the grant request as application/json.",
                _ => "Send a valid currency amount or catalog vehicle ID.",
            },
        )
    })?;
    let grant = match body {
        GrantBody::Currency { currency, amount } => {
            let currency = match currency {
                Currency::Credits => GrantCurrency::Credits,
                Currency::Gold => GrantCurrency::Gold,
                Currency::Bonds => GrantCurrency::Bonds,
                Currency::FreeXp => GrantCurrency::FreeXp,
            };
            PlayerGrant::Currency { currency, amount }
        }
        GrantBody::Vehicle { vehicle } => PlayerGrant::Vehicle { vehicle },
    };
    tracing::Span::current().record("input.grant", tracing::field::debug(grant));
    let (snapshot, delivery) = state.grants.grant(id, grant).await.map_err(grant_error)?;
    Ok(Json(GrantResponse {
        garage: Garage::from_snapshot(snapshot, &state.catalog),
        delivery: match delivery {
            GrantDelivery::QueuedForGarage => "queued_for_garage",
            GrantDelivery::OnNextSync => "on_next_sync",
        },
    }))
}

fn grant_error(error: GrantRequestError) -> ApiError {
    let message = error.to_string();
    let (status, message) = match error {
        GrantRequestError::NotFound => (StatusCode::NOT_FOUND, message.as_str()),
        GrantRequestError::Busy => (StatusCode::CONFLICT, message.as_str()),
        GrantRequestError::Unavailable => (StatusCode::SERVICE_UNAVAILABLE, message.as_str()),
        GrantRequestError::Internal(error) => return ApiError::internal(error),
        GrantRequestError::Rejected(GrantError::Vehicle(vehicle)) => match vehicle {
            ProgressionError::VehicleNotFound => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "Select a tank eligible for Random Battles.",
            ),
            ProgressionError::VehicleAlreadyOwned => {
                (StatusCode::CONFLICT, "The player already owns this tank.")
            }
            ProgressionError::NoFreeSlot => (
                StatusCode::CONFLICT,
                "The player's garage has no free slot.",
            ),
            ProgressionError::NoFreeBerth => (
                StatusCode::CONFLICT,
                "The account has reached its crew capacity.",
            ),
            ProgressionError::CatalogUnavailable => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "The installed catalog cannot provision this tank.",
            ),
            _ => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "This grant could not be applied to the player.",
            ),
        },
        GrantRequestError::Rejected(_) => (StatusCode::UNPROCESSABLE_ENTITY, message.as_str()),
    };
    ApiError::new(status, message)
}
