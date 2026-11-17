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

    #[tracing::instrument(name = "social.search", skip_all, fields(ddd.bounded_context.name = "social", output.count = tracing::field::Empty, input.prefix.bytes = prefix.len()), err)]
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
        self.repository
            .search(prefix, online_ids)
            .await
            .inspect(|values| {
                tracing::Span::current().record("output.count", values.len());
            })
    }

    #[tracing::instrument(name = "social.roster", skip_all, fields(ddd.bounded_context.name = "social", output.count = tracing::field::Empty, input.owner = owner), err)]
    pub async fn roster(&self, owner: i64) -> anyhow::Result<Vec<Contact>> {
        self.repository.roster(owner).await.inspect(|values| {
            tracing::Span::current().record("output.count", values.len());
        })
    }

    #[tracing::instrument(name = "social.change", skip_all, fields(ddd.bounded_context.name = "social", output.count = tracing::field::Empty, input.owner = owner, input.peer = peer, input.change = ?change), err)]
    pub async fn change(
        &self,
        owner: i64,
        peer: i64,
        change: ContactChange,
    ) -> anyhow::Result<Vec<Contact>> {
        anyhow::ensure!(
            super::domain::distinct_accounts(owner, peer),
            "Invalid contact"
        );
        self.repository.change(owner, peer, change).await?;
        self.repository.roster(owner).await.inspect(|values| {
            tracing::Span::current().record("output.count", values.len());
        })
    }
    #[tracing::instrument(name = "social.friendship", skip_all, fields(ddd.bounded_context.name = "social", input.owner = owner, input.peer = peer, input.action = ?action), err)]
    pub async fn friendship(
        &self,
        owner: i64,
        peer: i64,
        action: FriendshipAction,
    ) -> anyhow::Result<bool> {
        anyhow::ensure!(
            super::domain::distinct_accounts(owner, peer),
            "Invalid friendship participants"
        );
        self.repository.friendship(owner, peer, action).await
    }

    #[tracing::instrument(name = "social.inbound_requests", skip_all, fields(ddd.bounded_context.name = "social", output.count = tracing::field::Empty, input.owner = owner), err)]
    pub async fn inbound_requests(&self, owner: i64) -> anyhow::Result<Vec<Player>> {
        self.repository
            .inbound_requests(owner)
            .await
            .inspect(|values| {
                tracing::Span::current().record("output.count", values.len());
            })
    }

    #[tracing::instrument(name = "social.observers", skip_all, fields(ddd.bounded_context.name = "social", output.count = tracing::field::Empty, input.player = player), err)]
    pub async fn observers(&self, player: i64, online: &[i64]) -> anyhow::Result<Vec<i64>> {
        self.repository
            .observers(player, online)
            .await
            .inspect(|values| {
                tracing::Span::current().record("output.count", values.len());
            })
    }
    #[tracing::instrument(name = "social.player", skip_all, fields(ddd.bounded_context.name = "social", input.id = id), err)]
    pub async fn player(&self, id: i64) -> anyhow::Result<Option<Player>> {
        self.repository.player(id).await
    }
    #[tracing::instrument(name = "social.open", skip_all, fields(ddd.bounded_context.name = "social", input.owner = owner, input.peer = peer), err)]
    pub async fn open(&self, owner: i64, peer: i64) -> anyhow::Result<PrivateConversation> {
        anyhow::ensure!(
            super::domain::distinct_accounts(owner, peer),
            "Invalid private recipient"
        );
        self.repository.open(owner, peer).await
    }
    #[tracing::instrument(name = "social.send", skip_all, fields(ddd.bounded_context.name = "social", input.owner = owner, input.conversation = conversation, input.text.bytes = text.len()), err)]
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
    #[tracing::instrument(name = "social.close", skip_all, fields(ddd.bounded_context.name = "social", input.owner = owner, input.conversation = conversation), err)]
    pub async fn close(&self, owner: i64, conversation: u32) -> anyhow::Result<()> {
        self.repository.close(owner, conversation).await
    }
    #[tracing::instrument(name = "social.restore", skip_all, fields(ddd.bounded_context.name = "social", output.count = tracing::field::Empty, input.owner = owner), err)]
    pub async fn restore(&self, owner: i64) -> anyhow::Result<Vec<PrivateConversation>> {
        self.repository.restore(owner).await.inspect(|values| {
            tracing::Span::current().record("output.count", values.len());
        })
    }
}
