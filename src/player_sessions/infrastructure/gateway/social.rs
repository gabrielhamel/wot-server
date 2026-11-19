use super::*;
use crate::player_sessions::infrastructure::schema::{ChatActionData, MessengerArgs};
use crate::player_sessions::mercury::app::entity::{Bytes, EntityId};
use crate::social::contract::{ContactChange, Conversation};

mod command {
    pub const ADD_FRIEND: u8 = 3;
    pub const ADD_IGNORED: u8 = 4;
    pub const REMOVE_FRIEND: u8 = 5;
    pub const REMOVE_IGNORED: u8 = 6;
    pub const CREATE_PRIVATE: u8 = 7;
    pub const ROSTER: u8 = 9;
    pub const FRIEND_STATUS: u8 = 10;
    pub const SYSTEM_CHANNELS: u8 = 31;
    pub const MEMBERS: u8 = 36;
    pub const ENTER: u8 = 37;
    pub const LEAVE: u8 = 38;
    pub const BROADCAST: u8 = 39;
}

#[derive(Clone)]
pub(super) enum Request {
    Search {
        request: u16,
        online: bool,
        limit: usize,
        prefix: String,
    },
    Roster {
        request: i64,
    },
    Change {
        request: i64,
        action: u8,
        peer: i64,
        change: ContactChange,
    },
}
pub(super) enum Reply {
    Players(Vec<Player>),
    Contacts(Vec<Contact>),
}

fn action(request: i64, action: u8, channel: u32, data: Value) -> ChatActionData {
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    ChatActionData {
        requestID: request,
        action,
        actionResponse: 0,
        time,
        sentTime: time,
        channel: EntityId(channel),
        originator: -1,
        originatorNickName: String::new(),
        group: 0,
        data: Pickle(data),
        flags: 0,
    }
}

impl Gateway {
    #[tracing::instrument(name = "player_sessions.social.send_chat", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.request_id = data.requestID, input.action = data.action, output.response = data.actionResponse), err)]
    fn send_chat(&mut self, entity: EntityKey<Account>, data: ChatActionData) -> Result<()> {
        self.base.entity(entity)?.onChatAction(data)?;
        Ok(())
    }

    #[tracing::instrument(name = "player_sessions.social.social_error", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.request = request, input.event = event, input.channel = channel, input.response = response), err)]
    fn social_error(
        &mut self,
        entity: EntityKey<Account>,
        request: i64,
        event: u8,
        channel: u32,
        response: u8,
    ) -> Result<()> {
        let mut data = action(request, event, channel, Value::None);
        data.actionResponse = response;
        self.send_chat(entity, data)
    }

    fn online_player(&self, id: i64) -> Option<(EntityKey<Account>, Player)> {
        self.sessions.values().find_map(|s| {
            (s.account.snapshot.id() == id).then_some(s).and_then(|s| {
                s.entity.map(|entity| {
                    (
                        entity,
                        Player {
                            id,
                            name: s.account.snapshot.display_name().to_owned(),
                        },
                    )
                })
            })
        })
    }

    fn ignores(&self, owner: i64, peer: i64) -> bool {
        self.sessions
            .values()
            .find(|s| s.account.snapshot.id() == owner)
            .is_none_or(|s| {
                !s.social_ready
                    || s.social_pending
                    || s.contacts.iter().any(|c| c.player.id == peer && c.ignored)
            })
    }

