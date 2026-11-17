use super::{Completion as GatewayCompletion, Gateway, Pickle, PreparedAccount};
use crate::{
    access::{
        GrantDelivery, GrantRequestError,
        application::grants::{GrantRequest, GrantResult},
    },
    accounts::AccountNotFound,
    client_formats::pickle::Value,
};
use blowfish::Blowfish;
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::oneshot;

pub(super) struct Completion {
    account_id: i64,
    reply: oneshot::Sender<GrantResult>,
    result: Result<CommittedGrant, GrantRequestError>,
}

struct CommittedGrant {
    previous_revision: i64,
    prepared: PreparedAccount,
    diff: Value,
}

impl Gateway {
    pub(super) fn start_grant(&mut self, request: GrantRequest) {
        let GrantRequest {
            parent,
            account_id,
            grant,
            reply,
        } = request;
        let _request = parent.enter();
        let span = tracing::info_span!(
            "access.infrastructure.gateway.grant",
            "code.layer" = "infrastructure",
            "wot.context" = "access",
            "user.id" = account_id
        );
        let _grant = span.enter();
        if self.pending_writes.contains(&account_id) {
            let _ = reply.send(Err(GrantRequestError::Busy));
            return;
        }
        let Ok(permit) = self.workers.clone().try_acquire_owned() else {
            let _ = reply.send(Err(GrantRequestError::Busy));
            return;
        };
        self.pending_writes.insert(account_id);
        if let Some(session) = self
            .sessions
            .values_mut()
            .find(|s| s.account.snapshot.id == account_id)
        {
            session.saving = true;
        }
        let accounts = self.accounts.clone();
        let catalog = self.catalog.clone();
        let sender = self.sender.clone();
        self.runtime
            .spawn(tracing::Instrument::in_current_span(async move {
                let result = async {
                    let previous = accounts.snapshot(account_id).await.map_err(|error| {
                        if error.is::<AccountNotFound>() {
                            GrantRequestError::NotFound
                        } else {
                            error.into()
                        }
                    })?;
                    let (change, prepared, diff) = tokio::task::spawn_blocking(
                        crate::observability::in_current_span(move || {
                            let change = catalog.plan_grant(previous.clone(), grant)?;
                            let diff = catalog.grant_diff(&previous, &change, grant)?;
                            let prepared = catalog.prepare(change.snapshot.clone())?;
                            Ok::<_, GrantRequestError>((change, prepared, diff))
                        }),
                    )
                    .await
                    .map_err(anyhow::Error::from)??;
                    if !accounts.commit_progression(&change).await? {
                        return Err(GrantRequestError::Busy);
                    }
                    Ok(CommittedGrant {
                        previous_revision: change.previous_revision,
                        prepared,
                        diff,
                    })
                }
                .await;
                tokio::task::spawn_blocking(crate::observability::in_current_span(move || {
                    let _permit = permit;
                    let _ = sender.send(super::TracedCompletion::new(GatewayCompletion::Grant(
                        Completion {
                            account_id,
                            reply,
                            result,
                        },
                    )));
                }))
                .await
                .ok();
            }));
    }

    #[tracing::instrument(name = "access.infrastructure.gateway.grant_response", skip_all, fields(code.layer = "infrastructure", wot.context = "access", user.id = completion.account_id, output.success = completion.result.is_ok()))]
    pub(super) fn complete_grant(&mut self, completion: Completion) {
        let Completion {
            account_id,
            reply,
            result,
        } = completion;
        self.pending_writes.remove(&account_id);
        let mut session = self
            .sessions
            .iter_mut()
            .find(|(_, session)| session.account.snapshot.id == account_id);
        if let Some((_, session)) = &mut session {
            session.saving = false;
        }
        let result = result.map(|committed| {
            self.grant_epoch = self.grant_epoch.wrapping_add(1);
            let snapshot = committed.prepared.snapshot.clone();
            let mut delivery = GrantDelivery::OnNextSync;
            if let Some((client, session)) = session {
                let previous_revision = session.account.snapshot.revision;
                session.account = committed.prepared;
                if let Some(entity) = session.entity.filter(|_| session.garage_synced) {
                    let send = if previous_revision == committed.previous_revision {
                        self.base.entity(entity).and_then(|mut account| account.update(Pickle(committed.diff)))
                    } else {
                        Err(std::io::Error::other("garage revision differs from the grant base"))
                    };
                    match send {
                        Ok(()) => delivery = GrantDelivery::QueuedForGarage,
                        Err(error) => {
                            tracing::warn!(%error, account_id, "Reclaiming garage session after grant delivery failed");
                            self.base.disconnect_client(*client);
                        }
                    }
                }
            } else if let Some(ticket) = self.tickets.values_mut().find(|ticket| ticket.account.snapshot.id == account_id) {
                ticket.account = committed.prepared;
            }
            tracing::info!(account_id, revision = snapshot.revision, ?delivery, "Committed admin grant");
            (snapshot, delivery)
        });
        let _ = reply.send(result);
    }

    pub(super) fn refresh_admission(
        &mut self,
        addr: SocketAddr,
        generation: u64,
        blowfish: Arc<Blowfish>,
        account_id: i64,
    ) {
        let Ok(permit) = self.workers.clone().try_acquire_owned() else {
            self.admissions
                .complete(addr, generation, std::time::Instant::now());
            self.login
                .answer_login_error(addr, super::LoginError::RateLimited, String::new());
            return;
        };
        let grant_epoch = self.grant_epoch;
        let accounts = self.accounts.clone();
        let catalog = self.catalog.clone();
        let sender = self.sender.clone();
        self.runtime
            .spawn(tracing::Instrument::in_current_span(async move {
                let _permit = permit;
                let result = async {
                    let snapshot = accounts.snapshot(account_id).await?;
                    tokio::task::spawn_blocking(crate::observability::in_current_span(move || {
                        catalog.prepare(snapshot)
                    }))
                    .await?
                    .map(Some)
                }
                .await;
                let _ = sender.try_send(super::TracedCompletion::new(GatewayCompletion::Login {
                    addr,
                    generation,
                    grant_epoch,
                    blowfish,
                    result,
                }));
            }));
    }
}
