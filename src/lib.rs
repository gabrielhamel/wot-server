#![forbid(unsafe_code)]

pub mod accounts;
pub(crate) mod player_administration;
pub mod player_sessions;
pub mod server;
pub(crate) mod shared_kernel;
pub mod social;

#[doc(hidden)]
pub mod platform;
