#![forbid(unsafe_code)]

mod application;
mod domain;
mod presentation;

pub mod contract {
    pub use crate::application::Store;
    pub use crate::presentation::store::{DashboardStorePlayer, dashboard_router, router};
}
