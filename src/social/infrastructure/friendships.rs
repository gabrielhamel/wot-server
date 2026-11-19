use super::super::domain::{Friendship, FriendshipAction, Player};
use super::PostgresSocial;
use sqlx::{Postgres, Transaction};

type Tx<'a> = Transaction<'a, Postgres>;

#[tracing::instrument(name = "social.friendships.save", skip_all, fields(ddd.bounded_context.name = "social", input.owner = owner, input.peer = peer), err)]
async fn save(tx: &mut Tx<'_>, owner: i64, peer: i64, state: Friendship) -> anyhow::Result<bool> {
    let count: i64 = {
        let statement =
            "SELECT count(*) FROM social.social_contacts WHERE owner_id=$1 AND peer_id<>$2";
        crate::platform::observability::database(
            "social",
            statement,
            sqlx::query_scalar(statement)
                .bind(owner)
                .bind(peer)
                .fetch_one(&mut **tx),
        )
        .await
    }?;
    anyhow::ensure!(count < 300, "Contact limit reached");
    let stored_state: i16 = match state {
        Friendship::Saved => 0,
        Friendship::Pending => 1,
        Friendship::Accepted => 2,
    };
    let result = {
        let statement = "INSERT INTO social.social_contacts(owner_id,peer_id,friendship) VALUES($1,$2,$3) ON CONFLICT(owner_id,peer_id) DO UPDATE SET friendship=EXCLUDED.friendship WHERE NOT social_contacts.ignored AND social_contacts.friendship < EXCLUDED.friendship";
        crate::platform::observability::database(
            "social",
            statement,
            sqlx::query(statement)
                .bind(owner)
                .bind(peer)
                .bind(stored_state)
                .execute(&mut **tx),
        )
        .await
    }?;
    Ok(result.rows_affected() != 0)
}

impl PostgresSocial {
    #[tracing::instrument(name = "social.friendships.change_friendship", skip_all, fields(ddd.bounded_context.name = "social", input.owner = owner, input.peer = peer, input.action = ?action), err)]
    pub(super) async fn change_friendship(
        &self,
        owner: i64,
        peer: i64,
        action: FriendshipAction,
    ) -> anyhow::Result<bool> {
        use FriendshipAction::*;
        if matches!(action, Save | Request | Accept) {
            self.require_players(owner, peer).await?;
        }
        let mut tx =
            crate::platform::observability::database("social", "BEGIN", self.pool.begin()).await?;
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
        let blocked: bool = {
            let statement = "SELECT EXISTS(SELECT 1 FROM social.social_contacts WHERE ignored AND ((owner_id=$1 AND peer_id=$2) OR (owner_id=$2 AND peer_id=$1)))";
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
        anyhow::ensure!(
            !blocked || matches!(action, Remove | Decline | Cancel),
            "Friendship blocked"
        );
        let changed = match action {
            Save => save(&mut tx, owner, peer, Friendship::Saved).await?,
            Request => {
                let count: i64 = {
                    let statement = "SELECT count(*) FROM social.social_contacts WHERE peer_id=$1 AND owner_id<>$2 AND friendship=1 AND NOT ignored";
                    crate::platform::observability::database(
                        "social",
                        statement,
                        sqlx::query_scalar(statement)
                            .bind(peer)
                            .bind(owner)
                            .fetch_one(&mut *tx),
                    )
                    .await
                }?;
                anyhow::ensure!(count < 100, "Recipient has too many pending requests");
                save(&mut tx, owner, peer, Friendship::Pending).await?
            }
            Accept => {
                let incoming: bool = {
                    let statement = "SELECT EXISTS(SELECT 1 FROM social.social_contacts WHERE owner_id=$1 AND peer_id=$2 AND friendship=1 AND NOT ignored)";
                    crate::platform::observability::database(
                        "social",
                        statement,
                        sqlx::query_scalar(statement)
                            .bind(peer)
                            .bind(owner)
                            .fetch_one(&mut *tx),
                    )
                    .await
                }?;
                if incoming {
                    save(&mut tx, owner, peer, Friendship::Accepted).await?;
                    save(&mut tx, peer, owner, Friendship::Accepted).await?;
                }
                incoming
            }
            Decline | Cancel => {
                let (requester, target) = if matches!(action, Decline) {
                    (peer, owner)
                } else {
                    (owner, peer)
                };
                let statement = "UPDATE social.social_contacts SET friendship=0 WHERE owner_id=$1 AND peer_id=$2 AND friendship=1 AND NOT ignored";
                let result = crate::platform::observability::database(
                    "social",
                    statement,
                    sqlx::query(statement)
                        .bind(requester)
                        .bind(target)
                        .execute(&mut *tx),
                )
                .await?;
                result.rows_affected() != 0
            }
            Remove => {
                let statement = "DELETE FROM social.social_contacts WHERE owner_id=$1 AND peer_id=$2 AND NOT ignored";
                let result = crate::platform::observability::database(
                    "social",
                    statement,
                    sqlx::query(statement)
                        .bind(owner)
                        .bind(peer)
                        .execute(&mut *tx),
                )
                .await?;
                let removed = result.rows_affected() != 0;
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
                removed
            }
        };
        crate::platform::observability::database("social", "COMMIT", tx.commit()).await?;
        Ok(changed)
    }

    #[tracing::instrument(name = "social.friendships.pending_inbound", skip_all, fields(ddd.bounded_context.name = "social", input.owner = owner), err)]
    pub(super) async fn pending_inbound(&self, owner: i64) -> anyhow::Result<Vec<Player>> {
        let ids: Vec<i64> = {
            let statement = "SELECT c.owner_id FROM social.social_contacts c WHERE c.peer_id=$1 AND c.friendship=1 AND NOT c.ignored AND NOT EXISTS (SELECT 1 FROM social.social_contacts b WHERE b.owner_id=$1 AND b.peer_id=c.owner_id AND b.ignored) ORDER BY c.owner_id LIMIT 100";
            crate::platform::observability::database(
                "social",
                statement,
                sqlx::query_scalar(statement)
                    .bind(owner)
                    .fetch_all(&self.pool),
            )
            .await
        }?;
        let mut names = self.player_names(&ids).await?;
        Ok(ids
            .into_iter()
            .filter_map(|id| {
                Some(Player {
                    id,
                    name: names.remove(&id)?,
                })
            })
            .collect())
    }
}
