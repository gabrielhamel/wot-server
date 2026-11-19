//! UDP wire protocol for client login and garage sessions.

#![cfg_attr(
    not(feature = "integration-test-support"),
    allow(
        dead_code,
        unused_imports,
        clippy::wrong_self_convention,
        reason = "Protocol fixtures use the client and codec API behind the integration-test feature"
    )
)]

pub mod app;
pub mod net;
pub mod script;
