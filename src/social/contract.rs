//! Social operations and value types used by session adapters.

pub use super::application::SocialService;
pub use super::domain::{
    ChatMessage, Contact, ContactChange, Conversation, Conversations, Friendship, FriendshipAction,
    Player, PrivateConversation, PrivateConversationBlocked, RequestBudget,
};
