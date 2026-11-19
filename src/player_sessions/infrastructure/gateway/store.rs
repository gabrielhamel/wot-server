use super::{Gateway, TracedCompletion, account_updates::CommittedAccountChange};
use crate::{
    accounts::{AccountSnapshot, GarageCatalog, StoreOrder, StoreOrderError},
    player_sessions::application::store::{
        StoreCatalog, StoreCustomer, StoreError, StoreListing, StoreReply, StoreRequest,
        StoreRequests,
    },
};
use tokio::sync::mpsc;
use tracing::Instrument;

pub(super) struct StoreAccess {
    pub origin: String,
    pub host: String,
    pub requests: mpsc::Receiver<StoreRequest>,
}

pub(super) struct Completion {
    account_id: i64,
    reply: StoreReply,
    result: Result<CommittedAccountChange, StoreError>,
}

impl Gateway {
    pub fn enable_store(&mut self, origin: String) -> anyhow::Result<axum::Router> {
        self.configure_store(origin)
            .map(crate::player_sessions::presentation::store::router)
    }

    pub(crate) fn configure_store(&mut self, origin: String) -> anyhow::Result<StoreRequests> {
        let uri: axum::http::Uri = origin.parse()?;
        let host = uri
            .host()
            .ok_or_else(|| anyhow::anyhow!("Store origin requires a host"))?;
        anyhow::ensure!(
            host.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
                || host
                    .strip_prefix('[')
                    .and_then(|host| host.strip_suffix(']'))
                    .is_some_and(|host| host.parse::<std::net::Ipv6Addr>().is_ok()),
            "Store origin requires an exact hostname or IP address"
        );
        let (handle, requests) = StoreRequests::channel();
        self.store = Some(StoreAccess {
            origin,
            host: host.to_ascii_lowercase(),
            requests,
        });
        Ok(handle)
    }

    pub(super) fn handle_store(&mut self, request: StoreRequest) {
        let StoreRequest {
            parent,
            customer,
            order,
            reply,
        } = request;
        parent.in_scope(|| {
            if reply.is_closed() {
                return;
            }
            let account_id = match customer {
                StoreCustomer::Dashboard(account_id) => account_id,
                StoreCustomer::Garage(token) => {
                    let Some(session) = self.sessions.values().find(|session| {
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
                    if order.is_none() {
                        tracing::Span::current().record("user.id", snapshot.id);
                        let _ = reply.send(Ok(store_catalog(&self.catalog, snapshot)));
                        return;
                    }
                    snapshot.id
                }
            };
            tracing::Span::current().record("user.id", account_id);
            let Some(order) = order else {
                self.load_dashboard_catalog(account_id, reply);
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
            if let Some(session) = self
                .sessions
                .values_mut()
                .find(|session| session.account.snapshot.id == account_id)
            {
                session.saving = true;
            }
            self.start_store_purchase(account_id, order, reply, permit);
        });
    }

    #[tracing::instrument(name = "player_sessions.store.load_catalog", skip_all, fields(ddd.bounded_context.name = "player_sessions", user.id = account_id, output.outcome = tracing::field::Empty, otel.status_code = tracing::field::Empty))]
    fn load_dashboard_catalog(&self, account_id: i64, reply: StoreReply) {
        let Ok(permit) = self.workers.clone().try_acquire_owned() else {
            tracing::Span::current().record("output.outcome", "busy");
            let _ = reply.send(Err(StoreError::Busy));
            return;
        };
        let accounts = self.accounts.clone();
        let catalog = self.catalog.clone();
        self.runtime.spawn(
            async move {
                let _permit = permit;
                let result = async {
                    let snapshot = accounts.snapshot(account_id).await?;
                    tokio::task::spawn_blocking(crate::platform::observability::in_current_span(
                        move || store_catalog(&catalog, &snapshot),
                    ))
                    .await
                    .map_err(anyhow::Error::from)
                }
                .await;
                let span = tracing::Span::current();
                if result.is_err() {
                    span.record("otel.status_code", "ERROR");
                    span.record("output.outcome", "failed");
                    tracing::error!("Could not load the dashboard Store catalog");
                } else {
                    span.record("output.outcome", "success");
                }
                let _ = reply.send(result.map_err(StoreError::from));
            }
            .in_current_span(),
        );
    }

    #[tracing::instrument(name = "player_sessions.store.purchase", skip_all, fields(ddd.bounded_context.name = "player_sessions", user.id = account_id, input.item = order.item, input.revision = order.revision, input.price.currency = order.price.currency(), input.price.amount = order.price.amount()))]
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
    let tiers = catalog.max_researched_tiers(snapshot);
    StoreCatalog {
        revision: snapshot.revision.to_string(),
        premium_expires_at: snapshot.active_premium_until(),
        offers: catalog
            .store_offers()
            .iter()
            .map(|offer| StoreListing {
                offer: offer.clone(),
                availability: catalog.store_availability(snapshot, &tiers, offer),
                in_depot: catalog.store_depot_count(snapshot, offer),
            })
            .collect(),
    }
}
