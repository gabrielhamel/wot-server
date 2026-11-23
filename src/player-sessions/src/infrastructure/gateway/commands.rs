use super::*;
use wot_platform::mercury::app::entity::MethodDecodeError;

const REQUEST_PREBATTLES: i16 = 520;
const PREBATTLES_STREAM: i16 = 200;

impl Gateway {
    pub(super) fn handle_base_method(&mut self, event: base::BaseMethodEvent) {
        let session = self.sessions.get(&event.client);
        if let Some(session) = session {
            event
                .span
                .record("user.id", session.account.snapshot.id().to_string());
        }
        if let Some(participant) = session.map(|session| ParticipantId(session.generation))
            && self.battle_participants.contains(&participant)
        {
            event.span.in_scope(|| {
                let entity_id = event.entity.id().0;
                let adapter = MercuryArena::new(&mut self.base, &self.battle_clients);
                if let Some(command) = adapter.avatar_command(&event) {
                    let result = self
                        .battle_worker
                        .avatar_command(participant, entity_id, command);
                    self.handle_battle_submission(event.client, result);
                }
            });
            return;
        }
        let span = event.span;
        let owns_entity = session
            .and_then(|session| session.entity)
            .is_some_and(|entity| entity.any() == event.entity);
        span.in_scope(|| {
            if !owns_entity {
                span.record("output.outcome", "entity_unavailable");
                return;
            }
            let (entity, request) = match self.base.decode::<AccountBase>(event.entity, &event.call)
            {
                Ok(command) => command,
                Err(error) => {
                    let outcome = match error {
                        MethodDecodeError::UnknownMethod => "unknown_method",
                        MethodDecodeError::Unimplemented => "unimplemented",
                        MethodDecodeError::Invalid => "decode_failed",
                    };
                    span.record("output.outcome", outcome);
                    span.record("otel.status_code", "ERROR");
                    if matches!(error, MethodDecodeError::Unimplemented) {
                        tracing::error!("Account base method is not implemented");
                    } else {
                        tracing::error!("Cannot decode account base method");
                    }
                    return;
                }
            };
            if let Some((request_id, cmd)) = command_ids(&request) {
                span.record("input.request_id", request_id);
                span.record("input.command_id", cmd);
            } else {
                match &request {
                    AccountBase::RequestToken { request_id, .. }
                    | AccountBase::MessengerOnActionByClientChat2 {
                        request: request_id,
                        ..
                    } => {
                        span.record("input.request_id", request_id);
                    }
                    AccountBase::ChatCommandFromClient { request_id, .. } => {
                        span.record("input.request_id", request_id);
                    }
                    _ => {}
                }
            }
            if let Err(error) = self.command(event.client, entity, request) {
                span.record("output.outcome", "dispatch_failed");
                span.record("otel.status_code", "ERROR");
                error!(%error, "Account command failed");
            } else {
                span.record("output.outcome", "dispatched");
            }
        });
    }

