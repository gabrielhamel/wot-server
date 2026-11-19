use super::PostgresSocial;
use crate::social::{ChatMessage, Player, PrivateConversation, PrivateConversationBlocked};
use sqlx::{Postgres, Transaction};

type Tx<'a> = Transaction<'a, Postgres>;

#[tracing::instrument(name = "social.conversations.lock_pair", skip_all, fields(ddd.bounded_context.name = "social", input.first = first, input.second = second), err)]
async fn lock_pair(tx: &mut Tx<'_>, first: i64, second: i64) -> anyhow::Result<()> {
    for id in [first, second] {
        {
            let statement = "SELECT pg_advisory_xact_lock($1)";
            crate::platform::observability::database(
                "social",
                statement,
                sqlx::query(statement).bind(id).execute(&mut **tx),
            )
            .await
        }?;
    }
    let ignored: bool = {
        let statement = "SELECT EXISTS(SELECT 1 FROM social.social_contacts WHERE ignored AND ((owner_id=$1 AND peer_id=$2) OR (owner_id=$2 AND peer_id=$1)))";
        crate::platform::observability::database(
            "social",
            statement,
            sqlx::query_scalar(statement)
                .bind(first)
                .bind(second)
                .fetch_one(&mut **tx),
        )
        .await
    }?;
    if ignored {
        return Err(PrivateConversationBlocked.into());
    }
    Ok(())
}

#[tracing::instrument(name = "social.conversations.retain_recent", skip_all, fields(ddd.bounded_context.name = "social", input.owner = owner, input.conversation = conversation), err)]
async fn retain_recent(tx: &mut Tx<'_>, owner: i64, conversation: i32) -> anyhow::Result<()> {
    {
        let statement = "UPDATE social.private_conversations SET first_open=CASE WHEN first_id=$1 THEN FALSE ELSE first_open END, second_open=CASE WHEN second_id=$1 THEN FALSE ELSE second_open END WHERE id IN (SELECT id FROM social.private_conversations WHERE id<>$2 AND ((first_id=$1 AND first_open) OR (second_id=$1 AND second_open)) ORDER BY last_used DESC,id DESC OFFSET 15)";
        crate::platform::observability::database(
            "social",
            statement,
            sqlx::query(statement)
                .bind(owner)
                .bind(conversation)
                .execute(&mut **tx),
        )
        .await
    }?;
    Ok(())
}

struct StoredConversation {
    id: u32,
    participants: [i64; 2],
    messages: Vec<StoredMessage>,
}

type StoredMessage = (i64, i64, f64, String);

impl StoredConversation {
    fn with_names(
        self,
        names: &std::collections::BTreeMap<i64, String>,
    ) -> Option<PrivateConversation> {
        let [first, second] = self.participants;
        Some(PrivateConversation {
            id: self.id,
            players: [
                Player {
                    id: first,
                    name: names.get(&first)?.clone(),
                },
                Player {
                    id: second,
                    name: names.get(&second)?.clone(),
                },
            ],
            history: self
                .messages
                .into_iter()
                .rev()
                .filter_map(|(id, sender, sent_at, text)| {
                    Some(ChatMessage {
                        id,
                        sender: Player {
                            id: sender,
                            name: names.get(&sender)?.clone(),
                        },
                        sent_at,
                        text,
                    })
                })
                .collect(),
        })
    }
}

#[tracing::instrument(name = "social.conversations.load", skip_all, fields(ddd.bounded_context.name = "social", input.id = id), err)]
async fn load(tx: &mut Tx<'_>, id: i32) -> anyhow::Result<StoredConversation> {
    let (first, second): (i64, i64) = {
        let statement = "SELECT first_id, second_id FROM social.private_conversations WHERE id=$1";
        crate::platform::observability::database(
            "social",
            statement,
            sqlx::query_as(statement).bind(id).fetch_one(&mut **tx),
        )
        .await
    }?;
    let messages: Vec<StoredMessage> = {
        let statement = "SELECT id,sender_id,sent_at,body FROM social.private_messages WHERE conversation_id=$1 ORDER BY id DESC LIMIT 100";
        crate::platform::observability::database(
            "social",
            statement,
            sqlx::query_as(statement).bind(id).fetch_all(&mut **tx),
        )
        .await
    }?;
    Ok(StoredConversation {
        id: u32::try_from(id)?,
        participants: [first, second],
        messages,
    })
}

