mod conversations;
mod friendships;
use super::application::SocialRepository;
use super::domain::{
    Contact, ContactChange, Friendship, FriendshipAction, Player, PrivateConversation,
};
use crate::accounts::contract::PlayerDirectory;
use sqlx::PgPool;
use std::collections::BTreeMap;

pub(super) struct PostgresSocial {
    pool: PgPool,
    directory: std::sync::Arc<dyn PlayerDirectory>,
}

impl PostgresSocial {
    pub(super) fn new(pool: PgPool, directory: std::sync::Arc<dyn PlayerDirectory>) -> Self {
        Self { pool, directory }
    }

    async fn player_names(&self, ids: &[i64]) -> anyhow::Result<BTreeMap<i64, String>> {
        Ok(self
            .directory
            .find(ids)
            .await?
            .into_iter()
            .map(|player| (player.id, player.display_name))
            .collect())
    }

    async fn require_players(
        &self,
        owner: i64,
        peer: i64,
    ) -> anyhow::Result<BTreeMap<i64, String>> {
        let names = self.player_names(&[owner, peer]).await?;
        anyhow::ensure!(names.len() == 2, "Unknown player");
        Ok(names)
    }
}

#[async_trait::async_trait]
impl SocialRepository for PostgresSocial {
    #[tracing::instrument(name = "social.search", skip_all, fields(ddd.bounded_context.name = "social", input.prefix.bytes = prefix.len()), err)]
    async fn search(
        &self,
        prefix: &str,
        online_ids: Option<&[i64]>,
    ) -> anyhow::Result<Vec<Player>> {
        Ok(self
            .directory
            .search(prefix, online_ids)
            .await?
            .into_iter()
            .map(|player| Player {
                id: player.id,
                name: player.display_name,
            })
            .collect())
    }

    #[tracing::instrument(name = "social.roster", skip_all, fields(ddd.bounded_context.name = "social", input.owner = owner), err)]
    async fn roster(&self, owner: i64) -> anyhow::Result<Vec<Contact>> {
        let rows: Vec<(i64, bool, i16)> = {
            let statement = "SELECT peer_id, ignored, friendship FROM social.social_contacts WHERE owner_id=$1 ORDER BY peer_id LIMIT 300";
            crate::platform::observability::database(
                "social",
                statement,
                sqlx::query_as(statement).bind(owner).fetch_all(&self.pool),
            )
            .await
        }?;
        let ids: Vec<_> = rows.iter().map(|row| row.0).collect();
        let mut names = self.player_names(&ids).await?;
        Ok(rows
            .into_iter()
            .filter_map(|(id, ignored, friendship)| {
                Some(Contact {
                    player: Player {
                        id,
                        name: names.remove(&id)?,
                    },
                    ignored,
                    friendship: match friendship {
                        1 => Friendship::Pending,
                        2 => Friendship::Accepted,
                        _ => Friendship::Saved,
                    },
                })
            })
            .collect())
    }

