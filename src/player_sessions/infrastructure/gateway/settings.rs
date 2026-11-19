use super::*;

impl Gateway {
    #[tracing::instrument(name = "player_sessions.settings.update", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.request_id = request_id, input.args.count = args.len(), output.outcome = tracing::field::Empty), err)]
    pub(super) fn change_settings(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
        request_id: i16,
        args: &[i32],
    ) -> Result<()> {
        let Some(session) = self.sessions.get_mut(&client) else {
            return Ok(());
        };
        let (pairs, remainder) = args.as_chunks::<2>();
        if !remainder.is_empty() || session.saving {
            let outcome = if !remainder.is_empty() {
                "invalid_request"
            } else {
                "busy"
            };
            tracing::Span::current().record("output.outcome", outcome);
            self.base.entity(entity)?.onCmdResponse(
                request_id,
                FAILURE,
                "Settings request invalid or busy".into(),
            )?;
            return Ok(());
        }
        tracing::Span::current().record("output.outcome", "queued");
        session.saving = true;
        let changes: BTreeMap<_, _> = pairs.iter().map(|&[key, value]| (key, value)).collect();
        let id = session.account.snapshot.id();
        self.pending_writes.insert(id);
        let accounts = self.accounts.clone();
        let sender = self.sender.clone();
        let catalog = self.catalog.clone();
        self.runtime
            .spawn(tracing::Instrument::in_current_span(async move {
                let result = async {
                    let snapshot = accounts.change_settings(id, changes.clone()).await?;
                    tokio::task::spawn_blocking(crate::platform::observability::in_current_span(
                        move || catalog.prepare(snapshot),
                    ))
                    .await?
                }
                .await;
                tokio::task::spawn_blocking(crate::platform::observability::in_current_span(
                    move || {
                        let _ = sender.send(super::TracedCompletion::new(Completion::Settings {
                            client,
                            account_id: id,
                            request_id,
                            changes,
                            result,
                        }));
                    },
                ))
                .await
                .ok();
            }));

        Ok(())
    }

    #[tracing::instrument(name = "player_sessions.settings.response", skip_all, fields(ddd.bounded_context.name = "player_sessions", user.id = account_id, input.request_id = request_id, input.changes.count = changes.len(), output.outcome = tracing::field::Empty, output.delivery = tracing::field::Empty))]
    pub(super) fn complete_settings(
        &mut self,
        client: ClientKey,
        account_id: i64,
        request_id: i16,
        changes: BTreeMap<i32, i32>,
        result: Result<PreparedAccount>,
    ) {
        let span = tracing::Span::current();
        span.record(
            "output.outcome",
            if result.is_ok() { "committed" } else { "error" },
        );
        if let Err(error) = &result {
            error!(%error, "Persist settings failed");
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
        let response = match result {
            Ok(prepared) => {
                let revision = prepared.snapshot.revision();
                let previous = session.account.snapshot.revision();
                session.account = prepared;
                let diff = crate::pickle!({"prevRev": previous, "rev": revision, "intUserSettings": (Value::Dict(changes.into_iter().map(|(k,v)| (Value::from(k),Value::from(v))).collect()))});
                self.base.entity(entity).and_then(|mut account| {
                    account.update(Pickle(diff))?;
                    account.onCmdResponse(request_id, SUCCESS, String::new())
                })
            }
            Err(_) => self.base.entity(entity).and_then(|mut account| {
                account.onCmdResponse(request_id, FAILURE, "Database unavailable".into())
            }),
        };
        match response {
            Ok(()) => {
                span.record("output.delivery", "queued");
            }
            Err(error) => {
                span.record("output.delivery", "failed");
                error!(%error, "Send settings result failed");
            }
        }
    }
}
