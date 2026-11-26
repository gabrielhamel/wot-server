use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rsa::rand_core::{OsRng, RngCore};
use subtle::ConstantTimeEq;
use tokio::sync::{mpsc, oneshot};
use wot_accounts::contract::{AccountView, PurchasableOffer, StoreOrderError};

pub struct StoreToken([u8; 32]);

impl StoreToken {
    pub(crate) fn generate() -> anyhow::Result<Self> {
        let mut bytes = [0; 32];
        OsRng
            .try_fill_bytes(&mut bytes)
            .map_err(|_| anyhow::anyhow!("Generate Store session token"))?;
        Ok(Self(bytes))
    }

    pub(crate) fn encode(&self) -> String {
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

    pub(crate) fn matches(&self, other: &Self) -> bool {
        bool::from(self.0.ct_eq(&other.0))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
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

pub(crate) type StoreReply = oneshot::Sender<Result<AccountView, StoreError>>;

pub enum StoreCustomer {
    Garage(StoreToken),
    Dashboard(i64),
}

pub(crate) struct StoreRequest {
    pub parent: tracing::Span,
    pub customer: StoreCustomer,
    pub checkout: Option<Result<StoreCheckout, StoreOrderError>>,
    pub reply: StoreReply,
}

pub struct StoreCheckout {
    pub offer: PurchasableOffer,
    pub revision: i64,
}

#[derive(Clone)]
pub struct GarageStoreAccess(mpsc::Sender<StoreRequest>);

impl GarageStoreAccess {
    pub(crate) fn channel() -> (Self, mpsc::Receiver<StoreRequest>) {
        let (sender, receiver) = mpsc::channel(16);
        (Self(sender), receiver)
    }

    pub async fn garage_account(&self, token: StoreToken) -> Result<AccountView, StoreError> {
        self.request(StoreCustomer::Garage(token), None).await
    }

    pub async fn checkout(
        &self,
        customer: StoreCustomer,
        checkout: Result<StoreCheckout, StoreOrderError>,
    ) -> Result<AccountView, StoreError> {
        self.request(customer, Some(checkout)).await
    }

    async fn request(
        &self,
        customer: StoreCustomer,
        checkout: Option<Result<StoreCheckout, StoreOrderError>>,
    ) -> Result<AccountView, StoreError> {
        let (reply, result) = oneshot::channel();
        self.0
            .try_send(StoreRequest {
                parent: tracing::Span::current(),
                customer,
                checkout,
                reply,
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => StoreError::Busy,
                mpsc::error::TrySendError::Closed(_) => StoreError::Unavailable,
            })?;
        result.await.map_err(|_| StoreError::Unavailable)?
    }
}
