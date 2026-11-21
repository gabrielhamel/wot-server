mod application;
mod domain;
mod infrastructure;
use application::{GrantDelivery, GrantRequestError, PlayerGrants};
pub use infrastructure::{Gateway, GatewayAccounts, GatewayConfig};

#[cfg(feature = "integration-test-support")]
#[doc(hidden)]
pub mod test_support {
    pub use crate::platform::mercury;
}

#[cfg(test)]
pub(crate) fn test_grant_channel() -> (
    PlayerGrants,
    tokio::sync::mpsc::Receiver<application::grants::GrantRequest>,
) {
    PlayerGrants::channel()
}

mod presentation;

use application::store::StoreRequests;

pub(crate) fn store_router(
    _composition: &crate::server::Composition,
    requests: StoreRequests,
) -> axum::Router {
    presentation::store::router(requests)
}

/// Operations published to Player Administration.
pub mod contract {
    pub(crate) use super::application::store::StoreRequests;
    pub use super::application::{GrantDelivery, GrantRequestError, PlayerGrants};
    pub(crate) use super::presentation::store::{
        DashboardStorePlayer, dashboard_router as dashboard_store_router,
    };
}
