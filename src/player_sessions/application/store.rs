use crate::accounts::contract::{StoreCatalog, StoreOrder, StoreOrderError};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rsa::rand_core::{OsRng, RngCore};
use subtle::ConstantTimeEq;
use tokio::sync::{mpsc, oneshot};

pub(in crate::player_sessions) struct StoreToken([u8; 32]);

impl StoreToken {
    pub fn generate() -> anyhow::Result<Self> {
        let mut bytes = [0; 32];
        OsRng
            .try_fill_bytes(&mut bytes)
            .map_err(|_| anyhow::anyhow!("Generate Store session token"))?;
        Ok(Self(bytes))
    }

    pub fn encode(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.0)
    }

    pub fn parse(value: &str) -> Option<Self> {
        let mut bytes = [0; 32];
        if value.len() != 43 || URL_SAFE_NO_PAD.decode_slice(value, &mut bytes).ok()? != bytes.len()
        {
            return None;
        }
        Some(Self(bytes))
    }

    pub fn matches(&self, other: &Self) -> bool {
        bool::from(self.0.ct_eq(&other.0))
    }
}

#[derive(Debug, thiserror::Error)]
pub(in crate::player_sessions) enum StoreError {
    #[error("Open the Store from a connected garage.")]
    Unauthorized,
    #[error("The account is busy. Try again shortly.")]
    Busy,
    #[error("The Store is temporarily unavailable.")]
    Unavailable,
    #[error(transparent)]
    Rejected(#[from] StoreOrderError),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub(in crate::player_sessions) type StoreReply = oneshot::Sender<Result<StoreCatalog, StoreError>>;

pub(in crate::player_sessions) enum StoreCustomer {
    Garage(StoreToken),
    Dashboard(i64),
}

pub(in crate::player_sessions) struct StoreRequest {
    pub parent: tracing::Span,
    pub customer: StoreCustomer,
    pub order: Option<StoreOrder>,
    pub reply: StoreReply,
}

#[derive(Clone)]
pub(crate) struct StoreRequests(mpsc::Sender<StoreRequest>);

impl StoreRequests {
    pub(in crate::player_sessions) fn channel() -> (Self, mpsc::Receiver<StoreRequest>) {
        let (sender, receiver) = mpsc::channel(16);
        (Self(sender), receiver)
    }

    pub(in crate::player_sessions) async fn request(
        &self,
        customer: StoreCustomer,
        order: Option<StoreOrder>,
    ) -> Result<StoreCatalog, StoreError> {
        let (reply, result) = oneshot::channel();
        self.0
            .try_send(StoreRequest {
                parent: tracing::Span::current(),
                customer,
                order,
                reply,
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => StoreError::Busy,
                mpsc::error::TrySendError::Closed(_) => StoreError::Unavailable,
            })?;
        result.await.map_err(|_| StoreError::Unavailable)?
    }
}