impl PostgresSocial {
    #[tracing::instrument(name = "social.conversations.open_conversation", skip_all, fields(ddd.bounded_context.name = "social", input.owner = owner, input.peer = peer), err)]
    pub(super) async fn open_conversation(
        &self,
        owner: i64,
        peer: i64,
    ) -> anyhow::Result<PrivateConversation> {
        let names = self.require_players(owner, peer).await?;
        let (first, second) = (owner.min(peer), owner.max(peer));
        let mut tx =
            crate::platform::observability::database("social", "BEGIN", self.pool.begin()).await?;
        lock_pair(&mut tx, first, second).await?;
        for player in [owner, peer] {
            let count: i64 = {
                let statement = "SELECT count(*) FROM social.private_conversations WHERE (first_id=$1 OR second_id=$1) AND NOT (first_id=$2 AND second_id=$3)";
                crate::platform::observability::database(
                    "social",
                    statement,
                    sqlx::query_scalar(statement)
                        .bind(player)
                        .bind(first)
                        .bind(second)
                        .fetch_one(&mut *tx),
                )
                .await
            }?;
            anyhow::ensure!(count < 300, "Saved conversation limit reached");
        }
        let id: i32 = {
            let statement = "INSERT INTO social.private_conversations(first_id,second_id) VALUES($1,$2) ON CONFLICT(first_id,second_id) DO UPDATE SET first_id=EXCLUDED.first_id RETURNING id";
            crate::platform::observability::database(
                "social",
                statement,
                sqlx::query_scalar(statement)
                    .bind(first)
                    .bind(second)
                    .fetch_one(&mut *tx),
            )
            .await
        }?;
        retain_recent(&mut tx, owner, id).await?;
        {
            let statement = "UPDATE social.private_conversations SET first_open=first_open OR first_id=$2, second_open=second_open OR second_id=$2, last_used=extract(epoch FROM clock_timestamp()) WHERE id=$1";
            crate::platform::observability::database(
                "social",
                statement,
                sqlx::query(statement)
                    .bind(id)
                    .bind(owner)
                    .execute(&mut *tx),
            )
            .await
        }?;
        let room = load(&mut tx, id).await?;
        let room = room
            .with_names(&names)
            .ok_or_else(|| anyhow::anyhow!("Unknown player"))?;
        crate::platform::observability::database("social", "COMMIT", tx.commit()).await?;
        Ok(room)
    }

    #[tracing::instrument(name = "social.conversations.send_message", skip_all, fields(ddd.bounded_context.name = "social", input.owner = owner, input.conversation = conversation, input.text.bytes = text.len()), err)]
    pub(super) async fn send_message(
        &self,
        owner: i64,
        conversation: u32,
        text: &str,
    ) -> anyhow::Result<PrivateConversation> {
        let id = i32::try_from(conversation)?;
        let (first, second): (i64, i64) = {
            let statement = "SELECT first_id,second_id FROM social.private_conversations WHERE id=$1 AND (first_id=$2 OR second_id=$2)";
            crate::platform::observability::database(
                "social",
                statement,
                sqlx::query_as(statement)
                    .bind(id)
                    .bind(owner)
                    .fetch_one(&self.pool),
            )
            .await
        }?;
        let names = self.require_players(first, second).await?;
        let mut tx =
            crate::platform::observability::database("social", "BEGIN", self.pool.begin()).await?;
        lock_pair(&mut tx, first, second).await?;
        let subscribed: bool = {
            let statement = "SELECT CASE WHEN first_id=$2 THEN first_open ELSE second_open END FROM social.private_conversations WHERE id=$1";
            crate::platform::observability::database(
                "social",
                statement,
                sqlx::query_scalar(statement)
                    .bind(id)
                    .bind(owner)
                    .fetch_one(&mut *tx),
            )
            .await
        }?;
        anyhow::ensure!(subscribed, "Private window is closed");
        for player in [first, second] {
            retain_recent(&mut tx, player, id).await?;
        }
        {
            let statement = "INSERT INTO social.private_messages(conversation_id,sender_id,body) VALUES($1,$2,$3)";
            crate::platform::observability::database(
                "social",
                statement,
                sqlx::query(statement)
                    .bind(id)
                    .bind(owner)
                    .bind(text)
                    .execute(&mut *tx),
            )
            .await
        }?;
        {
            let statement = "DELETE FROM social.private_messages WHERE conversation_id=$1 AND id NOT IN (SELECT id FROM social.private_messages WHERE conversation_id=$1 ORDER BY id DESC LIMIT 100)";
            crate::platform::observability::database(
                "social",
                statement,
                sqlx::query(statement).bind(id).execute(&mut *tx),
            )
            .await
        }?;
        {
            let statement = "UPDATE social.private_conversations SET first_open=TRUE,second_open=TRUE,last_used=extract(epoch FROM clock_timestamp()) WHERE id=$1";
            crate::platform::observability::database(
                "social",
                statement,
                sqlx::query(statement).bind(id).execute(&mut *tx),
            )
            .await
        }?;
        let room = load(&mut tx, id).await?;
        let room = room
            .with_names(&names)
            .ok_or_else(|| anyhow::anyhow!("Unknown player"))?;
        crate::platform::observability::database("social", "COMMIT", tx.commit()).await?;
        Ok(room)
    }

