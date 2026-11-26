use super::{Completion as GatewayCompletion, Gateway};
use crate::{
    GrantRequestError,
    application::grants::{GrantRequest, GrantResult},
};
use tokio::sync::oneshot;
use wot_accounts::contract::AccountNotFound;
use wot_accounts::contract::garage::CommittedGarageChange;

pub(super) struct Completion {
    account_id: i64,
    reply: oneshot::Sender<GrantResult>,
    result: Result<CommittedGarageChange, GrantRequestError>,
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
            "user.id" = account_id,
            input.grant = ?grant
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
            .find(|s| s.account.snapshot.id() == account_id)
        {
            session.saving = true;
        }
        let garage = self.garage.clone();
        let sender = self.sender.clone();
        self.runtime
            .spawn(tracing::Instrument::in_current_span(async move {
                let result = async {
                    let result = garage.grant(account_id, grant).await.map_err(|error| {
                        if error.is::<AccountNotFound>() {
                            GrantRequestError::NotFound
                        } else {
                            error.into()
                        }
                    })?;
                    result?.ok_or(GrantRequestError::Busy)
                }
                .await;
                tokio::task::spawn_blocking(wot_platform::observability::in_current_span(
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
                revision = snapshot.revision(),
                ?delivery,
                "Committed admin grant"
            );
            (snapshot, delivery)
        });
        let _ = reply.send(result);
    }
}
