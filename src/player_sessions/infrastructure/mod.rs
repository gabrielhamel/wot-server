mod bootstrap;
mod gateway;
mod schema;
pub use gateway::{Gateway, GatewayAccounts, GatewayConfig};
mod credentials;
mod endpoint;

pub mod xmpp;