    #[tracing::instrument(name = "social.conversations.close_conversation", skip_all, fields(ddd.bounded_context.name = "social", input.owner = owner, input.conversation = conversation), err)]
    pub(super) async fn close_conversation(
        &self,
        owner: i64,
        conversation: u32,
    ) -> anyhow::Result<()> {
        let mut tx =
            crate::platform::observability::database("social", "BEGIN", self.pool.begin()).await?;
        {
            let statement = "SELECT pg_advisory_xact_lock($1)";
            crate::platform::observability::database(
                "social",
                statement,
                sqlx::query(statement).bind(owner).execute(&mut *tx),
            )
            .await
        }?;
        let result = {
            let statement = "UPDATE social.private_conversations SET first_open=CASE WHEN first_id=$2 THEN FALSE ELSE first_open END, second_open=CASE WHEN second_id=$2 THEN FALSE ELSE second_open END WHERE id=$1 AND (first_id=$2 OR second_id=$2)";
            crate::platform::observability::database(
                "social",
                statement,
                sqlx::query(statement)
                    .bind(i32::try_from(conversation)?)
                    .bind(owner)
                    .execute(&mut *tx),
            )
            .await
        }?;
        anyhow::ensure!(result.rows_affected() == 1, "Unknown conversation");
        crate::platform::observability::database("social", "COMMIT", tx.commit()).await?;
        Ok(())
    }

    #[tracing::instrument(name = "social.conversations.restore_conversations", skip_all, fields(ddd.bounded_context.name = "social", input.owner = owner), err)]
    pub(super) async fn restore_conversations(
        &self,
        owner: i64,
    ) -> anyhow::Result<Vec<PrivateConversation>> {
        let mut tx =
            crate::platform::observability::database("social", "BEGIN", self.pool.begin()).await?;
        {
            let statement = "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY";
            crate::platform::observability::database(
                "social",
                statement,
                sqlx::query(statement).execute(&mut *tx),
            )
            .await
        }?;
        let ids: Vec<i32> = {
            let statement = "SELECT c.id FROM social.private_conversations c WHERE ((c.first_id=$1 AND c.first_open) OR (c.second_id=$1 AND c.second_open)) AND NOT EXISTS (SELECT 1 FROM social.social_contacts b WHERE b.ignored AND ((b.owner_id=c.first_id AND b.peer_id=c.second_id) OR (b.owner_id=c.second_id AND b.peer_id=c.first_id))) ORDER BY c.last_used DESC,c.id DESC LIMIT 16";
            crate::platform::observability::database(
                "social",
                statement,
                sqlx::query_scalar(statement)
                    .bind(owner)
                    .fetch_all(&mut *tx),
            )
            .await
        }?;
        let mut rooms = Vec::new();
        for id in ids {
            rooms.push(load(&mut tx, id).await?);
        }
        crate::platform::observability::database("social", "COMMIT", tx.commit()).await?;
        let ids: Vec<_> = rooms.iter().flat_map(|room| room.participants).collect();
        let names = self.player_names(&ids).await?;
        Ok(rooms
            .into_iter()
            .filter_map(|room| room.with_names(&names))
            .collect())
    }
}
