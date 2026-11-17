use super::*;

impl Gateway {
    #[tracing::instrument(name = "mercury.response.resource", skip_all, fields(ddd.bounded_context.name = "access", input.request_id = request_id, output.bytes = data.len()), err)]
    fn stream(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
        request_id: i16,
        data: &[u8],
    ) -> Result<()> {
        tracing::debug!(%client, request_id, bytes = data.len(), "Sending garage resource");
        self.base
            .entity(entity)?
            .onCmdResponse(request_id, STREAM, String::new())?;
        let description = pickle::to_bytes(&crate::pickle!((
            (data.len() as i64),
            (crc32fast::hash(data) as i32)
        )));
        self.base
            .push_resource(client, request_id as u16, description, data)?;
        Ok(())
    }

    #[tracing::instrument(name = "access.commands.command", skip_all, fields(ddd.bounded_context.name = "access", input.request_id = tracing::field::Empty, input.command_id = tracing::field::Empty, input.arg0 = tracing::field::Empty, input.arg1 = tracing::field::Empty, input.arg2 = tracing::field::Empty, input.arg3 = tracing::field::Empty, input.args.count = tracing::field::Empty), err)]
    pub(super) fn command(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
        request: AccountBase,
    ) -> Result<()> {
        let Some(session) = self.sessions.get(&client) else {
            return Ok(());
        };
        let span = tracing::Span::current();
        match &request {
            AccountBase::DoCmdInt3 {
                request_id,
                cmd,
                arg0,
                arg1,
                arg2,
            } => {
                span.record("input.request_id", request_id);
                span.record("input.command_id", cmd);
                span.record("input.arg0", arg0);
                span.record("input.arg1", arg1);
                span.record("input.arg2", arg2);
            }
            AccountBase::DoCmdInt4 {
                request_id,
                cmd,
                arg0,
                arg1,
                arg2,
                arg3,
            } => {
                span.record("input.request_id", request_id);
                span.record("input.command_id", cmd);
                span.record("input.arg0", arg0);
                span.record("input.arg1", arg1);
                span.record("input.arg2", arg2);
                span.record("input.arg3", arg3);
            }
            AccountBase::DoCmdInt {
                request_id,
                cmd,
                arg,
            } => {
                span.record("input.request_id", request_id);
                span.record("input.command_id", cmd);
                span.record("input.arg0", arg);
            }
            AccountBase::DoCmdIntArr {
                request_id,
                cmd,
                args,
            } => {
                span.record("input.request_id", request_id);
                span.record("input.command_id", cmd);
                span.record("input.args.count", args.len());
            }
            _ => {}
        }
        let current_revision = session.account.snapshot.revision;
        let account_id = session.account.snapshot.id;
        let cache = session.account.cache.clone();
        if self.crew_command(client, entity, &request)? {
            return Ok(());
        }
        match request {
            request @ (AccountBase::ChatCommandFromClient { .. }
            | AccountBase::MessengerOnActionByClientChat2 { .. }) => {
                self.social_command(client, entity, request)?;
            }
            AccountBase::DoCmdInt3 {
                request_id,
                cmd: SYNC_DATA,
                arg0: revision,
                ..
            } => {
                let current = current_revision;
                if revision == current {
                    self.base.entity(entity)?.onCmdResponseExt(
                        request_id,
                        SUCCESS,
                        String::new(),
                        Pickle(crate::pickle!({"prevRev": current, "rev": current})),
                    )?;
                } else {
                    let data = cache;
                    self.stream(client, entity, request_id, &data)?;
                }
                if let Some(session) = self.sessions.get_mut(&client) {
                    session.garage_synced = true;
                }
            }
            AccountBase::DoCmdInt3 {
                request_id,
                cmd: SYNC_SHOP,
                arg1: len,
                arg2: crc,
                ..
            } => {
                let data = self.catalog.shop.clone();
                if len == data.len() as i64 && crc == i64::from(crc32fast::hash(&data) as i32) {
                    self.base
                        .entity(entity)?
                        .onCmdResponse(request_id, CACHE, String::new())?;
                } else {
                    self.stream(client, entity, request_id, &data)?;
                }
            }
            AccountBase::DoCmdInt3 {
                request_id,
                cmd: SYNC_DOSSIERS,
                ..
            } => {
                use std::io::Write;
                let mut encoder =
                    flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                encoder.write_all(&pickle::to_bytes(&crate::pickle!((1, []))))?;
                self.stream(client, entity, request_id, &encoder.finish()?)?;
            }
            AccountBase::DoCmdInt3 {
                cmd: SERVER_STATS, ..
            } => {
                let online = self.sessions.len() as u32;
                self.base.entity(entity)?.receiveServerStats(ServerStats {
                    clusterCCU: online,
                    regionCCU: online,
                })?;
            }
            AccountBase::RequestToken {
                request_id,
                token_type,
            } => {
                let data = if token_type == 1 {
                    let token = self.xmpp.issue(account_id, client.addr().ip())?;
                    Pickle(crate::pickle!({"databaseID": account_id, "token": (token.as_str())}))
                } else {
                    bootstrap::token_data(token_type, account_id)
                };
                self.base
                    .entity(entity)?
                    .onTokenReceived(request_id, token_type, data)?;
            }
            AccountBase::DoCmdInt3 {
                request_id,
                cmd: progression::BUY_VEHICLE_SLOT,
                arg0: shop_revision,
                arg1,
                arg2,
            } => {
                let command = (arg1 == 0 && arg2 == 0)
                    .then_some(crate::accounts::ProgressionCommand::BuyVehicleSlot);
                self.change_progression(client, entity, request_id, command, Some(shop_revision))?;
            }
            AccountBase::DoCmdInt3 {
                request_id,
                cmd: progression::RESEARCH,
                arg0,
                arg1,
                arg2,
            } => {
                let command = (arg2 == 0)
                    .then(|| {
                        Some(crate::accounts::ProgressionCommand::Research {
                            vehicle: i32::try_from(arg0).ok()?,
                            index: usize::try_from(arg1).ok()?,
                        })
                    })
                    .flatten();
                self.change_progression(client, entity, request_id, command, None)?;
            }
            AccountBase::DoCmdInt3 {
                request_id,
                cmd: progression::EQUIP,
                arg0,
                arg1,
                arg2,
            } => {
                let command = (arg0 > 0 && arg1 > 0 && arg2 >= 0)
                    .then(|| {
                        Some(crate::accounts::ProgressionCommand::Equip {
                            inventory_id: arg0,
                            item: i32::try_from(arg1).ok()?,
                            gun: i32::try_from(arg2).ok()?,
                            buy: false,
                        })
                    })
                    .flatten();
                self.change_progression(client, entity, request_id, command, None)?;
            }
            AccountBase::DoCmdInt4 {
                request_id,
                cmd: progression::BUY,
                arg0,
                arg1,
                arg2,
                arg3,
            } => {
                let command = (arg3 == 0)
                    .then(|| {
                        Some(crate::accounts::ProgressionCommand::Buy {
                            item: i32::try_from(arg1).ok()?,
                            count: u32::try_from(arg2).ok()?,
                        })
                    })
                    .flatten();
                self.change_progression(client, entity, request_id, command, Some(arg0))?;
            }
            AccountBase::DoCmdIntArr {
                request_id,
                cmd: progression::SELL_MULTIPLE,
                args,
            } => {
                self.change_progression(
                    client,
                    entity,
                    request_id,
                    progression::sell_multiple(&args),
                    args.first().copied().map(i64::from),
                )?;
            }
            AccountBase::DoCmdInt4 {
                request_id,
                cmd: progression::SELL,
                arg0,
                arg1,
                arg2,
                arg3,
            } => {
                let command = progression::sale_line(arg1, arg2, arg3)
                    .map(|line| crate::accounts::ProgressionCommand::Sell(vec![line]));
                self.change_progression(client, entity, request_id, command, Some(arg0))?;
            }
            AccountBase::DoCmdIntArr {
                request_id,
                cmd: cmd @ (progression::EQUIP_DEVICE | progression::EQUIP_DEVICE_SEQUENCE),
                args,
            } => {
                self.change_progression(
                    client,
                    entity,
                    request_id,
                    progression::equip_devices(cmd, &args),
                    args.first().map(|&revision| i64::from(revision)),
                )?;
            }
            AccountBase::DoCmdIntArr {
                request_id,
                cmd: progression::BUY_VEHICLE,
                args,
            } => {
                self.change_progression(
                    client,
                    entity,
                    request_id,
                    progression::buy_vehicle(&args),
                    args.first().map(|&r| i64::from(r)),
                )?;
            }
            AccountBase::DoCmdIntArr {
                request_id,
                cmd: progression::SELL_VEHICLE,
                args,
            } => {
                self.change_progression(
                    client,
                    entity,
                    request_id,
                    progression::sell_vehicle(&args),
                    args.first().copied().map(i64::from),
                )?;
            }
            AccountBase::DoCmdIntArr {
                request_id,
                cmd: progression::BUY_AND_EQUIP,
                args,
            } => {
                self.change_progression(
                    client,
                    entity,
                    request_id,
                    progression::equip(&args),
                    args.first().map(|&r| i64::from(r)),
                )?;
            }
            AccountBase::DoCmdIntArr {
                request_id,
                cmd: ammunition::SET_AND_FILL_LAYOUTS,
                args,
            } => {
                if expendables::has_layout(&args) {
                    self.change_progression(
                        client,
                        entity,
                        request_id,
                        expendables::install(&args),
                        args.first().copied().map(i64::from),
                    )?;
                } else {
                    self.change_ammunition(client, entity, request_id, &args)?;
                }
            }
            AccountBase::DoCmdInt4 {
                request_id,
                cmd: expendables::VEHICLE_SETTINGS,
                arg0: inventory_id,
                arg1: flag @ (expendables::AUTO_CONSUMABLES | expendables::AUTO_DIRECTIVES),
                arg2: enabled,
                ..
            } => {
                let command = expendables::auto_resupply(inventory_id, flag, enabled);
                self.change_progression(client, entity, request_id, command, None)?;
            }
            AccountBase::DoCmdIntArr {
                request_id,
                cmd: ADD_SETTINGS,
                args,
            } => {
                let Some(session) = self.sessions.get_mut(&client) else {
                    return Ok(());
                };
                if args.len() % 2 != 0 || session.saving {
                    self.base.entity(entity)?.onCmdResponse(
                        request_id,
                        FAILURE,
                        "Settings request invalid or busy".into(),
                    )?;
                    return Ok(());
                }
                session.saving = true;
                let changes: BTreeMap<_, _> = args
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| (pair[0], pair[1]))
                    .collect();
                let id = session.account.snapshot.id;
                self.pending_writes.insert(id);
                let accounts = self.accounts.clone();
                let sender = self.sender.clone();
                let catalog = self.catalog.clone();
                self.runtime
                    .spawn(tracing::Instrument::in_current_span(async move {
                        let result = async {
                            let snapshot = accounts.change_settings(id, changes.clone()).await?;
                            tokio::task::spawn_blocking(crate::observability::in_current_span(
                                move || catalog.prepare(snapshot),
                            ))
                            .await?
                        }
                        .await;
                        tokio::task::spawn_blocking(crate::observability::in_current_span(
                            move || {
                                let _ = sender.send(super::TracedCompletion::new(
                                    Completion::Settings {
                                        client,
                                        account_id: id,
                                        request_id,
                                        changes,
                                        result,
                                    },
                                ));
                            },
                        ))
                        .await
                        .ok();
                    }));
            }
            AccountBase::DoCmdInt2 {
                request_id,
                cmd: SET_ACTIVE_VEHICLE_SEASON,
                ..
            }
            | AccountBase::DoCmdStr {
                request_id,
                cmd: RESET_SESSION_STATS,
                ..
            } => {
                self.base
                    .entity(entity)?
                    .onCmdResponse(request_id, SUCCESS, String::new())?;
            }
            AccountBase::DoCmdInt3 { request_id, .. }
            | AccountBase::DoCmdInt4 { request_id, .. }
            | AccountBase::DoCmdInt2 { request_id, .. }
            | AccountBase::DoCmdInt { request_id, .. }
            | AccountBase::DoCmdStr { request_id, .. }
            | AccountBase::DoCmdIntArr { request_id, .. }
            | AccountBase::DoCmdIntArrStrArr { request_id, .. }
                if request_id != REQUEST_NO_RESPONSE =>
            {
                self.base.entity(entity)?.onCmdResponse(
                    request_id,
                    FAILURE,
                    "Command not implemented in garage milestone".into(),
                )?;
            }
            _ => {}
        }
        Ok(())
    }
}
