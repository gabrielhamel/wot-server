//! Operations published to Player Administration.
pub use crate::application::store::StoreRequests;
pub use crate::application::{GrantDelivery, GrantRequestError, PlayerGrants};
pub use crate::presentation::store::{
    DashboardStorePlayer, dashboard_router as dashboard_store_router,
};

pub use crate::application::admin_battles::{
    AdminBattleError, AdminBattleSetup, AdminBattles, BattleLaunchReceipt, BattleOptions,
};
