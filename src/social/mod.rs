mod application;
pub mod contract;
mod domain;
mod infrastructure;

pub mod postgres {
    use crate::accounts::contract::PlayerDirectory;
    use crate::server::Composition;
    use std::sync::Arc;

    pub fn service(
        _composition: &Composition,
        pool: sqlx::PgPool,
        directory: Arc<dyn PlayerDirectory>,
    ) -> super::contract::SocialService {
        super::contract::SocialService::new(super::infrastructure::PostgresSocial::new(
            pool, directory,
        ))
    }

    pub fn migrations(_composition: &Composition) -> &'static sqlx::migrate::Migrator {
        &sqlx::migrate!("./src/social/infrastructure/migrations")
    }
}
