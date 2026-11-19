mod application;
mod domain;
mod infrastructure;
pub use application::{Social, SocialRepository};
pub use domain::{
    ChatMessage, Contact, ContactChange, Conversation, Conversations, Friendship, FriendshipAction,
    Player, PrivateConversation, PrivateConversationBlocked, RequestBudget,
};
pub type SocialService = Social<infrastructure::PostgresSocial>;

pub mod postgres {
    pub fn service(
        pool: sqlx::PgPool,
        directory: std::sync::Arc<dyn crate::accounts::contract::PlayerDirectory>,
    ) -> super::SocialService {
        super::Social::new(super::infrastructure::PostgresSocial::new(pool, directory))
    }

    pub const MIGRATIONS: &sqlx::migrate::Migrator =
        &sqlx::migrate!("./src/social/infrastructure/migrations");
}

/// Social operations and value types used by session adapters.
pub mod contract {
    pub use super::{
        ChatMessage, Contact, ContactChange, Conversation, Conversations, Friendship,
        FriendshipAction, Player, PrivateConversation, PrivateConversationBlocked, RequestBudget,
        SocialService,
    };
}
