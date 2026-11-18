use super::{Gateway, TracedCompletion, account_updates::CommittedAccountChange};
use crate::{
    accounts::{AccountSnapshot, GarageCatalog, StoreOrder, StoreOrderError},
    player_sessions::application::store::{
        StoreCatalog, StoreError, StoreListing, StoreReply, StoreRequest, StoreRequests,
    },
};
use tokio::sync::mpsc;
use tracing::Instrument;

pub(super) struct StoreAccess {
    pub origin: String,
    pub requests: mpsc::Receiver<StoreRequest>,
}

pub(super) struct Completion {
    account_id: i64,
    reply: StoreReply,
    result: Result<CommittedAccountChange, StoreError>,
}

impl Gateway {
    pub fn enable_store(&mut self, origin: String) -> axum::Router {
        let (handle, requests) = StoreRequests::channel();
        self.store = Some(StoreAccess { origin, requests });
        crate::player_sessions::presentation::store::router(handle)
    }

    pub(super) fn handle_store(&mut self, request: StoreRequest) {
        let StoreRequest {
            parent,
            token,
            order,
            reply,
        } = request;
        parent.in_scope(|| {
            if reply.is_closed() {
                return;
            }
            let Some(session) = self.sessions.values_mut().find(|session| {
                session.garage_synced
                    && session
                        .store_token
                        .as_ref()
                        .is_some_and(|value| value.matches(&token))
            }) else {
                let _ = reply.send(Err(StoreError::Unauthorized));
                return;
            };
            let snapshot = &session.account.snapshot;
            let account_id = snapshot.id;
            tracing::Span::current().record("user.id", account_id);
            let Some(order) = order else {
                let _ = reply.send(Ok(store_catalog(&self.catalog, snapshot)));
                return;
            };

            if self.pending_writes.contains(&account_id) {
                let _ = reply.send(Err(StoreError::Busy));
                return;
            }
            let Ok(permit) = self.workers.clone().try_acquire_owned() else {
                let _ = reply.send(Err(StoreError::Busy));
                return;
            };
            self.pending_writes.insert(account_id);
            session.saving = true;
            self.start_store_purchase(account_id, order, reply, permit);
        });
    }

    #[tracing::instrument(name = "player_sessions.store.purchase", skip_all, fields(ddd.bounded_context.name = "player_sessions", user.id = account_id, input.item = order.item, input.price.credits = order.credits))]
    fn start_store_purchase(
        &self,
        account_id: i64,
        order: StoreOrder,
        reply: StoreReply,
        permit: tokio::sync::OwnedSemaphorePermit,
    ) {
        let accounts = self.accounts.clone();
        let catalog = self.catalog.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(
            async move {
                let result = async {
                    let previous = accounts.snapshot(account_id).await?;
                    let (change, prepared, diff) = tokio::task::spawn_blocking(
                        crate::platform::observability::in_current_span(move || {
                            let change = catalog.plan_store_purchase(previous.clone(), order)?;
                            let diff = catalog.progression_diff(&previous, &change)?;
                            let prepared = catalog.prepare(change.snapshot.clone())?;
                            Ok::<_, StoreError>((change, prepared, diff))
                        }),
                    )
                    .await
                    .map_err(anyhow::Error::from)??;
                    if !accounts.commit_progression(&change).await? {
                        return Err(StoreOrderError::Stale.into());
                    }
                    Ok(CommittedAccountChange {
                        previous_revision: change.previous_revision,
                        prepared,
                        diff,
                    })
                }
                .await;
                tokio::task::spawn_blocking(crate::platform::observability::in_current_span(
                    move || {
                        let _permit = permit;
                        let _ = sender.send(TracedCompletion::new(super::Completion::Store(
                            Completion {
                                account_id,
                                reply,
                                result,
                            },
                        )));
                    },
                ))
                .await
                .ok();
            }
            .in_current_span(),
        );
    }

    #[tracing::instrument(name = "player_sessions.store.complete_purchase", skip_all, fields(ddd.bounded_context.name = "player_sessions", user.id = completion.account_id, output.success = completion.result.is_ok()))]
    pub(super) fn complete_store(&mut self, completion: Completion) {
        self.release_account_write(completion.account_id);
        let result = completion.result.map(|change| {
            let catalog = store_catalog(&self.catalog, &change.prepared.snapshot);
            self.publish_account_change(change);
            catalog
        });
        let _ = completion.reply.send(result);
    }
}

#[tracing::instrument(name = "player_sessions.store.catalog", skip_all, fields(ddd.bounded_context.name = "player_sessions", user.id = snapshot.id, input.revision = snapshot.revision, output.offers.count = catalog.store_offers().len()))]
fn store_catalog(catalog: &GarageCatalog, snapshot: &AccountSnapshot) -> StoreCatalog {
    StoreCatalog {
        revision: snapshot.revision.to_string(),
        credits: snapshot.balances.credits,
        offers: catalog
            .store_offers()
            .iter()
            .map(|offer| StoreListing {
                offer: offer.clone(),
                in_depot: snapshot
                    .progression
                    .depot
                    .get(&offer.item)
                    .copied()
                    .unwrap_or_default(),
            })
            .collect(),
    }
}
