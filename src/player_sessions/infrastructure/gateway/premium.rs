use super::{Account, ClientKey, EntityKey, GarageCatalog, Gateway, Pickle};

impl Gateway {
    pub(super) fn expire_premium(&mut self) {
        for (client, session) in &mut self.sessions {
            let snapshot = &session.account.snapshot;
            let Some(expiry) = snapshot.expired_premium_at() else {
                continue;
            };
            if session.reported_premium_expiry == Some(expiry) {
                continue;
            }
            if let Some(entity) = session.entity.filter(|_| session.garage_synced) {
                session.reported_premium_expiry = Some(expiry);
                Self::send_premium_expiry(
                    &mut self.base,
                    *client,
                    entity,
                    snapshot.id,
                    snapshot.revision,
                    expiry,
                );
            }
        }
    }

    #[tracing::instrument(name = "player_sessions.premium.expire", skip_all, fields(ddd.bounded_context.name = "player_sessions", user.id = account_id, input.revision = revision, input.premium.expires_at = expiry, output.delivery = tracing::field::Empty, otel.status_code = tracing::field::Empty))]
    fn send_premium_expiry(
        base: &mut super::base::App,
        client: ClientKey,
        entity: EntityKey<Account>,
        account_id: i64,
        revision: i64,
        expiry: u32,
    ) {
        let span = tracing::Span::current();
        match base.entity(entity).and_then(|mut account| {
            account.update(Pickle(GarageCatalog::premium_sync(revision, None)))
        }) {
            Ok(()) => {
                span.record("output.delivery", "queued");
            }
            Err(error) => {
                span.record("output.delivery", "failed");
                span.record("otel.status_code", "ERROR");
                tracing::warn!(%error, "Reclaiming garage session after premium expiry delivery failed");
                base.disconnect_client(client);
            }
        }
    }
}
