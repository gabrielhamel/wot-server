#![forbid(unsafe_code)]

//! Contacts and durable player conversations.
//!
//! ```compile_fail,E0603
//! use wot_social::application::SocialRepository;
//! ```

mod application;
pub mod contract;
mod domain;
mod infrastructure;

pub mod postgres {
    use std::sync::Arc;
    use wot_accounts::contract::PlayerDirectory;

    pub fn service(
        pool: sqlx::PgPool,
        directory: Arc<dyn PlayerDirectory>,
    ) -> super::contract::SocialService {
        super::contract::SocialService::new(super::infrastructure::PostgresSocial::new(
            pool, directory,
        ))
    }

    pub fn migrations() -> &'static sqlx::migrate::Migrator {
        &sqlx::migrate!("./src/infrastructure/migrations")
    }
}
