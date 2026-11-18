use super::{Account, ClientKey, EntityKey, FAILURE, Gateway, Pickle, PreparedAccount, SUCCESS};
use crate::{
    accounts::{AmmoError, ShellSelection},
    platform::client_formats::pickle::Value,
};

pub(super) const SET_AND_FILL_LAYOUTS: i16 = 108;
const WRONG_ARGS: i16 = -2;
const SHOP_DESYNC: i16 = -4;
const BUSY: i16 = -5;
const MAX_SHELL_TYPES: usize = 16;

struct Request {
    shop_revision: i32,
    inventory_id: i64,
    shells: Vec<ShellSelection>,
}

impl Request {
    fn decode(args: &[i32]) -> Option<Self> {
        let &[shop_revision, inventory_id, shell_len, ref payload @ ..] = args else {
            return None;
        };
        let len = usize::try_from(shell_len).ok()?;
        if inventory_id <= 0 || len == 0 || len > 2 * MAX_SHELL_TYPES {
            return None;
        }
        let (shells, equipment) = payload.split_at_checked(len)?;
        // The shell-only client command supplies equipment type 0 and no equipment.
        if equipment != [0, 0] {
            return None;
        }
        let shells = decode_shells(shells)?;
        Some(Self {
            shop_revision,
            inventory_id: i64::from(inventory_id),
            shells,
        })
    }
}

pub(super) fn decode_shells(shells: &[i32]) -> Option<Vec<ShellSelection>> {
    if shells.len() > 2 * MAX_SHELL_TYPES {
        return None;
    }
    let (pairs, []) = shells.as_chunks::<2>() else {
        return None;
    };
    pairs
        .iter()
        .map(|&[id, count]| {
            Some(ShellSelection {
                id: id.checked_abs().filter(|&id| id > 0)?,
                count: u32::try_from(count).ok()?,
                alternate_price: id < 0,
            })
        })
        .collect()
}

pub(super) struct Completion {
    client: ClientKey,
    account_id: i64,
    request_id: i16,
    result: anyhow::Result<Outcome>,
}

enum Outcome {
    Unchanged,
    Committed {
        prepared: Box<PreparedAccount>,
        diff: Value,
        receipt: Value,
    },
    Rejected(AmmoError),
    StaleAccount,
}

impl Gateway {
    #[tracing::instrument(name = "player_sessions.gateway.change_ammunition", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.request_id = request_id, input.args.count = args.len(), input.inventory_id = tracing::field::Empty, input.shop_revision = tracing::field::Empty, input.requested.count = tracing::field::Empty, output.outcome = tracing::field::Empty), err)]
    pub(super) fn change_ammunition(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
        request_id: i16,
        args: &[i32],
    ) -> anyhow::Result<()> {
        let Some(session) = self.sessions.get_mut(&client) else {
            return Ok(());
        };
        let span = tracing::Span::current();
        let Some(request) = Request::decode(args) else {
            span.record("output.outcome", "invalid_request");
            self.base.entity(entity)?.onCmdResponse(
                request_id,
                WRONG_ARGS,
                "invalidShells".into(),
            )?;
            return Ok(());
        };
        span.record("input.inventory_id", request.inventory_id);
        span.record("input.shop_revision", request.shop_revision);
        span.record("input.requested.count", request.shells.len());
        let shop_revision = self.catalog.shop_revision();
        if request.shop_revision != shop_revision {
            span.record("output.outcome", "stale_shop");
            self.base.entity(entity)?.onCmdResponseExt(
                request_id,
                SHOP_DESYNC,
                String::new(),
                Pickle(crate::pickle!({"shopRev": shop_revision})),
            )?;
            return Ok(());
        }
        let permit = (!session.saving)
            .then(|| self.workers.clone().try_acquire_owned().ok())
            .flatten();
        let Some(permit) = permit else {
            span.record("output.outcome", "busy");
            self.base
                .entity(entity)?
                .onCmdResponse(request_id, BUSY, "accountBusy".into())?;
            return Ok(());
        };
        let previous = session.account.snapshot.clone();
        let account_id = previous.id;
        span.record("output.outcome", "queued");
        session.saving = true;
        self.pending_writes.insert(account_id);
        let catalog = self.catalog.clone();
        let accounts = self.accounts.clone();
        let sender = self.sender.clone();
        self.runtime
            .spawn(tracing::Instrument::in_current_span(async move {
                let result = async {
                    let projection = tokio::task::spawn_blocking(
                        crate::platform::observability::in_current_span(move || {
                            let change = match catalog.plan_ammunition(
                                previous.clone(),
                                request.inventory_id,
                                &request.shells,
                            ) {
                                Ok(change) => change,
                                Err(error) => return Ok(Err(error)),
                            };
                            if !change.changed() {
                                return Ok(Ok(None));
                            }
                            let diff = catalog.ammunition_diff(&previous, &change)?;
                            let prepared = catalog.prepare(change.snapshot.clone())?;
                            let receipt = catalog.ammunition_response(&change);
                            Ok::<_, anyhow::Error>(Ok(Some((change, prepared, diff, receipt))))
                        }),
                    )
                    .await??;
                    let (change, prepared, diff, receipt) = match projection {
                        Ok(Some(values)) => values,
                        Ok(None) => return Ok(Outcome::Unchanged),
                        Err(error) => return Ok(Outcome::Rejected(error)),
                    };
                    if !accounts.commit_ammunition(&change).await? {
                        return Ok(Outcome::StaleAccount);
                    }
                    Ok(Outcome::Committed {
                        prepared: Box::new(prepared),
                        diff,
                        receipt,
                    })
                }
                .await;
                // Hold the worker permit until the completion is queued, even after disconnect.
                tokio::task::spawn_blocking(crate::platform::observability::in_current_span(
                    move || {
                        let _permit = permit;
                        let _ = sender.send(super::TracedCompletion::new(
                            super::Completion::Ammunition(Completion {
                                client,
                                account_id,
                                request_id,
                                result,
                            }),
                        ));
                    },
                ))
                .await
                .ok();
            }));
        Ok(())
    }

