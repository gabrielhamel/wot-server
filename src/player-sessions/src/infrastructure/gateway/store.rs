use super::{Gateway, TracedCompletion};
use crate::application::store::{
    StoreCustomer, StoreError, StoreReply, StoreRequest, StoreRequests,
};
use std::{
    borrow::Cow,
    net::{IpAddr, SocketAddr},
};
use tokio::sync::mpsc;
use tracing::Instrument;
use wot_accounts::contract::CommittedGarageChange;
use wot_accounts::contract::{StoreOrder, StoreOrderError};

pub(super) struct StoreAccess {
    origin: StoreOrigin,
    pub requests: mpsc::Receiver<StoreRequest>,
}

enum StoreOrigin {
    Explicit { origin: String, host: String },
    Listener(SocketAddr),
}

impl StoreAccess {
    pub(super) fn origin_for(&self, client: SocketAddr) -> Option<(Cow<'_, str>, Cow<'_, str>)> {
        self.origin.for_client(client)
    }
}

impl StoreOrigin {
    fn for_client(&self, client: SocketAddr) -> Option<(Cow<'_, str>, Cow<'_, str>)> {
        match self {
            Self::Explicit { origin, host } => Some((origin.as_str().into(), host.as_str().into())),
            Self::Listener(bound) => {
                let IpAddr::V4(peer) = client.ip() else {
                    return None;
                };
                if !peer.is_loopback() && !peer.is_private() {
                    return None;
                }
                let address = super::super::endpoint::advertised_base(None, *bound, client).ok()?;
                if !(address.ip().is_private()
                    || (peer.is_loopback() && address.ip().is_loopback()))
                {
                    return None;
                }
                Some((
                    format!("http://{address}").into(),
                    address.ip().to_string().into(),
                ))
            }
        }
    }
}

pub(super) struct Completion {
    account_id: i64,
    reply: StoreReply,
    result: Result<CommittedGarageChange, StoreError>,
}

impl Gateway {
    #[cfg(feature = "integration-test-support")]
    pub fn enable_store(&mut self, origin: String) -> anyhow::Result<axum::Router> {
        self.configure_store(origin)
            .map(crate::presentation::store::router)
    }

    pub fn configure_store(&mut self, origin: String) -> anyhow::Result<StoreRequests> {
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
        let host = host.to_ascii_lowercase();
        Ok(self.set_store_origin(StoreOrigin::Explicit { origin, host }))
    }

    /// Advertise a local Store origin based on the bound HTTP listener and the game client.
    pub fn configure_store_on_listener(&mut self, bound: SocketAddr) -> StoreRequests {
        self.set_store_origin(StoreOrigin::Listener(bound))
    }

    fn set_store_origin(&mut self, origin: StoreOrigin) -> StoreRequests {
        let (handle, requests) = StoreRequests::channel();
        self.store = Some(StoreAccess { origin, requests });
        handle
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
                        tracing::Span::current().record("user.id", snapshot.id());
                        let _ = reply.send(Ok(self.catalog.store_catalog(snapshot)));
                        return;
                    }
                    snapshot.id()
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
                .find(|session| session.account.snapshot.id() == account_id)
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
                    tokio::task::spawn_blocking(wot_platform::observability::in_current_span(
                        move || catalog.store_catalog(&snapshot),
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
        let garage = self.garage.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(
            async move {
                let result = async {
                    garage
                        .purchase_offer(account_id, order)
                        .await??
                        .ok_or_else(|| StoreOrderError::Stale.into())
                }
                .await;
                tokio::task::spawn_blocking(wot_platform::observability::in_current_span(
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
            let catalog = self.catalog.store_catalog(&change.prepared.snapshot);
            self.publish_account_change(change);
            catalog
        });
        let _ = completion.reply.send(result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_a_local_store_url_using_the_listener_and_client() {
        let wildcard = StoreOrigin::Listener("0.0.0.0:43210".parse().unwrap());
        let lan = StoreOrigin::Listener("192.168.50.3:8080".parse().unwrap());
        let loopback = StoreOrigin::Listener("127.0.0.1:8080".parse().unwrap());
        let local_client = "127.0.0.1:42000".parse().unwrap();
        let lan_client = "192.168.50.42:42000".parse().unwrap();
        let public_client = "198.51.100.42:42000".parse().unwrap();

        assert_eq!(
            wildcard.for_client(local_client),
            Some(("http://127.0.0.1:43210".into(), "127.0.0.1".into()))
        );
        assert_eq!(
            lan.for_client(lan_client),
            Some(("http://192.168.50.3:8080".into(), "192.168.50.3".into()))
        );
        assert!(loopback.for_client(lan_client).is_none());
        assert!(lan.for_client(public_client).is_none());
        assert!(wildcard.for_client(public_client).is_none());
    }
}
