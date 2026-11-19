use super::{AdminState, RequestSpan, access::lookup_session, players::ApiError};
use crate::{
    accounts::{
        AccountNotFound, AccountSnapshot, CosmeticGrantEntry, CosmeticKind, GarageCatalog,
        GrantCurrency, GrantError, PlayerGrant, ProgressionError, VehicleCatalogEntry,
    },
    player_sessions::{GrantDelivery, GrantRequestError},
    shared_kernel::AccountId,
};
use axum::{
    Extension, Json,
    extract::{Path, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize)]
pub(super) struct CosmeticGrantRecord {
    id: u16,
    name: String,
    max_owned: Option<u32>,
}

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
    inventory_id: i64,
    stat_trackers: BTreeMap<u16, u32>,
    key: String,
    name: String,
    tier: Option<i64>,
}

#[derive(Serialize)]
struct GarageSummary {
    display_name: String,
    credits: String,
    gold: String,
    bonds: String,
    free_xp: String,
    slots: i64,
}

impl From<&AccountSnapshot> for GarageSummary {
    fn from(snapshot: &AccountSnapshot) -> Self {
        Self {
            display_name: snapshot.display_name.clone(),
            credits: snapshot.balances.credits.to_string(),
            gold: snapshot.balances.gold.to_string(),
            bonds: snapshot.balances.bonds.to_string(),
            free_xp: snapshot.balances.free_xp.to_string(),
            slots: snapshot.balances.slots,
        }
    }
}

#[derive(Serialize)]
pub(super) struct Garage {
    #[serde(flatten)]
    summary: GarageSummary,
    vehicles: Vec<GarageVehicle>,
    attachments: BTreeMap<u16, u32>,
}

impl Garage {
    fn from_snapshot(snapshot: AccountSnapshot, catalog: &GarageCatalog) -> Self {
        let mut vehicles: Vec<_> = snapshot
            .vehicles
            .iter()
            .map(|vehicle| {
                let entry = catalog.vehicle_catalog_entry(&vehicle.catalog_key);
                let name = entry
                    .as_ref()
                    .map_or(vehicle.catalog_key.as_str(), |entry| entry.name)
                    .to_owned();
                let stat_trackers = catalog
                    .stat_tracker_options(&snapshot, vehicle.inventory_id)
                    .map(|(vehicle_type, eligible)| {
                        let owned = snapshot
                            .progression
                            .cosmetics
                            .stat_trackers()
                            .get(&vehicle_type);
                        eligible
                            .into_iter()
                            .map(|id| {
                                (
                                    id,
                                    owned
                                        .and_then(|items| items.get(&id))
                                        .copied()
                                        .unwrap_or_default(),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                GarageVehicle {
                    inventory_id: vehicle.inventory_id,
                    stat_trackers,
                    tier: entry.and_then(|entry| entry.tier),
                    key: vehicle.catalog_key.clone(),
                    name,
                }
            })
            .collect();
        vehicles.sort_unstable_by(|a, b| a.name.cmp(&b.name).then_with(|| a.key.cmp(&b.key)));
        Self {
            summary: GarageSummary::from(&snapshot),
            vehicles,
            attachments: snapshot
                .progression
                .cosmetics
                .owned()
                .get(&CosmeticKind::Attachment)
                .cloned()
                .unwrap_or_default(),
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum GrantBody {
    Currency { currency: Currency, amount: i64 },
    Vehicle { vehicle: i32 },
    Attachment { id: u16, count: u32 },
    StatTracker { vehicle: i64, id: u16, count: u32 },
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
        .and_then(AccountId::new)
        .map(AccountId::get)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "Player not found."))
}

#[tracing::instrument(name = "admin.garage.show", skip_all, fields(ddd.bounded_context.name = "player_administration", input.id.bytes = id.len()), err)]
pub(super) async fn show(
    State(state): State<AdminState>,
    Extension(RequestSpan(request_span)): Extension<RequestSpan>,
    Path(id): Path<String>,
) -> Result<Json<Garage>, ApiError> {
    let id = player_id(&id)?;
    request_span.record("input.account_id", id);
    let snapshot = read_garage(&state, id).await?;
    Ok(Json(Garage::from_snapshot(snapshot, &state.catalog)))
}

async fn read_garage(state: &AdminState, id: i64) -> Result<AccountSnapshot, ApiError> {
    state.garage_accounts.snapshot(id).await.map_err(|error| {
        if error.is::<AccountNotFound>() {
            ApiError::new(StatusCode::NOT_FOUND, "Player not found.")
        } else {
            ApiError::internal(error)
        }
    })
}

#[derive(Serialize)]
pub(super) struct PlayerGarage {
    #[serde(flatten)]
    summary: GarageSummary,
    vehicles: Vec<PlayerGarageVehicle>,
}

#[derive(Serialize)]
struct PlayerGarageVehicle {
    key: String,
    name: String,
    tier: Option<i64>,
}

#[tracing::instrument(name = "web_panel.garage.show_own", skip_all, fields(ddd.bounded_context.name = "player_administration", input.operation = "read_own_garage", user.id = tracing::field::Empty, output.vehicle_count = tracing::field::Empty), err)]
pub(super) async fn show_own(
    State(state): State<AdminState>,
    headers: HeaderMap,
) -> Result<Json<PlayerGarage>, ApiError> {
    let player = lookup_session(&state, &headers)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "Sign in to view your garage."))?;
    let span = tracing::Span::current();
    span.record("user.id", player.id);
    let snapshot = read_garage(&state, player.id).await?;
    let summary = GarageSummary::from(&snapshot);
    let mut vehicles: Vec<_> = snapshot
        .vehicles
        .into_iter()
        .map(|vehicle| {
            let catalog_entry = state.catalog.vehicle_catalog_entry(&vehicle.catalog_key);
            PlayerGarageVehicle {
                name: catalog_entry
                    .as_ref()
                    .map_or("Unknown tank", |entry| entry.name)
                    .to_owned(),
                tier: catalog_entry.and_then(|entry| entry.tier),
                key: vehicle.catalog_key,
            }
        })
        .collect();
    vehicles.sort_unstable_by(|a, b| a.name.cmp(&b.name).then_with(|| a.key.cmp(&b.key)));
    span.record("output.vehicle_count", vehicles.len());
    Ok(Json(PlayerGarage { summary, vehicles }))
}

#[tracing::instrument(name = "admin.garage.catalog", skip_all, fields(ddd.bounded_context.name = "player_administration", input.collection = "vehicles"))]
pub(super) async fn catalog(State(state): State<AdminState>) -> Json<Vec<VehicleRecord>> {
    let mut vehicles: Vec<_> = state
        .catalog
        .grantable_vehicles()
        .map(VehicleRecord::from)
        .collect();
    vehicles.sort_unstable_by(|a, b| a.name.cmp(&b.name).then_with(|| a.key.cmp(&b.key)));
    Json(vehicles)
}

#[tracing::instrument(
    name = "admin.garage.catalog",
    skip_all,
    fields(ddd.bounded_context.name = "player_administration", input.collection = "attachments", output.items.count = tracing::field::Empty)
)]
pub(super) async fn attachment_catalog(
    State(state): State<AdminState>,
) -> Json<Vec<CosmeticGrantRecord>> {
    cosmetic_grant_catalog(state.catalog.grantable_attachments())
}

#[tracing::instrument(
    name = "admin.garage.catalog",
    skip_all,
    fields(ddd.bounded_context.name = "player_administration", input.collection = "stat_trackers", output.items.count = tracing::field::Empty)
)]
pub(super) async fn stat_tracker_catalog(
    State(state): State<AdminState>,
) -> Json<Vec<CosmeticGrantRecord>> {
    cosmetic_grant_catalog(state.catalog.grantable_stat_trackers())
}

