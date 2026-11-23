#![forbid(unsafe_code)]

//! Player admission, garage sessions and delivery of player updates.
//!
//! ```compile_fail,E0603
//! use wot_player_sessions::infrastructure::gateway;
//! ```

mod application;
mod domain;
mod infrastructure;
use application::{GrantDelivery, GrantRequestError, PlayerGrants};
pub use infrastructure::{BindBattles, Gateway, GatewayAccounts, GatewayConfig};

#[cfg(feature = "integration-test-support")]
#[doc(hidden)]
pub mod test_support {
    pub use crate::application::grants::GrantRequest;

    pub fn grant_channel() -> (
        super::PlayerGrants,
        tokio::sync::mpsc::Receiver<GrantRequest>,
    ) {
        super::PlayerGrants::channel()
    }
}

mod presentation;

/// HTTP adapter for the garage Store.
pub use presentation::store::router as store_router;

pub mod contract;
