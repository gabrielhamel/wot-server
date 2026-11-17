use crate::accounts::{AccountSnapshot, GrantError, PlayerGrant};
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantDelivery {
    QueuedForGarage,
    OnNextSync,
}

#[derive(Debug, thiserror::Error)]
pub enum GrantRequestError {
    #[error("Player not found.")]
    NotFound,
    #[error("The account is busy. Try again shortly.")]
    Busy,
    #[error("The game gateway is unavailable.")]
    Unavailable,
    #[error(transparent)]
    Rejected(#[from] GrantError),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub(crate) type GrantResult = Result<(AccountSnapshot, GrantDelivery), GrantRequestError>;

pub(crate) struct GrantRequest {
    pub parent: tracing::Span,
    pub account_id: i64,
    pub grant: PlayerGrant,
    pub reply: oneshot::Sender<GrantResult>,
}

#[derive(Clone)]
pub struct PlayerGrants {
    sender: mpsc::Sender<GrantRequest>,
}

impl PlayerGrants {
    pub(crate) fn channel() -> (Self, mpsc::Receiver<GrantRequest>) {
        let (sender, receiver) = mpsc::channel(16);
        (Self { sender }, receiver)
    }

    #[tracing::instrument(name = "access.grants.grant", skip_all, fields(ddd.bounded_context.name = "access", input.account_id = account_id, input.grant = ?grant), err)]
    pub async fn grant(&self, account_id: i64, grant: PlayerGrant) -> GrantResult {
        let (reply, result) = oneshot::channel();
        self.sender
            .try_send(GrantRequest {
                parent: tracing::Span::current(),
                account_id,
                grant,
                reply,
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => GrantRequestError::Busy,
                mpsc::error::TrySendError::Closed(_) => GrantRequestError::Unavailable,
            })?;
        result.await.map_err(|_| GrantRequestError::Unavailable)?
    }
}
