use serde::Serialize;
use std::collections::BTreeMap;
use tokio::sync::{mpsc, oneshot};
use wot_accounts::contract::{
    BattleVehicleChoice, BattleVehicleUnavailable, CustomBattleLoadout, CustomBattleLoadoutOptions,
};
use wot_battles::contract::BattleParameterDefinition;

#[derive(Clone)]
pub struct TrainingBattleLaunches(mpsc::Sender<BattleRequest>);

pub(crate) enum BattleRequest {
    Options(tracing::Span, oneshot::Sender<BattleOptions>),
    OwnedLoadout(
        tracing::Span,
        i64,
        i64,
        oneshot::Sender<Result<CustomBattleLoadoutOptions, TrainingBattleError>>,
    ),
    Launch(
        tracing::Span,
        TrainingBattleSetup,
        oneshot::Sender<Result<BattleLaunchReceipt, TrainingBattleError>>,
    ),
}

#[derive(Serialize)]
pub struct BattleLaunchReceipt {
    pub battle_id: u32,
    pub players_handed_off: usize,
}

#[derive(Serialize)]
pub struct BattleOptions {
    pub maps: Vec<BattleChoice<i32>>,
    pub players: Vec<AvailablePlayer>,
    pub observer_available: bool,
    pub parameters: &'static [BattleParameterDefinition],
}

#[derive(Serialize)]
pub struct BattleChoice<T> {
    pub id: T,
    pub name: String,
}

#[derive(Serialize)]
pub struct AvailablePlayer {
    pub id: i64,
    pub name: String,
    pub unavailable_reason: Option<&'static str>,
    pub vehicles: Vec<BattleVehicleChoice>,
}

#[derive(Debug)]
pub struct TrainingBattleSetup {
    pub arena_type: i32,
    pub duration_seconds: i32,
    pub players: Vec<TrainingBattlePlayer>,
    pub dummies: Vec<TrainingBattleDummy>,
    pub parameters: BTreeMap<String, f64>,
}

#[derive(Debug)]
pub struct TrainingBattlePlayer {
    pub account_id: i64,
    pub team: u8,
    pub inventory_id: Option<i64>,
    pub loadout: Option<CustomBattleLoadout>,
    pub owned_overrides: Option<CustomBattleLoadout>,
}

#[derive(Debug)]
pub struct TrainingBattleDummy {
    pub team: u8,
    pub loadout: CustomBattleLoadout,
}

#[derive(Clone, Copy, Debug, thiserror::Error)]
pub enum TrainingBattleError {
    #[error(
        "Select a map, a duration from 5 to 30 minutes, and vehicles on both teams. Include at least one connected player and no more than 30 entrants."
    )]
    InvalidSetup,
    #[error("A battle parameter is unknown or outside its supported range.")]
    InvalidParameters,
    #[error(
        "A selected player or vehicle is no longer available. Refresh the player list and try again."
    )]
    PlayerUnavailable,
    #[error(
        "Select an owned vehicle with compatible edits, a custom tank, or Observer for each player."
    )]
    InvalidPlayerLoadout,
    #[error("{0}. Refresh the player list and try again.")]
    VehicleUnavailable(BattleVehicleUnavailable),
    #[error("A dummy has an unavailable vehicle or incompatible loadout.")]
    InvalidDummy,
    #[error("The battle server is busy or unavailable. Try again shortly.")]
    Unavailable,
    #[error("The arena could not be prepared.")]
    PreparationFailed,
}

impl TrainingBattleError {
    pub fn outcome(self) -> &'static str {
        match self {
            Self::InvalidSetup => "invalid_setup",
            Self::InvalidParameters => "invalid_parameters",
            Self::PlayerUnavailable => "player_unavailable",
            Self::InvalidPlayerLoadout => "invalid_player_loadout",
            Self::VehicleUnavailable(reason) => reason.outcome(),
            Self::InvalidDummy => "invalid_dummy",
            Self::Unavailable => "unavailable",
            Self::PreparationFailed => "preparation_failed",
        }
    }
}

impl TrainingBattleLaunches {
    pub(crate) fn channel() -> (Self, mpsc::Receiver<BattleRequest>) {
        let (sender, receiver) = mpsc::channel(4);
        (Self(sender), receiver)
    }

    pub async fn options(&self) -> Result<BattleOptions, TrainingBattleError> {
        let (reply, response) = oneshot::channel();
        self.0
            .try_send(BattleRequest::Options(tracing::Span::current(), reply))
            .map_err(|_| TrainingBattleError::Unavailable)?;
        response.await.map_err(|_| TrainingBattleError::Unavailable)
    }

    pub async fn owned_loadout(
        &self,
        account_id: i64,
        inventory_id: i64,
    ) -> Result<CustomBattleLoadoutOptions, TrainingBattleError> {
        let (reply, response) = oneshot::channel();
        self.0
            .try_send(BattleRequest::OwnedLoadout(
                tracing::Span::current(),
                account_id,
                inventory_id,
                reply,
            ))
            .map_err(|_| TrainingBattleError::Unavailable)?;
        response
            .await
            .map_err(|_| TrainingBattleError::Unavailable)?
    }

    pub async fn start(
        &self,
        setup: TrainingBattleSetup,
    ) -> Result<BattleLaunchReceipt, TrainingBattleError> {
        let (reply, response) = oneshot::channel();
        self.0
            .try_send(BattleRequest::Launch(
                tracing::Span::current(),
                setup,
                reply,
            ))
            .map_err(|_| TrainingBattleError::Unavailable)?;
        response
            .await
            .map_err(|_| TrainingBattleError::Unavailable)?
    }
}
