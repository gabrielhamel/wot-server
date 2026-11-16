use super::PostgresSocial;
use crate::social::{ChatMessage, Player, PrivateConversation, PrivateConversationBlocked};
use sqlx::{Postgres, Transaction};

type Tx<'a> = Transaction<'a, Postgres>;

async fn lock_pair(tx: &mut Tx<'_>, first: i64, second: i64) -> anyhow::Result<()> {
    for id in [first, second] {
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(id)
            .execute(&mut **tx)
            .await?;
    }
    let ignored: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM social_contacts WHERE ignored AND ((owner_id=$1 AND peer_id=$2) OR (owner_id=$2 AND peer_id=$1)))")
        .bind(first).bind(second).fetch_one(&mut **tx).await?;
    if ignored {
        return Err(PrivateConversationBlocked.into());
    }
    Ok(())
}

async fn retain_recent(tx: &mut Tx<'_>, owner: i64, conversation: i32) -> anyhow::Result<()> {
    sqlx::query("UPDATE private_conversations SET first_open=CASE WHEN first_id=$1 THEN FALSE ELSE first_open END, second_open=CASE WHEN second_id=$1 THEN FALSE ELSE second_open END WHERE id IN (SELECT id FROM private_conversations WHERE id<>$2 AND ((first_id=$1 AND first_open) OR (second_id=$1 AND second_open)) ORDER BY last_used DESC,id DESC OFFSET 15)")
        .bind(owner).bind(conversation).execute(&mut **tx).await?;
    Ok(())
}

async fn load(tx: &mut Tx<'_>, id: i32) -> anyhow::Result<PrivateConversation> {
    let (first, first_name, second, second_name): (i64, String, i64, String) = sqlx::query_as("SELECT c.first_id,a.display_name,c.second_id,b.display_name FROM private_conversations c JOIN account_directory a ON a.id=c.first_id JOIN account_directory b ON b.id=c.second_id WHERE c.id=$1")
        .bind(id).fetch_one(&mut **tx).await?;
    let rows: Vec<(i64,i64,String,f64,String)> = sqlx::query_as("SELECT m.id,m.sender_id,a.display_name,m.sent_at,m.body FROM private_messages m JOIN account_directory a ON a.id=m.sender_id WHERE conversation_id=$1 ORDER BY m.id DESC LIMIT 100")
        .bind(id).fetch_all(&mut **tx).await?;
    Ok(PrivateConversation {
        id: u32::try_from(id)?,
        players: [
            Player {
                id: first,
                name: first_name,
            },
            Player {
                id: second,
                name: second_name,
            },
        ],
        history: rows
            .into_iter()
            .rev()
            .map(|(id, sender, name, sent_at, text)| ChatMessage {
                id,
                sender: Player { id: sender, name },
                sent_at,
                text,
            })
            .collect(),
    })
}

impl PostgresSocial {
    pub(super) async fn open_conversation(
        &self,
        owner: i64,
        peer: i64,
    ) -> anyhow::Result<PrivateConversation> {
        let (first, second) = (owner.min(peer), owner.max(peer));
        let mut tx = self.pool.begin().await?;
        lock_pair(&mut tx, first, second).await?;
        for player in [owner, peer] {
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM private_conversations WHERE (first_id=$1 OR second_id=$1) AND NOT (first_id=$2 AND second_id=$3)")
                .bind(player).bind(first).bind(second).fetch_one(&mut *tx).await?;
            anyhow::ensure!(count < 300, "Saved conversation limit reached");
        }
        let id: i32 = sqlx::query_scalar("INSERT INTO private_conversations(first_id,second_id) VALUES($1,$2) ON CONFLICT(first_id,second_id) DO UPDATE SET first_id=EXCLUDED.first_id RETURNING id")
            .bind(first).bind(second).fetch_one(&mut *tx).await?;
        retain_recent(&mut tx, owner, id).await?;
        sqlx::query("UPDATE private_conversations SET first_open=first_open OR first_id=$2, second_open=second_open OR second_id=$2, last_used=extract(epoch FROM clock_timestamp()) WHERE id=$1")
            .bind(id).bind(owner).execute(&mut *tx).await?;
        let room = load(&mut tx, id).await?;
        tx.commit().await?;
        Ok(room)
    }

    pub(super) async fn send_message(
        &self,
        owner: i64,
        conversation: u32,
        text: &str,
    ) -> anyhow::Result<PrivateConversation> {
        let id = i32::try_from(conversation)?;
        let mut tx = self.pool.begin().await?;
        let (first, second): (i64,i64) = sqlx::query_as("SELECT first_id,second_id FROM private_conversations WHERE id=$1 AND (first_id=$2 OR second_id=$2)")
            .bind(id).bind(owner).fetch_one(&mut *tx).await?;
        lock_pair(&mut tx, first, second).await?;
        let subscribed: bool = sqlx::query_scalar("SELECT CASE WHEN first_id=$2 THEN first_open ELSE second_open END FROM private_conversations WHERE id=$1")
            .bind(id).bind(owner).fetch_one(&mut *tx).await?;
        anyhow::ensure!(subscribed, "Private window is closed");
        for player in [first, second] {
            retain_recent(&mut tx, player, id).await?;
        }
        sqlx::query(
            "INSERT INTO private_messages(conversation_id,sender_id,body) VALUES($1,$2,$3)",
        )
        .bind(id)
        .bind(owner)
        .bind(text)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM private_messages WHERE conversation_id=$1 AND id NOT IN (SELECT id FROM private_messages WHERE conversation_id=$1 ORDER BY id DESC LIMIT 100)").bind(id).execute(&mut *tx).await?;
        sqlx::query(
            "UPDATE private_conversations SET first_open=TRUE,second_open=TRUE,last_used=extract(epoch FROM clock_timestamp()) WHERE id=$1",
        )
        .bind(id)
        .execute(&mut *tx)
        .await?;
        let room = load(&mut tx, id).await?;
        tx.commit().await?;
        Ok(room)
    }

    pub(super) async fn close_conversation(
        &self,
        owner: i64,
        conversation: u32,
    ) -> anyhow::Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(owner)
            .execute(&mut *tx)
            .await?;
        let result = sqlx::query("UPDATE private_conversations SET first_open=CASE WHEN first_id=$2 THEN FALSE ELSE first_open END, second_open=CASE WHEN second_id=$2 THEN FALSE ELSE second_open END WHERE id=$1 AND (first_id=$2 OR second_id=$2)")
            .bind(i32::try_from(conversation)?).bind(owner).execute(&mut *tx).await?;
        anyhow::ensure!(result.rows_affected() == 1, "Unknown conversation");
        tx.commit().await?;
        Ok(())
    }

    pub(super) async fn restore_conversations(
        &self,
        owner: i64,
    ) -> anyhow::Result<Vec<PrivateConversation>> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *tx)
            .await?;
        let ids: Vec<i32> = sqlx::query_scalar("SELECT c.id FROM private_conversations c WHERE ((c.first_id=$1 AND c.first_open) OR (c.second_id=$1 AND c.second_open)) AND NOT EXISTS (SELECT 1 FROM social_contacts b WHERE b.ignored AND ((b.owner_id=c.first_id AND b.peer_id=c.second_id) OR (b.owner_id=c.second_id AND b.peer_id=c.first_id))) ORDER BY c.last_used DESC,c.id DESC LIMIT 16")
            .bind(owner).fetch_all(&mut *tx).await?;
        let mut rooms = Vec::new();
        for id in ids {
            rooms.push(load(&mut tx, id).await?);
        }
        tx.commit().await?;
        Ok(rooms)
    }
}
