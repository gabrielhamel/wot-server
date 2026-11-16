use super::domain::{Contact, ContactChange, FriendshipAction, Player, PrivateConversation};
use std::future::Future;

pub trait SocialRepository: Clone + Send + Sync + 'static {
    fn search(
        &self,
        prefix: &str,
        online_ids: Option<&[i64]>,
    ) -> impl Future<Output = anyhow::Result<Vec<Player>>> + Send;
    fn roster(&self, owner: i64) -> impl Future<Output = anyhow::Result<Vec<Contact>>> + Send;
    fn change(
        &self,
        owner: i64,
        peer: i64,
        change: ContactChange,
    ) -> impl Future<Output = anyhow::Result<()>> + Send;
    fn friendship(
        &self,
        owner: i64,
        peer: i64,
        action: FriendshipAction,
    ) -> impl Future<Output = anyhow::Result<bool>> + Send;
    fn inbound_requests(
        &self,
        owner: i64,
    ) -> impl Future<Output = anyhow::Result<Vec<Player>>> + Send;
    fn observers(
        &self,
        player: i64,
        online: &[i64],
    ) -> impl Future<Output = anyhow::Result<Vec<i64>>> + Send;
    fn player(&self, id: i64) -> impl Future<Output = anyhow::Result<Option<Player>>> + Send;
    fn open(
        &self,
        owner: i64,
        peer: i64,
    ) -> impl Future<Output = anyhow::Result<PrivateConversation>> + Send;
    fn send(
        &self,
        owner: i64,
        conversation: u32,
        text: &str,
    ) -> impl Future<Output = anyhow::Result<PrivateConversation>> + Send;
    fn close(
        &self,
        owner: i64,
        conversation: u32,
    ) -> impl Future<Output = anyhow::Result<()>> + Send;
    fn restore(
        &self,
        owner: i64,
    ) -> impl Future<Output = anyhow::Result<Vec<PrivateConversation>>> + Send;
}

#[derive(Clone)]
pub struct Social<R> {
    repository: R,
}

impl<R: SocialRepository> Social<R> {
    pub fn new(repository: R) -> Self {
        Self { repository }
    }

    pub async fn search(
        &self,
        prefix: &str,
        online_ids: Option<&[i64]>,
    ) -> anyhow::Result<Vec<Player>> {
        anyhow::ensure!(
            !prefix.trim().is_empty()
                && prefix.len() <= 128
                && !prefix.chars().any(char::is_control),
            "Invalid player search"
        );
        self.repository.search(prefix, online_ids).await
    }

    pub async fn roster(&self, owner: i64) -> anyhow::Result<Vec<Contact>> {
        self.repository.roster(owner).await
    }

    pub async fn change(
        &self,
        owner: i64,
        peer: i64,
        change: ContactChange,
    ) -> anyhow::Result<Vec<Contact>> {
        anyhow::ensure!(owner > 0 && peer > 0 && owner != peer, "Invalid contact");
        self.repository.change(owner, peer, change).await?;
        self.repository.roster(owner).await
    }
    pub async fn friendship(
        &self,
        owner: i64,
        peer: i64,
        action: FriendshipAction,
    ) -> anyhow::Result<bool> {
        anyhow::ensure!(
            owner > 0 && peer > 0 && owner != peer,
            "Invalid friendship participants"
        );
        self.repository.friendship(owner, peer, action).await
    }

    pub async fn inbound_requests(&self, owner: i64) -> anyhow::Result<Vec<Player>> {
        self.repository.inbound_requests(owner).await
    }

    pub async fn observers(&self, player: i64, online: &[i64]) -> anyhow::Result<Vec<i64>> {
        self.repository.observers(player, online).await
    }
    pub async fn player(&self, id: i64) -> anyhow::Result<Option<Player>> {
        self.repository.player(id).await
    }
    pub async fn open(&self, owner: i64, peer: i64) -> anyhow::Result<PrivateConversation> {
        anyhow::ensure!(
            owner > 0 && peer > 0 && owner != peer,
            "Invalid private recipient"
        );
        self.repository.open(owner, peer).await
    }
    pub async fn send(
        &self,
        owner: i64,
        conversation: u32,
        text: &str,
    ) -> anyhow::Result<PrivateConversation> {
        anyhow::ensure!(
            !text.trim().is_empty()
                && text.len() <= 2048
                && !text.chars().any(|c| c.is_control() && c != '\n'),
            "Invalid message"
        );
        self.repository.send(owner, conversation, text).await
    }
    pub async fn close(&self, owner: i64, conversation: u32) -> anyhow::Result<()> {
        self.repository.close(owner, conversation).await
    }
    pub async fn restore(&self, owner: i64) -> anyhow::Result<Vec<PrivateConversation>> {
        self.repository.restore(owner).await
    }
}
