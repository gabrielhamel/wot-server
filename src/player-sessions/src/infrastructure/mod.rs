mod bootstrap;
mod gateway;
mod schema;
pub use gateway::{BindBattles, Gateway, GatewayAccounts, GatewayConfig};
mod credentials;
mod endpoint;

pub mod xmpp;
