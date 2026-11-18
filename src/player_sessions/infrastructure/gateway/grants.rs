use super::{Completion as GatewayCompletion, Gateway, account_updates::CommittedAccountChange};
use crate::{
    accounts::AccountNotFound,
    player_sessions::{
        GrantRequestError,
        application::grants::{GrantRequest, GrantResult},
    },
};
use blowfish::Blowfish;
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::oneshot;

pub(super) struct Completion {
    account_id: i64,
    reply: oneshot::Sender<GrantResult>,
    result: Result<CommittedAccountChange, GrantRequestError>,
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
            "player_sessions.gateway.grant",
            "ddd.bounded_context.name" = "player_sessions",
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
                        crate::platform::observability::in_current_span(move || {
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
                        let _ = sender.send(super::TracedCompletion::new(
                            GatewayCompletion::Grant(Completion {
                                account_id,
                                reply,
                                result,
                            }),
                        ));
                    },
                ))
                .await
                .ok();
            }));
    }

    #[tracing::instrument(name = "player_sessions.gateway.grant_response", skip_all, fields(ddd.bounded_context.name = "player_sessions", user.id = completion.account_id, output.success = completion.result.is_ok()))]
    pub(super) fn complete_grant(&mut self, completion: Completion) {
        let Completion {
            account_id,
            reply,
            result,
        } = completion;
        self.release_account_write(account_id);
        let result = result.map(|committed| {
            let snapshot = committed.prepared.snapshot.clone();
            let delivery = self.publish_account_change(committed);
            tracing::info!(
                account_id,
                revision = snapshot.revision,
                ?delivery,
                "Committed admin grant"
            );
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
        let account_change_epoch = self.account_change_epoch;
        let accounts = self.accounts.clone();
        let catalog = self.catalog.clone();
        let sender = self.sender.clone();
        self.runtime
            .spawn(tracing::Instrument::in_current_span(async move {
                let _permit = permit;
                let result = async {
                    let snapshot = accounts.snapshot(account_id).await?;
                    tokio::task::spawn_blocking(crate::platform::observability::in_current_span(
                        move || catalog.prepare(snapshot),
                    ))
                    .await?
                    .map(Some)
                }
                .await;
                let _ = sender.try_send(super::TracedCompletion::new(GatewayCompletion::Login {
                    addr,
                    generation,
                    account_change_epoch,
                    blowfish,
                    result,
                }));
            }));
    }
}