    #[tracing::instrument(name = "player_sessions.gateway.ammunition_response", skip_all, fields(ddd.bounded_context.name = "player_sessions", user.id = completion.account_id, input.request_id = completion.request_id, output.outcome = tracing::field::Empty, output.delivery = tracing::field::Empty))]
    pub(super) fn complete_ammunition(&mut self, completion: Completion) {
        let Completion {
            client,
            account_id,
            request_id,
            result,
        } = completion;
        let span = tracing::Span::current();
        span.record(
            "output.outcome",
            match &result {
                Ok(Outcome::Committed { .. }) => "committed",
                Ok(Outcome::Unchanged) => "unchanged",
                Ok(Outcome::Rejected(_)) => "rejected",
                Ok(Outcome::StaleAccount) => "stale",
                Err(_) => "error",
            },
        );
        if let Err(error) = &result {
            tracing::error!(%error, account_id, "Apply ammunition layout failed");
        }
        self.finish_account_write(account_id);
        let Some(session) = self.sessions.get_mut(&client) else {
            span.record("output.delivery", "unavailable");
            return;
        };
        session.saving = false;
        let Some(entity) = session.entity else {
            span.record("output.delivery", "unavailable");
            return;
        };
        let response = self
            .base
            .entity(entity)
            .and_then(|mut account| match result {
                Ok(Outcome::Unchanged) => account.onCmdResponseExt(
                    request_id,
                    SUCCESS,
                    String::new(),
                    Pickle(crate::pickle!({})),
                ),
                Ok(Outcome::Committed {
                    prepared,
                    diff,
                    receipt,
                }) => {
                    session.account = *prepared;
                    account.update(Pickle(diff))?;
                    account.onCmdResponseExt(request_id, SUCCESS, String::new(), Pickle(receipt))
                }
                Ok(Outcome::Rejected(error)) => {
                    account.onCmdResponse(request_id, FAILURE, error.to_string())
                }
                Ok(Outcome::StaleAccount) => {
                    account.onCmdResponse(request_id, FAILURE, "accountChanged".into())
                }
                Err(_) => account.onCmdResponse(request_id, FAILURE, "databaseUnavailable".into()),
            });
        match response {
            Ok(()) => {
                span.record("output.delivery", "queued");
            }
            Err(error) => {
                span.record("output.delivery", "failed");
                tracing::error!(%error, account_id, "Send ammunition response failed");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_malformed_and_mixed_equipment_requests() {
        for args in [
            vec![],
            vec![1, 42],
            vec![1, 42, -1, 0, 0],
            vec![1, 42, 0, 0, 0],
            vec![1, 42, 33, 0, 0],
            vec![1, 42, 2, 10],
            vec![1, 42, 1, 10, 0, 0],
            vec![1, 42, 2, 0, 1, 0, 0],
            vec![1, 42, 2, i32::MIN, 1, 0, 0],
            vec![1, 42, 2, 10, -1, 0, 0],
            vec![1, 42, 2, 10, 1, 1, 0],
            vec![1, 42, 2, 10, 1, 0, 2, 0, 0],
            vec![1, 42, 2, 10, 1, 0, 0, 0],
        ] {
            assert!(Request::decode(&args).is_none(), "{args:?}");
        }
        let valid = Request::decode(&[1, 42, 2, -20, 3, 0, 0]).unwrap();
        assert_eq!(valid.shells[0].id, 20);
        assert_eq!(valid.shells[0].count, 3);
        assert!(valid.shells[0].alternate_price);
    }
}