    #[tracing::instrument(name = "player_sessions.social.social_command", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.client.address = %client, input.request_id = tracing::field::Empty, input.action = tracing::field::Empty, input.channel = tracing::field::Empty, input.text.bytes = tracing::field::Empty, input.prefix.bytes = tracing::field::Empty, output.outcome = tracing::field::Empty), err)]
    pub(super) fn social_command(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
        command: AccountBase,
    ) -> Result<()> {
        let Some(session) = self.sessions.get_mut(&client) else {
            return Ok(());
        };
        let span = tracing::Span::current();
        if !session.social_budget.admit(Instant::now()) {
            span.record("output.outcome", "rate_limited");
            return Ok(());
        }
        match command {
            AccountBase::MessengerOnActionByClientChat2 {
                action: command,
                request,
                args,
            } => {
                span.record("input.request_id", request);
                span.record("input.action", command);
                span.record("input.prefix.bytes", args.strArg1.0.len());
                if command != 2 {
                    span.record("output.outcome", "unsupported_action");
                    return self
                        .base
                        .entity(entity)?
                        .messenger_onActionByServer_chat2(
                            1,
                            request,
                            MessengerArgs {
                                int32Arg1: 7,
                                ..Default::default()
                            },
                        )
                        .map_err(Into::into);
                }
                let Ok(prefix) = String::from_utf8(args.strArg1.0) else {
                    span.record("output.outcome", "invalid_search_prefix");
                    return self.search_error(entity, request, 8);
                };
                self.social_job(
                    client,
                    entity,
                    Request::Search {
                        request,
                        online: args.int64Arg1 != 0,
                        limit: usize::try_from(args.int32Arg1).unwrap_or(0).min(50),
                        prefix,
                    },
                )?;
            }
            AccountBase::ChatCommandFromClient {
                request_id,
                action: command,
                channel,
                int_arg,
                short_arg: _,
                str_arg,
                str_arg2,
            } => {
                span.record("input.request_id", request_id);
                span.record("input.action", command);
                span.record("input.channel", channel.0);
                span.record("input.text.bytes", str_arg.len());
                let Some(session) = self.sessions.get(&client) else {
                    return Ok(());
                };
                let owner = session.account.snapshot.id();
                if str_arg.len() > 2048 || str_arg2.len() > 128 {
                    return self.social_error(
                        entity,
                        request_id,
                        event_for(command),
                        channel.0,
                        34,
                    );
                }
                match command {
                    command::ROSTER => self.social_job(
                        client,
                        entity,
                        Request::Roster {
                            request: request_id,
                        },
                    )?,
                    command::ADD_FRIEND
                    | command::ADD_IGNORED
                    | command::REMOVE_FRIEND
                    | command::REMOVE_IGNORED => {
                        let (event, change) = match command {
                            command::ADD_FRIEND => (15, ContactChange::Friend),
                            command::ADD_IGNORED => (14, ContactChange::Ignore),
                            command::REMOVE_FRIEND => (17, ContactChange::RemoveFriend),
                            _ => (16, ContactChange::RemoveIgnored),
                        };
                        self.social_job(
                            client,
                            entity,
                            Request::Change {
                                request: request_id,
                                action: event,
                                peer: int_arg,
                                change,
                            },
                        )?;
                    }
                    command::FRIEND_STATUS => self.send_presence(client, entity, int_arg)?,
                    command::CREATE_PRIVATE => {
                        if self.online_player(int_arg).is_none()
                            || self.ignores(owner, int_arg)
                            || self.ignores(int_arg, owner)
                        {
                            return self.social_error(entity, request_id, 18, 0, 25);
                        }
                        let already_open = self
                            .conversations
                            .between(owner, int_arg)
                            .is_some_and(|room| room.is_subscribed(owner));
                        let Some(room) = self.conversations.open(owner, int_arg) else {
                            return self.social_error(entity, request_id, 18, 0, 14);
                        };
                        if already_open {
                            self.send_chat(entity, action(-1, 4, room.id, Value::None))?;
                        }
                        self.announce_room(&room, owner, owner, request_id)?;
                    }
                    command::MEMBERS => {
                        let Some(room) = self.conversations.joined(channel.0, owner).cloned()
                        else {
                            return self.social_error(entity, request_id, 3, channel.0, 9);
                        };
                        let members = room.participants.iter().filter_map(|id| self.online_player(*id)).map(|(_, p)| crate::pickle!({"id": (p.id), "nickName": (p.name.as_str()), "status": 0})).collect();
                        self.send_chat(
                            entity,
                            action(request_id, 3, channel.0, Value::List(members)),
                        )?;
                    }
                    command::ENTER => {
                        let already_open = self.conversations.joined(channel.0, owner).is_some();
                        let Some(room) = self.conversations.subscribe(channel.0, owner) else {
                            return self.social_error(entity, request_id, 7, channel.0, 9);
                        };
                        if !already_open {
                            self.announce_room(&room, owner, owner, request_id)?;
                        }
                    }
                    command::LEAVE => {
                        if self.conversations.close(channel.0, owner).is_some() {
                            self.send_chat(entity, action(request_id, 8, channel.0, Value::None))?;
                        }
                    }
                    command::BROADCAST => {
                        let Some(room) = self.conversations.joined(channel.0, owner).cloned()
                        else {
                            return self.social_error(entity, request_id, 1, channel.0, 9);
                        };
                        let Some(session) = self.sessions.get_mut(&client) else {
                            return Ok(());
                        };
                        if Instant::now() < session.message_next {
                            return self.social_error(entity, request_id, 1, channel.0, 26);
                        }
                        session.message_next = Instant::now() + Duration::from_millis(500);
                        if str_arg.trim().is_empty()
                            || str_arg.chars().any(|c| c.is_control() && c != '\n')
                        {
                            return self.social_error(entity, request_id, 1, channel.0, 34);
                        }
                        if room.participants.iter().any(|id| self.ignores(*id, owner)) {
                            return self.social_error(entity, request_id, 1, channel.0, 19);
                        }
                        let name = self
                            .online_player(owner)
                            .map(|(_, p)| p.name)
                            .unwrap_or_default();
                        let mut message =
                            action(request_id, 1, channel.0, Value::Bytes(str_arg.into_bytes()));
                        message.originator = owner;
                        message.originatorNickName = name;
                        for id in room.participants {
                            let result = self.deliver_private_message(&room, id, owner, &message);
                            if let Err(error) = result {
                                tracing::error!(%error, "Private message delivery failed");
                            }
                        }
                    }
                    0 | 22 | 23 => {}
                    command::SYSTEM_CHANNELS => {
                        self.send_chat(entity, action(-1, 6, 0, Value::List(Vec::new())))?
                    }
                    _ => {
                        self.social_error(entity, request_id, event_for(command), channel.0, 32)?
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn search_error(&mut self, entity: EntityKey<Account>, request: u16, code: i32) -> Result<()> {
        self.base.entity(entity)?.messenger_onActionByServer_chat2(
            1,
            request,
            MessengerArgs {
                int32Arg1: code,
                ..Default::default()
            },
        )?;
        Ok(())
    }

    #[tracing::instrument(name = "player_sessions.social.social_job", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.client.address = %client, output.outcome = tracing::field::Empty), err)]
    fn social_job(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
        request: Request,
    ) -> Result<()> {
        let Some(session) = self.sessions.get_mut(&client) else {
            return Ok(());
        };
        let permit = self.workers.clone().try_acquire_owned();
        if session.social_pending || Instant::now() < session.social_next || permit.is_err() {
            tracing::Span::current().record("output.outcome", "busy");
            return match request {
                Request::Search { request, .. } => self.search_error(entity, request, 6),
                Request::Roster { request } => self.social_error(entity, request, 13, 0, 26),
                Request::Change {
                    request, action, ..
                } => self.social_error(entity, request, action, 0, 26),
            };
        }
        tracing::Span::current().record("output.outcome", "started");
        session.social_pending = true;
        session.social_next = Instant::now() + Duration::from_millis(100);
        let owner = session.account.snapshot.id();
        let online_ids: Vec<_> = self
            .sessions
            .values()
            .filter(|s| s.entity.is_some())
            .map(|s| s.account.snapshot.id())
            .collect();
        let social = self.social.clone();
        let sender = self.sender.clone();
        self.runtime
            .spawn(tracing::Instrument::in_current_span(async move {
                let _permit = permit;
                let result = tokio::time::timeout(Duration::from_secs(10), async {
                    match &request {
                        Request::Search { prefix, online, .. } => social
                            .search(prefix, online.then_some(online_ids.as_slice()))
                            .await
                            .map(Reply::Players),
                        Request::Roster { .. } => social.roster(owner).await.map(Reply::Contacts),
                        Request::Change { peer, change, .. } => social
                            .change(owner, *peer, *change)
                            .await
                            .map(Reply::Contacts),
                    }
                })
                .await
                .unwrap_or_else(|_| Err(anyhow::anyhow!("Social operation timed out")));
                if sender
                    .try_send(super::TracedCompletion::new(Completion::Social {
                        client,
                        entity,
                        request,
                        result,
                    }))
                    .is_err()
                {
                    tracing::Span::current().record("output.outcome", "completion_unavailable");
                    tracing::error!("Social completion queue unavailable");
                }
            }));
        Ok(())
    }

    #[tracing::instrument(name = "player_sessions.social.social_complete", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.client.address = %client, output.success = result.is_ok()), err)]
    pub(super) fn social_complete(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
        request: Request,
        result: Result<Reply>,
    ) -> Result<()> {
        if let Err(error) = &result {
            tracing::error!(%error, "Social operation failed");
        }
        let Some(session) = self
            .sessions
            .get_mut(&client)
            .filter(|s| s.entity == Some(entity))
        else {
            return Ok(());
        };
        session.social_pending = false;
        match (request, result) {
            (
                Request::Search {
                    request,
                    online,
                    limit,
                    ..
                },
                Ok(Reply::Players(players)),
            ) => {
                let data = Value::List(
                    players
                        .into_iter()
                        .filter(|p| !online || self.online_player(p.id).is_some())
                        .take(limit)
                        .map(|p| crate::pickle!(((p.name.as_str()), (p.id), "")))
                        .collect(),
                );
                self.base.entity(entity)?.messenger_onActionByServer_chat2(
                    0,
                    request,
                    MessengerArgs {
                        strArg1: Bytes(pickle::to_bytes(&data)),
                        ..Default::default()
                    },
                )?;
            }
            (Request::Roster { request }, Ok(Reply::Contacts(contacts))) => {
                let data = roster_data(&contacts);
                session.social_ready = true;
                session.contacts = contacts;
                self.send_chat(entity, action(request, 13, 0, data))?;
                self.send_presence(client, entity, -1)?;
            }
            (
                Request::Change {
                    request,
                    action: event,
                    peer,
                    ..
                },
                Ok(Reply::Contacts(contacts)),
            ) => {
                let data = if event == 15 || event == 14 {
                    contacts
                        .iter()
                        .find(|c| c.player.id == peer)
                        .map(|c| {
                            crate::pickle!((
                                (c.player.id),
                                (c.player.name.as_str()),
                                (if c.ignored { 2 } else { 1 })
                            ))
                        })
                        .unwrap_or(Value::None)
                } else {
                    Value::I64(peer)
                };
                session.social_ready = true;
                session.contacts = contacts;
                self.send_chat(entity, action(request, event, 0, data))?;
                self.send_presence(client, entity, peer)?;
            }
            (request, _) => match request {
                Request::Search { request, .. } => self.search_error(entity, request, 8)?,
                Request::Roster { request } => self.social_error(entity, request, 13, 0, 1)?,
                Request::Change {
                    request, action, ..
                } => self.social_error(entity, request, action, 0, 9)?,
            },
        }
        Ok(())
    }

    fn send_presence(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
        peer: i64,
    ) -> Result<()> {
        let contacts = self
            .sessions
            .get(&client)
            .map(|s| s.contacts.clone())
            .unwrap_or_default();
        for contact in contacts
            .iter()
            .filter(|c| !c.ignored && (peer == -1 || peer == c.player.id))
        {
            self.send_chat(entity, action(-1, 19, 0, crate::pickle!({(contact.player.id): (self.online_player(contact.player.id).is_some())})))?;
        }
        Ok(())
    }

    pub(super) fn notify_presence(&mut self, id: i64, online: bool) {
        let targets: Vec<_> = self
            .sessions
            .values()
            .filter(|s| s.contacts.iter().any(|c| c.player.id == id && !c.ignored))
            .filter_map(|s| s.entity)
            .collect();
        for target in targets {
            let _ = self.send_chat(target, action(-1, 19, 0, crate::pickle!({(id): online})));
        }
    }

    fn deliver_private_message(
        &mut self,
        room: &Conversation,
        recipient: i64,
        sender: i64,
        message: &ChatActionData,
    ) -> Result<()> {
        let Some((target, _)) = self.online_player(recipient) else {
            return Ok(());
        };
        if !room.is_subscribed(recipient) {
            self.announce_room(room, recipient, sender, -1)?;
            self.conversations.subscribe(room.id, recipient);
        }
        self.send_chat(target, message.clone())
    }

    fn announce_room(
        &mut self,
        room: &Conversation,
        recipient: i64,
        opener: i64,
        request: i64,
    ) -> Result<()> {
        let Some((target, _)) = self.online_player(recipient) else {
            return Ok(());
        };
        let other = room
            .participants
            .iter()
            .find(|peer| **peer != recipient)
            .and_then(|peer| self.online_player(*peer));
        let Some((_, other)) = other else {
            return Ok(());
        };
        let owner_name = self
            .online_player(opener)
            .map(|(_, p)| p.name)
            .unwrap_or_default();
        let data = crate::pickle!([{"id": (room.id), "channelName": (other.name.as_str()), "owner": opener, "ownerName": (owner_name.as_str()), "flags": 8, "notifyFlags": 0}]);
        self.send_chat(target, action(-1, 6, 0, data))?;
        self.send_chat(target, action(request, 7, room.id, Value::None))?;
        Ok(())
    }

    fn destroy_room(&mut self, room: &Conversation) {
        for id in room.participants {
            if room.is_subscribed(id)
                && let Some((target, _)) = self.online_player(id)
            {
                let _ = self.send_chat(target, action(-1, 4, room.id, Value::None));
            }
        }
    }

    pub(super) fn social_disconnect(&mut self, client: ClientKey) {
        if let Some(session) = self.sessions.get(&client) {
            let id = session.account.snapshot.id();
            self.notify_presence(id, false);
            for room in self.conversations.disconnect(id) {
                self.destroy_room(&room);
            }
        }
    }
}

fn roster_data(contacts: &[Contact]) -> Value {
    Value::List(
        contacts
            .iter()
            .map(|c| {
                crate::pickle!((
                    (c.player.id),
                    (c.player.name.as_str()),
                    (if c.ignored { 2 } else { 1 })
                ))
            })
            .collect(),
    )
}
fn event_for(command: u8) -> u8 {
    match command {
        command::ADD_FRIEND => 15,
        command::ADD_IGNORED => 14,
        command::REMOVE_FRIEND => 17,
        command::REMOVE_IGNORED => 16,
        command::CREATE_PRIVATE => 18,
        command::ROSTER => 13,
        command::FRIEND_STATUS => 19,
        command::MEMBERS => 3,
        command::ENTER => 7,
        command::LEAVE => 8,
        command::BROADCAST => 1,
        _ => 20,
    }
}
