#![forbid(unsafe_code)]

mod application;
mod domain;
mod infrastructure;

pub mod contract;

/// In-process adapter for sessions connected through Mercury.
pub mod mercury {
    pub use super::infrastructure::mercury::{MercuryArena, bind};
}
