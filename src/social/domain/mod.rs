use crate::shared_kernel::AccountId;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn distinct_accounts(first: i64, second: i64) -> bool {
    matches!(
        (AccountId::new(first), AccountId::new(second)),
        (Some(first), Some(second)) if first != second
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Player {
    pub id: i64,
    pub name: String,
}

#[derive(Clone, Copy, Debug)]
pub enum ContactChange {
    Friend,
    Ignore,
    RemoveFriend,
    RemoveIgnored,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Friendship {
    Saved,
    Pending,
    Accepted,
}

#[derive(Clone, Copy, Debug)]
pub enum FriendshipAction {
    Save,
    Request,
    Accept,
    Decline,
    Cancel,
    Remove,
}

#[derive(Clone, Debug)]
pub struct Contact {
    pub player: Player,
    pub ignored: bool,
    pub friendship: Friendship,
}

#[derive(Clone, Debug)]
pub struct Conversation {
    pub id: u32,
    pub participants: [i64; 2],
    subscribers: BTreeSet<i64>,
}

impl Conversation {
    pub fn is_subscribed(&self, player: i64) -> bool {
        self.subscribers.contains(&player)
    }
}
#[derive(Default)]
pub struct Conversations {
    next_id: u32,
    rooms: BTreeMap<u32, Conversation>,
}

impl Conversations {
    pub fn between(&self, first: i64, second: i64) -> Option<&Conversation> {
        self.rooms
            .values()
            .find(|room| room.participants.contains(&first) && room.participants.contains(&second))
    }

    pub fn open(&mut self, owner: i64, peer: i64) -> Option<Conversation> {
        if !distinct_accounts(owner, peer) {
            return None;
        }
        if let Some(id) = self.between(owner, peer).map(|room| room.id) {
            return self.subscribe(id, owner);
        }
        if [owner, peer].iter().any(|id| {
            self.rooms
                .values()
                .filter(|r| r.participants.contains(id))
                .count()
                >= 16
        }) {
            return None;
        }
        let id = self.next_id.checked_add(1)?;
        self.next_id = id;
        let room = Conversation {
            id,
            participants: [owner, peer],
            subscribers: BTreeSet::from([owner]),
        };
        self.rooms.insert(id, room.clone());
        Some(room)
    }

    pub fn joined(&self, id: u32, player: i64) -> Option<&Conversation> {
        self.rooms.get(&id).filter(|r| r.is_subscribed(player))
    }
    pub fn subscribe(&mut self, id: u32, player: i64) -> Option<Conversation> {
        let room = self
            .rooms
            .get_mut(&id)
            .filter(|r| r.participants.contains(&player))?;
        room.subscribers.insert(player);
        Some(room.clone())
    }
    pub fn close(&mut self, id: u32, player: i64) -> Option<Conversation> {
        let room = self
            .rooms
            .get_mut(&id)
            .filter(|r| r.is_subscribed(player))?;
        room.subscribers.remove(&player);
        let remaining = room.clone();
        if room.subscribers.is_empty() {
            self.rooms.remove(&id);
        }
        Some(remaining)
    }
    pub fn disconnect(&mut self, player: i64) -> Vec<Conversation> {
        let ids: Vec<_> = self
            .rooms
            .values()
            .filter(|r| r.participants.contains(&player))
            .map(|r| r.id)
            .collect();
        ids.into_iter()
            .filter_map(|id| self.rooms.remove(&id))
            .collect()
    }
}

pub struct RequestBudget {
    since: std::time::Instant,
    remaining: u8,
}
impl Default for RequestBudget {
    fn default() -> Self {
        Self {
            since: std::time::Instant::now(),
            remaining: 32,
        }
    }
}
impl RequestBudget {
    pub fn admit(&mut self, now: std::time::Instant) -> bool {
        if now.saturating_duration_since(self.since) >= std::time::Duration::from_secs(1) {
            self.since = now;
            self.remaining = 32;
        }
        if self.remaining == 0 {
            return false;
        }
        self.remaining -= 1;
        true
    }
}

#[derive(Debug)]
pub struct PrivateConversationBlocked;

impl std::fmt::Display for PrivateConversationBlocked {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Private conversation blocked")
    }
}

impl std::error::Error for PrivateConversationBlocked {}

#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub id: i64,
    pub sender: Player,
    pub sent_at: f64,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct PrivateConversation {
    pub id: u32,
    pub players: [Player; 2],
    pub history: Vec<ChatMessage>,
}

impl PrivateConversation {
    pub fn peer(&self, player: i64) -> Option<&Player> {
        if self.players[0].id == player {
            Some(&self.players[1])
        } else if self.players[1].id == player {
            Some(&self.players[0])
        } else {
            None
        }
    }
}
