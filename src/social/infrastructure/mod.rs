mod conversations;
mod friendships;
use super::{
    Contact, ContactChange, Friendship, FriendshipAction, Player, PrivateConversation,
    SocialRepository,
};
use sqlx::PgPool;

#[derive(Clone)]
pub struct PostgresSocial {
    pool: PgPool,
}

impl PostgresSocial {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl SocialRepository for PostgresSocial {
    async fn search(
        &self,
        prefix: &str,
        online_ids: Option<&[i64]>,
    ) -> anyhow::Result<Vec<Player>> {
        let pattern = format!(
            "{}%",
            prefix
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let rows: Vec<(i64, String)> = sqlx::query_as("SELECT id, display_name FROM account_directory WHERE lower(display_name) LIKE lower($1) AND ($2::bigint[] IS NULL OR id=ANY($2)) ORDER BY lower(display_name), id LIMIT 50")
            .bind(pattern).bind(online_ids).fetch_all(&self.pool).await?;
        Ok(rows
            .into_iter()
            .map(|(id, name)| Player { id, name })
            .collect())
    }

    async fn roster(&self, owner: i64) -> anyhow::Result<Vec<Contact>> {
        let rows: Vec<(i64, String, bool, i16)> = sqlx::query_as("SELECT a.id, a.display_name, c.ignored, c.friendship FROM social_contacts c JOIN account_directory a ON a.id=c.peer_id WHERE c.owner_id=$1 ORDER BY a.id LIMIT 300")
            .bind(owner).fetch_all(&self.pool).await?;
        Ok(rows
            .into_iter()
            .map(|(id, name, ignored, friendship)| Contact {
                player: Player { id, name },
                ignored,
                friendship: match friendship {
                    1 => Friendship::Pending,
                    2 => Friendship::Accepted,
                    _ => Friendship::Saved,
                },
            })
            .collect())
    }

    async fn change(&self, owner: i64, peer: i64, change: ContactChange) -> anyhow::Result<()> {
        let mut tx = self.pool.begin().await?;
        if matches!(change, ContactChange::Ignore) {
            for id in [owner.min(peer), owner.max(peer)] {
                sqlx::query("SELECT pg_advisory_xact_lock($1)")
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
            }
            sqlx::query("UPDATE social_contacts SET friendship=0 WHERE owner_id=$1 AND peer_id=$2 AND NOT ignored")
                .bind(peer).bind(owner).execute(&mut *tx).await?;
        } else {
            sqlx::query("SELECT pg_advisory_xact_lock($1)")
                .bind(owner)
                .execute(&mut *tx)
                .await?;
        }
        if matches!(change, ContactChange::Friend | ContactChange::Ignore) {
            let ignored = matches!(change, ContactChange::Ignore);
            let count: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM social_contacts WHERE owner_id=$1 AND peer_id<>$2",
            )
            .bind(owner)
            .bind(peer)
            .fetch_one(&mut *tx)
            .await?;
            anyhow::ensure!(count < 300, "Contact limit reached");
            sqlx::query("INSERT INTO social_contacts(owner_id,peer_id,ignored) VALUES($1,$2,$3) ON CONFLICT(owner_id,peer_id) DO UPDATE SET ignored=EXCLUDED.ignored, friendship=EXCLUDED.friendship")
                .bind(owner).bind(peer).bind(ignored).execute(&mut *tx).await?;
        } else {
            sqlx::query(
                "DELETE FROM social_contacts WHERE owner_id=$1 AND peer_id=$2 AND ignored=$3",
            )
            .bind(owner)
            .bind(peer)
            .bind(matches!(change, ContactChange::RemoveIgnored))
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }
    async fn friendship(
        &self,
        owner: i64,
        peer: i64,
        action: FriendshipAction,
    ) -> anyhow::Result<bool> {
        self.change_friendship(owner, peer, action).await
    }

    async fn inbound_requests(&self, owner: i64) -> anyhow::Result<Vec<Player>> {
        self.pending_inbound(owner).await
    }

    async fn observers(&self, player: i64, online: &[i64]) -> anyhow::Result<Vec<i64>> {
        Ok(sqlx::query_scalar("SELECT owner_id FROM social_contacts WHERE peer_id=$1 AND NOT ignored AND friendship=2 AND owner_id=ANY($2) LIMIT 256").bind(player).bind(online).fetch_all(&self.pool).await?)
    }
    async fn player(&self, id: i64) -> anyhow::Result<Option<Player>> {
        let row: Option<(i64, String)> =
            sqlx::query_as("SELECT id,display_name FROM account_directory WHERE id=$1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(id, name)| Player { id, name }))
    }
    async fn open(&self, owner: i64, peer: i64) -> anyhow::Result<PrivateConversation> {
        self.open_conversation(owner, peer).await
    }
    async fn send(
        &self,
        owner: i64,
        conversation: u32,
        text: &str,
    ) -> anyhow::Result<PrivateConversation> {
        self.send_message(owner, conversation, text).await
    }
    async fn close(&self, owner: i64, conversation: u32) -> anyhow::Result<()> {
        self.close_conversation(owner, conversation).await
    }
    async fn restore(&self, owner: i64) -> anyhow::Result<Vec<PrivateConversation>> {
        self.restore_conversations(owner).await
    }
}