    #[tracing::instrument(name = "social.change", skip_all, fields(ddd.bounded_context.name = "social", input.owner = owner, input.peer = peer, input.change = ?change), err)]
    async fn change(&self, owner: i64, peer: i64, change: ContactChange) -> anyhow::Result<()> {
        if matches!(change, ContactChange::Friend | ContactChange::Ignore) {
            self.require_players(owner, peer).await?;
        }
        let mut tx =
            crate::platform::observability::database("social", "BEGIN", self.pool.begin()).await?;
        if matches!(change, ContactChange::Ignore) {
            for id in [owner.min(peer), owner.max(peer)] {
                {
                    let statement = "SELECT pg_advisory_xact_lock($1)";
                    crate::platform::observability::database(
                        "social",
                        statement,
                        sqlx::query(statement).bind(id).execute(&mut *tx),
                    )
                    .await
                }?;
            }
            {
                let statement = "UPDATE social.social_contacts SET friendship=0 WHERE owner_id=$1 AND peer_id=$2 AND NOT ignored";
                crate::platform::observability::database(
                    "social",
                    statement,
                    sqlx::query(statement)
                        .bind(peer)
                        .bind(owner)
                        .execute(&mut *tx),
                )
                .await
            }?;
        } else {
            {
                let statement = "SELECT pg_advisory_xact_lock($1)";
                crate::platform::observability::database(
                    "social",
                    statement,
                    sqlx::query(statement).bind(owner).execute(&mut *tx),
                )
                .await
            }?;
        }
        if matches!(change, ContactChange::Friend | ContactChange::Ignore) {
            let ignored = matches!(change, ContactChange::Ignore);
            let count: i64 = {
                let statement =
                    "SELECT count(*) FROM social.social_contacts WHERE owner_id=$1 AND peer_id<>$2";
                crate::platform::observability::database(
                    "social",
                    statement,
                    sqlx::query_scalar(statement)
                        .bind(owner)
                        .bind(peer)
                        .fetch_one(&mut *tx),
                )
                .await
            }?;
            anyhow::ensure!(count < 300, "Contact limit reached");
            {
                let statement = "INSERT INTO social.social_contacts(owner_id,peer_id,ignored) VALUES($1,$2,$3) ON CONFLICT(owner_id,peer_id) DO UPDATE SET ignored=EXCLUDED.ignored, friendship=EXCLUDED.friendship";
                crate::platform::observability::database(
                    "social",
                    statement,
                    sqlx::query(statement)
                        .bind(owner)
                        .bind(peer)
                        .bind(ignored)
                        .execute(&mut *tx),
                )
                .await
            }?;
        } else {
            {
                let statement = "DELETE FROM social.social_contacts WHERE owner_id=$1 AND peer_id=$2 AND ignored=$3";
                crate::platform::observability::database(
                    "social",
                    statement,
                    sqlx::query(statement)
                        .bind(owner)
                        .bind(peer)
                        .bind(matches!(change, ContactChange::RemoveIgnored))
                        .execute(&mut *tx),
                )
                .await
            }?;
        }
        crate::platform::observability::database("social", "COMMIT", tx.commit()).await?;
        Ok(())
    }
    #[tracing::instrument(name = "social.friendship", skip_all, fields(ddd.bounded_context.name = "social", input.owner = owner, input.peer = peer, input.action = ?action), err)]
    async fn friendship(
        &self,
        owner: i64,
        peer: i64,
        action: FriendshipAction,
    ) -> anyhow::Result<bool> {
        self.change_friendship(owner, peer, action).await
    }

    #[tracing::instrument(name = "social.inbound_requests", skip_all, fields(ddd.bounded_context.name = "social", input.owner = owner), err)]
    async fn inbound_requests(&self, owner: i64) -> anyhow::Result<Vec<Player>> {
        self.pending_inbound(owner).await
    }

    #[tracing::instrument(name = "social.observers", skip_all, fields(ddd.bounded_context.name = "social", input.player = player), err)]
    async fn observers(&self, player: i64, online: &[i64]) -> anyhow::Result<Vec<i64>> {
        Ok({
            let statement = "SELECT owner_id FROM social.social_contacts WHERE peer_id=$1 AND NOT ignored AND friendship=2 AND owner_id=ANY($2) LIMIT 256";
            crate::platform::observability::database(
                "social",
                statement,
                sqlx::query_scalar(statement)
                    .bind(player)
                    .bind(online)
                    .fetch_all(&self.pool),
            )
            .await
        }?)
    }
    #[tracing::instrument(name = "social.player", skip_all, fields(ddd.bounded_context.name = "social", input.id = id), err)]
    async fn player(&self, id: i64) -> anyhow::Result<Option<Player>> {
        Ok(self
            .directory
            .find(&[id])
            .await?
            .into_iter()
            .next()
            .map(|player| Player {
                id: player.id,
                name: player.display_name,
            }))
    }
    #[tracing::instrument(name = "social.open", skip_all, fields(ddd.bounded_context.name = "social", input.owner = owner, input.peer = peer), err)]
    async fn open(&self, owner: i64, peer: i64) -> anyhow::Result<PrivateConversation> {
        self.open_conversation(owner, peer).await
    }
    #[tracing::instrument(name = "social.send", skip_all, fields(ddd.bounded_context.name = "social", input.owner = owner, input.conversation = conversation, input.text.bytes = text.len()), err)]
    async fn send(
        &self,
        owner: i64,
        conversation: u32,
        text: &str,
    ) -> anyhow::Result<PrivateConversation> {
        self.send_message(owner, conversation, text).await
    }
    #[tracing::instrument(name = "social.close", skip_all, fields(ddd.bounded_context.name = "social", input.owner = owner, input.conversation = conversation), err)]
    async fn close(&self, owner: i64, conversation: u32) -> anyhow::Result<()> {
        self.close_conversation(owner, conversation).await
    }
    #[tracing::instrument(name = "social.restore", skip_all, fields(ddd.bounded_context.name = "social", input.owner = owner), err)]
    async fn restore(&self, owner: i64) -> anyhow::Result<Vec<PrivateConversation>> {
        self.restore_conversations(owner).await
    }
}
