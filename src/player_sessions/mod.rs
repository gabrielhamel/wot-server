mod application;
mod domain;
mod infrastructure;
pub use application::{GrantDelivery, GrantRequestError, PlayerGrants};
pub use infrastructure::{Gateway, GatewayConfig};

#[doc(hidden)]
pub use infrastructure::mercury;

mod presentation;

pub(crate) use application::store::StoreRequests;
pub(crate) use presentation::store::{
    DashboardStorePlayer, dashboard_router as dashboard_store_router, router as store_router,
};

/// Operations published to Player Administration.
pub(crate) mod contract {
    pub(crate) use super::{
        DashboardStorePlayer, GrantDelivery, GrantRequestError, PlayerGrants, StoreRequests,
        dashboard_store_router,
    };
}