    #[tracing::instrument(name = "mercury.response.resource", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.request_id = request_id, output.bytes = data.len(), output.delivery = tracing::field::Empty), err)]
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
        let description = pickle::to_bytes(&wot_platform::pickle!((
            (data.len() as i64),
            (crc32fast::hash(data) as i32)
        )));
        self.base
            .push_resource(client, request_id as u16, description, data)?;
        tracing::Span::current().record("output.delivery", "sent");
        Ok(())
    }

    #[tracing::instrument(name = "player_sessions.commands.command", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.request_id = tracing::field::Empty, input.command_id = tracing::field::Empty, input.arg0 = tracing::field::Empty, input.arg1 = tracing::field::Empty, input.arg2 = tracing::field::Empty, input.arg3 = tracing::field::Empty, input.args.count = tracing::field::Empty, input.outfits.count = tracing::field::Empty, input.string.bytes = tracing::field::Empty, output.outcome = tracing::field::Empty, otel.status_code = tracing::field::Empty), err)]
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
        if let Some((request_id, cmd)) = command_ids(&request) {
            span.record("input.request_id", request_id);
            span.record("input.command_id", cmd);
        }
        match &request {
            AccountBase::DoCmdInt3 {
                arg0, arg1, arg2, ..
            } => {
                span.record("input.arg0", arg0);
                span.record("input.arg1", arg1);
                span.record("input.arg2", arg2);
            }
            AccountBase::DoCmdInt4 {
                arg0,
                arg1,
                arg2,
                arg3,
                ..
            } => {
                span.record("input.arg0", arg0);
                span.record("input.arg1", arg1);
                span.record("input.arg2", arg2);
                span.record("input.arg3", arg3);
            }
            AccountBase::DoCmdInt2 { arg0, arg1, .. } => {
                span.record("input.arg0", arg0);
                span.record("input.arg1", arg1);
            }
            AccountBase::DoCmdStr { arg, .. } => {
                span.record("input.string.bytes", arg.len());
            }
            AccountBase::DoCmdInt { arg, .. } => {
                span.record("input.arg0", arg);
            }
            AccountBase::DoCmdIntArr { args, .. } => {
                span.record("input.args.count", args.len());
            }
            AccountBase::DoCmdIntArrStrArr { ints, strings, .. } => {
                span.record("input.args.count", ints.len());
                span.record("input.outfits.count", strings.len());
            }
            _ => {}
        }
        let current_revision = session.account.snapshot.revision();
        let account_id = session.account.snapshot.id();
        let cache = session.account.cache.clone();
        if self.crew_command(client, entity, &request)? {
            return Ok(());
        }
        match request {
            AccountBase::AccountPrebattleCreateTraining {
                arena_type_id,
                round_length,
                is_opened,
                comment,
            } => {
                self.create_training_room(
                    client,
                    entity,
                    arena_type_id,
                    round_length,
                    is_opened,
                    &comment.0,
                )?;
            }
            AccountBase::AccountPrebattleSendPrebattleInvites {
                recipients,
                comment,
            } => {
                self.send_training_invites(client, &recipients, &comment.0);
            }
            AccountBase::DoCmdInt3 {
                cmd: command @ (training::JOIN | training::ACCEPT_INVITE),
                arg0,
                arg1,
                arg2,
                ..
            } => self.join_training_room(
                client,
                entity,
                [arg0, arg1, arg2],
                command == training::ACCEPT_INVITE,
            )?,
            AccountBase::DoCmdInt3 {
                cmd: training::DECLINE_INVITE,
                arg0,
                arg1,
                arg2,
                ..
            } => self.decline_training_invite(client, entity, [arg0, arg1, arg2]),
            AccountBase::DoCmdInt3 {
                request_id,
                cmd: training::LEAVE,
                ..
            } => {
                self.leave_training_room(client, entity, request_id)?;
            }
            AccountBase::DoCmdInt3 {
                request_id,
                cmd: training::KICK,
                arg0,
                arg1,
                arg2,
            } => self.kick_training_player(client, entity, request_id, [arg0, arg1, arg2])?,
            AccountBase::DoCmdInt3 {
                request_id,
                cmd: command @ (training::TEAM_READY | training::TEAM_NOT_READY),
                arg0,
                arg1,
                arg2,
            } => self.change_training_team_ready(
                client,
                entity,
                request_id,
                [arg0, arg1, arg2],
                command == training::TEAM_READY,
            )?,
            AccountBase::DoCmdInt3 {
                request_id,
                cmd: training::READY,
                arg0: vehicle_id,
                ..
            } => {
                self.set_training_ready(client, entity, request_id, Some(vehicle_id))?;
            }
            AccountBase::DoCmdInt3 {
                request_id,
                cmd: training::NOT_READY,
                ..
            } => {
                self.set_training_ready(client, entity, request_id, None)?;
            }
            AccountBase::DoCmdInt3 {
                request_id,
                cmd: training::ASSIGN,
                arg0: player_entity_id,
                arg1: roster_slot,
                arg2: group,
            } => {
                self.assign_training_roster(
                    client,
                    entity,
                    request_id,
                    player_entity_id,
                    roster_slot,
                    group,
                )?;
            }
            AccountBase::DoCmdIntArr {
                request_id,
                cmd: cosmetics::BUY_CUSTOMIZATIONS,
                args,
            } => {
                self.buy_cosmetics(client, entity, request_id, &args)?;
            }
            AccountBase::DoCmdInt3 {
                request_id,
                cmd: training::SWAP_TEAMS,
                arg0,
                arg1,
                arg2,
            } => {
                self.swap_training_teams(client, entity, request_id, [arg0, arg1, arg2])?;
            }
            AccountBase::DoCmdInt3 {
                request_id,
                cmd:
                    command @ (training::CHANGE_ARENA
                    | training::CHANGE_DURATION
                    | training::CHANGE_OPEN_STATUS
                    | training::CHANGE_VOICE_CHAT),
                arg0,
                arg1,
                arg2,
            } => self.change_training_setting(
                client,
                entity,
                request_id,
                command,
                training::SettingArgument::Numeric([arg0, arg1, arg2]),
            )?,
            AccountBase::DoCmdStr {
                request_id,
                cmd: training::CHANGE_COMMENT,
                arg,
            } => self.change_training_setting(
                client,
                entity,
                request_id,
                training::CHANGE_COMMENT,
                training::SettingArgument::Comment(&arg),
            )?,
            AccountBase::DoCmdIntArrStrArr {
                request_id,
                cmd: cosmetics::APPLY_OUTFIT,
                ints,
                strings,
            } => {
                self.apply_outfit(client, entity, request_id, &ints, &strings)?;
            }
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
                self.sync_garage(
                    client,
                    entity,
                    request_id,
                    revision,
                    current_revision,
                    &cache,
                )?;
            }
            AccountBase::DoCmdInt3 {
                request_id,
                cmd: SYNC_SHOP,
                arg1: len,
                arg2: crc,
                ..
            } => self.sync_shop(entity, client, request_id, len, crc)?,
            AccountBase::DoCmdInt3 {
                request_id,
                cmd: SYNC_DOSSIERS,
                ..
            } => self.sync_dossiers(client, entity, request_id)?,
            AccountBase::DoCmdInt3 {
                request_id,
                cmd: SERVER_STATS,
                ..
            } => self.server_stats(entity, request_id)?,
            AccountBase::DoCmdIntArr {
                request_id,
                cmd: REQUEST_PREBATTLES,
                args,
            } => {
                if request_id == REQUEST_NO_RESPONSE {
                    span.record("output.outcome", "ignored");
                } else {
                    self.list_training_rooms(client, entity, request_id, &args)?;
                }
            }
            AccountBase::RequestToken {
                request_id,
                token_type,
            } => self.request_token(client, entity, account_id, request_id, token_type)?,
            AccountBase::DoCmdInt3 {
                request_id,
                cmd: progression::BUY_PREMIUM,
                arg0: shop_revision,
                arg1: days,
                arg2: arena_id,
            } => {
                let command = i32::try_from(days)
                    .ok()
                    .filter(|_| arena_id == 0)
                    .map(|days| wot_accounts::contract::ProgressionCommand::BuyPremium { days });
                self.change_progression(client, entity, request_id, command, Some(shop_revision))?;
            }
            AccountBase::DoCmdInt3 {
                request_id,
                cmd: progression::BUY_VEHICLE_SLOT,
                arg0: shop_revision,
                arg1,
                arg2,
            } => {
                let command = (arg1 == 0 && arg2 == 0)
                    .then_some(wot_accounts::contract::ProgressionCommand::BuyVehicleSlot);
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
                        Some(wot_accounts::contract::ProgressionCommand::Research {
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
                        Some(wot_accounts::contract::ProgressionCommand::Equip {
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
                        Some(wot_accounts::contract::ProgressionCommand::Buy {
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
                    .map(|line| wot_accounts::contract::ProgressionCommand::Sell(vec![line]));
                self.change_progression(client, entity, request_id, command, Some(arg0))?;
            }
            AccountBase::DoCmdIntArr {
                request_id,
                cmd:
                    cmd @ (progression::EQUIP_DEVICE
                    | progression::EQUIP_DEVICE_SEQUENCE
                    | progression::DISASSEMBLE_DEVICE),
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
                cmd: progression::UPGRADE_DEVICE,
                args,
            } => self.change_progression(
                client,
                entity,
                request_id,
                progression::upgrade_device(&args),
                args.first().copied().map(i64::from),
            )?,
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
                self.change_settings(client, entity, request_id, &args)?;
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
                span.record("output.outcome", "acknowledged_without_change");
                self.base
                    .entity(entity)?
                    .onCmdResponse(request_id, SUCCESS, String::new())?;
            }
            request => {
                if let Some((request_id, command_id)) = command_ids(&request) {
                    span.record("output.outcome", "unimplemented");
                    span.record("otel.status_code", "ERROR");
                    error!(request_id, command_id, "Account command is not implemented");
                    if request_id != REQUEST_NO_RESPONSE {
                        self.base.entity(entity)?.onCmdResponse(
                            request_id,
                            FAILURE,
                            "Command not implemented in garage milestone".into(),
                        )?;
                    }
                } else {
                    span.record("output.outcome", "ignored");
                }
            }
        }
        Ok(())
    }

    #[tracing::instrument(name = "player_sessions.sync_garage", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.request_id = request_id, input.revision = revision, output.revision = current, output.outcome = if revision == current { "up_to_date" } else { "stream" }), err)]
    fn sync_garage(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
        request_id: i16,
        revision: i64,
        current: i64,
        cache: &[u8],
    ) -> Result<()> {
        if revision == current {
            self.base.entity(entity)?.onCmdResponseExt(
                request_id,
                SUCCESS,
                String::new(),
                Pickle(
                    self.catalog.garage_sync(
                        current,
                        self.sessions
                            .get(&client)
                            .and_then(|session| session.account.snapshot.active_premium_until()),
                    ),
                ),
            )?;
        } else {
            self.stream(client, entity, request_id, cache)?;
        }
        if let Some(session) = self.sessions.get_mut(&client) {
            let first_sync = !session.garage_synced;
            session.garage_synced = true;
            session.reported_premium_expiry = None;
            if first_sync {
                self.restore_training_room(client, entity)?;
                self.deliver_pending_training_invites(client, entity);
            }
        }
        Ok(())
    }

    #[tracing::instrument(name = "player_sessions.sync_shop", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.request_id = request_id, input.cache.bytes = len, input.cache.crc = crc, output.outcome = tracing::field::Empty), err)]
    fn sync_shop(
        &mut self,
        entity: EntityKey<Account>,
        client: ClientKey,
        request_id: i16,
        len: i64,
        crc: i64,
    ) -> Result<()> {
        let data = self.catalog.shop_data();
        let cache_hit = len == data.len() as i64 && crc == i64::from(crc32fast::hash(&data) as i32);
        tracing::Span::current().record(
            "output.outcome",
            if cache_hit { "cache_hit" } else { "stream" },
        );
        if cache_hit {
            self.base
                .entity(entity)?
                .onCmdResponse(request_id, CACHE, String::new())?;
        } else {
            self.stream(client, entity, request_id, &data)?;
        }
        Ok(())
    }

    #[tracing::instrument(name = "player_sessions.sync_dossiers", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.request_id = request_id), err)]
    fn sync_dossiers(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
        request_id: i16,
    ) -> Result<()> {
        self.stream_pickle(client, entity, request_id, &wot_platform::pickle!((1, [])))
    }

    #[tracing::instrument(name = "player_sessions.list_training_rooms", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.request_id = request_id, input.args.count = args.len(), output.rooms.count = tracing::field::Empty, output.rooms.total = tracing::field::Empty, output.players.count = tracing::field::Empty, output.outcome = tracing::field::Empty), err)]
    fn list_training_rooms(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
        request_id: i16,
        args: &[i32],
    ) -> Result<()> {
        let span = tracing::Span::current();
        let page = match args {
            [training::TRAINING, _, 0 | 1, start, end]
                if request_id == PREBATTLES_STREAM && *start >= 0 && end >= start =>
            {
                Some((*start as usize, (*end - *start) as usize))
            }
            _ => None,
        };
        let Some((start, length)) = page else {
            span.record("output.outcome", "invalid_request");
            self.base.entity(entity)?.onCmdResponse(
                request_id,
                FAILURE,
                "Invalid or unsupported Training Rooms list request".into(),
            )?;
            return Ok(());
        };

        let rooms = self
            .training_rooms
            .values()
            .rev()
            .skip(start)
            .take(length.min(training::LIST_PAGE_LIMIT));
        let mut players = 0;
        let entries: Vec<_> = rooms
            .map(|room| {
                players += room.player_count();
                room.list_entry()
            })
            .collect();
        let count = entries.len();
        let total = self.training_rooms.len();
        self.stream_pickle(
            client,
            entity,
            request_id,
            &wot_platform::pickle!(((training::TRAINING), (total as i64), (Value::List(entries)))),
        )?;
        span.record("output.rooms.count", count);
        span.record("output.rooms.total", total);
        span.record("output.players.count", players);
        span.record(
            "output.outcome",
            if count == 0 { "empty" } else { "listed" },
        );
        Ok(())
    }

    fn stream_pickle(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
        request_id: i16,
        value: &pickle::Value,
    ) -> Result<()> {
        use std::io::Write;
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&pickle::to_bytes(value))?;
        self.stream(client, entity, request_id, &encoder.finish()?)
    }

    #[tracing::instrument(name = "player_sessions.server_stats", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.request_id = request_id, output.players.online = tracing::field::Empty), err)]
    fn server_stats(&mut self, entity: EntityKey<Account>, request_id: i16) -> Result<()> {
        let online = self.sessions.len() as u32;
        tracing::Span::current().record("output.players.online", online);
        self.base.entity(entity)?.receiveServerStats(ServerStats {
            clusterCCU: online,
            regionCCU: online,
        })?;
        Ok(())
    }

    #[tracing::instrument(name = "player_sessions.request_token", skip_all, fields(ddd.bounded_context.name = "player_sessions", user.id = account_id, input.request_id = request_id, input.token_type = token_type), err)]
    fn request_token(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
        account_id: i64,
        request_id: u16,
        token_type: u8,
    ) -> Result<()> {
        let data = if token_type == 1 {
            let token = self.xmpp.issue(account_id, client.addr().ip())?;
            Pickle(wot_platform::pickle!({"databaseID": account_id, "token": (token.as_str())}))
        } else {
            bootstrap::token_data(token_type, account_id)
        };
        self.base
            .entity(entity)?
            .onTokenReceived(request_id, token_type, data)?;
        Ok(())
    }
}

fn command_ids(request: &AccountBase) -> Option<(i16, i16)> {
    match *request {
        AccountBase::DoCmdInt {
            request_id, cmd, ..
        }
        | AccountBase::DoCmdInt2 {
            request_id, cmd, ..
        }
        | AccountBase::DoCmdInt3 {
            request_id, cmd, ..
        }
        | AccountBase::DoCmdInt4 {
            request_id, cmd, ..
        }
        | AccountBase::DoCmdIntArr {
            request_id, cmd, ..
        }
        | AccountBase::DoCmdIntArrStrArr {
            request_id, cmd, ..
        }
        | AccountBase::DoCmdStr {
            request_id, cmd, ..
        } => Some((request_id, cmd)),
        _ => None,
    }
}
