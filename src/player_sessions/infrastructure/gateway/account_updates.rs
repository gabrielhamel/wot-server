use super::{Gateway, Pickle};
use crate::{accounts::contract::CommittedGarageChange, player_sessions::GrantDelivery};

impl Gateway {
    pub(super) fn finish_account_write(&mut self, account_id: i64) {
        self.account_change_epoch = self.account_change_epoch.wrapping_add(1);
        self.pending_writes.remove(&account_id);
    }

    pub(super) fn release_account_write(&mut self, account_id: i64) {
        self.finish_account_write(account_id);
        if let Some(session) = self
            .sessions
            .values_mut()
            .find(|s| s.account.snapshot.id() == account_id)
        {
            session.saving = false;
        }
    }

    #[tracing::instrument(name = "player_sessions.publish_account_change", skip_all, fields(ddd.bounded_context.name = "player_sessions", user.id = change.prepared.snapshot.id(), input.previous_revision = change.previous_revision, output.revision = change.prepared.snapshot.revision(), output.delivery = tracing::field::Empty, otel.status_code = tracing::field::Empty))]
    pub(super) fn publish_account_change(
        &mut self,
        change: CommittedGarageChange,
    ) -> GrantDelivery {
        let account_id = change.prepared.snapshot.id();
        let mut delivery = GrantDelivery::OnNextSync;
        if let Some((client, session)) = self
            .sessions
            .iter_mut()
            .find(|(_, session)| session.account.snapshot.id() == account_id)
        {
            let previous_revision = session.account.snapshot.revision();
            session.account = change.prepared;
            if let Some(entity) = session.entity.filter(|_| session.garage_synced) {
                let send = if previous_revision == change.previous_revision {
                    self.base
                        .entity(entity)
                        .and_then(|mut account| account.update(Pickle(change.diff)))
                } else {
                    Err(std::io::Error::other(
                        "garage revision differs from the change base",
                    ))
                };
                match send {
                    Ok(()) => delivery = GrantDelivery::QueuedForGarage,
                    Err(error) => {
                        let span = tracing::Span::current();
                        span.record("otel.status_code", "ERROR");
                        tracing::warn!(%error, account_id, "Reclaiming garage session after account update delivery failed");
                        self.base.disconnect_client(*client);
                    }
                }
            }
        } else if let Some(ticket) = self
            .tickets
            .values_mut()
            .find(|ticket| ticket.account.snapshot.id() == account_id)
        {
            ticket.account = change.prepared;
        }
        tracing::Span::current().record(
            "output.delivery",
            match delivery {
                GrantDelivery::QueuedForGarage => "queued",
                GrantDelivery::OnNextSync => "next_sync",
            },
        );
        delivery
    }
}
