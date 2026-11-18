use crate::accounts::{StoreOffer, StoreOrder, StoreOrderError};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rsa::rand_core::{OsRng, RngCore};
use subtle::ConstantTimeEq;
use tokio::sync::{mpsc, oneshot};

pub(crate) struct StoreToken([u8; 32]);

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

#[derive(serde::Serialize)]
pub(crate) struct StoreCatalog {
    pub revision: String,
    pub credits: i64,
    pub offers: Vec<StoreListing>,
}

#[derive(serde::Serialize)]
pub(crate) struct StoreListing {
    #[serde(flatten)]
    pub offer: StoreOffer,
    pub in_depot: u32,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum StoreError {
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

pub(crate) type StoreReply = oneshot::Sender<Result<StoreCatalog, StoreError>>;

pub(crate) struct StoreRequest {
    pub parent: tracing::Span,
    pub token: StoreToken,
    pub order: Option<StoreOrder>,
    pub reply: StoreReply,
}

#[derive(Clone)]
pub(crate) struct StoreRequests(mpsc::Sender<StoreRequest>);

impl StoreRequests {
    pub fn channel() -> (Self, mpsc::Receiver<StoreRequest>) {
        let (sender, receiver) = mpsc::channel(16);
        (Self(sender), receiver)
    }

    pub async fn request(
        &self,
        token: StoreToken,
        order: Option<StoreOrder>,
    ) -> Result<StoreCatalog, StoreError> {
        let (reply, result) = oneshot::channel();
        self.0
            .try_send(StoreRequest {
                parent: tracing::Span::current(),
                token,
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