fn cosmetic_grant_catalog<'a>(
    entries: impl Iterator<Item = CosmeticGrantEntry<'a>>,
) -> Json<Vec<CosmeticGrantRecord>> {
    let mut items: Vec<_> = entries
        .map(|entry| CosmeticGrantRecord {
            id: entry.id,
            name: entry.name.to_owned(),
            max_owned: entry.max_owned,
        })
        .collect();
    items.sort_unstable_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    tracing::Span::current().record("output.items.count", items.len());
    Json(items)
}

#[tracing::instrument(name = "admin.garage.grant", skip_all, fields(ddd.bounded_context.name = "player_administration", input.id.bytes = id.len(), input.grant = tracing::field::Empty), err)]
pub(super) async fn grant(
    State(state): State<AdminState>,
    Extension(RequestSpan(request_span)): Extension<RequestSpan>,
    Path(id): Path<String>,
    body: Result<Json<GrantBody>, JsonRejection>,
) -> Result<Json<GrantResponse>, ApiError> {
    let id = player_id(&id)?;
    request_span.record("input.account_id", id);
    let Json(body) = body.map_err(|error| {
        ApiError::new(
            error.status(),
            match error.status() {
                StatusCode::PAYLOAD_TOO_LARGE => "The grant request exceeds 8 KiB.",
                StatusCode::UNSUPPORTED_MEDIA_TYPE => "Send the grant request as application/json.",
                _ => "Send a valid currency, vehicle or cosmetic grant.",
            },
        )
    })?;
    let grant = match body {
        GrantBody::Currency { currency, amount } => {
            let (currency, name) = match currency {
                Currency::Credits => (GrantCurrency::Credits, "credits"),
                Currency::Gold => (GrantCurrency::Gold, "gold"),
                Currency::Bonds => (GrantCurrency::Bonds, "bonds"),
                Currency::FreeXp => (GrantCurrency::FreeXp, "free_xp"),
            };
            request_span.record("input.grant.kind", "currency");
            request_span.record("input.grant.currency", name);
            request_span.record("input.amount", amount);
            PlayerGrant::Currency { currency, amount }
        }
        GrantBody::Attachment { id, count } => {
            request_span.record("input.grant.kind", "attachment");
            request_span.record("input.attachment_id", id);
            request_span.record("input.amount", count);
            PlayerGrant::Attachment { id, count }
        }
        GrantBody::StatTracker { vehicle, id, count } => {
            request_span.record("input.grant.kind", "stat_tracker");
            request_span.record("input.inventory_id", vehicle);
            request_span.record("input.stat_tracker_id", id);
            request_span.record("input.amount", count);
            PlayerGrant::StatTracker { vehicle, id, count }
        }
        GrantBody::Vehicle { vehicle } => {
            request_span.record("input.grant.kind", "vehicle");
            request_span.record("input.vehicle_type", vehicle);
            PlayerGrant::Vehicle { vehicle }
        }
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
