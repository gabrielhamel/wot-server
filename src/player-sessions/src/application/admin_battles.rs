use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use wot_accounts::contract::DummyLoadout;

#[derive(Clone)]
pub struct AdminBattles(mpsc::Sender<BattleRequest>);

pub(crate) enum BattleRequest {
    Options(tracing::Span, oneshot::Sender<BattleOptions>),
    Launch(
        tracing::Span,
        AdminBattleSetup,
        oneshot::Sender<Result<BattleLaunchReceipt, AdminBattleError>>,
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
    pub vehicles: Vec<BattleChoice<i64>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminBattleSetup {
    pub arena_type: i32,
    pub duration_seconds: i32,
    pub players: Vec<AdminBattlePlayer>,
    pub dummies: Vec<AdminBattleDummy>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminBattlePlayer {
    pub account_id: i64,
    pub team: u8,
    pub inventory_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminBattleDummy {
    pub team: u8,
    pub loadout: DummyLoadout,
}

#[derive(Clone, Copy, Debug, thiserror::Error)]
pub enum AdminBattleError {
    #[error(
        "Select a map, a duration from 5 to 30 minutes, and vehicles on both teams. Include at least one connected player and no more than 30 entrants."
    )]
    InvalidSetup,
    #[error(
        "A selected player or vehicle is no longer available. Refresh the player list and try again."
    )]
    PlayerUnavailable,
    #[error("A dummy has an unavailable vehicle or incompatible loadout.")]
    InvalidDummy,
    #[error("The battle server is busy or unavailable. Try again shortly.")]
    Unavailable,
    #[error("The arena could not be prepared.")]
    PreparationFailed,
}

impl AdminBattleError {
    pub fn outcome(self) -> &'static str {
        match self {
            Self::InvalidSetup => "invalid_setup",
            Self::PlayerUnavailable => "player_unavailable",
            Self::InvalidDummy => "invalid_dummy",
            Self::Unavailable => "unavailable",
            Self::PreparationFailed => "preparation_failed",
        }
    }
}

impl AdminBattles {
    pub(crate) fn channel() -> (Self, mpsc::Receiver<BattleRequest>) {
        let (sender, receiver) = mpsc::channel(4);
        (Self(sender), receiver)
    }

    pub async fn options(&self) -> Result<BattleOptions, AdminBattleError> {
        let (reply, response) = oneshot::channel();
        self.0
            .try_send(BattleRequest::Options(tracing::Span::current(), reply))
            .map_err(|_| AdminBattleError::Unavailable)?;
        response.await.map_err(|_| AdminBattleError::Unavailable)
    }

    pub async fn start(
        &self,
        setup: AdminBattleSetup,
    ) -> Result<BattleLaunchReceipt, AdminBattleError> {
        let (reply, response) = oneshot::channel();
        self.0
            .try_send(BattleRequest::Launch(
                tracing::Span::current(),
                setup,
                reply,
            ))
            .map_err(|_| AdminBattleError::Unavailable)?;
        response.await.map_err(|_| AdminBattleError::Unavailable)?
    }
}
