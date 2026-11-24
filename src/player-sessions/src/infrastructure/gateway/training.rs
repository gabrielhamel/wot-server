use super::*;
use crate::domain::training::{TrainingRoomSettings, VoiceChatMode};
use std::time::{SystemTime, UNIX_EPOCH};
use wot_platform::mercury::app::entity::EntityId;

mod start;
pub(super) use start::{PendingTrainingBattle, TEAM_NOT_READY, TEAM_READY};
mod chat;
mod invites;
pub(super) use chat::CHAT_BROADCAST;
pub(super) use invites::{ACCEPT_INVITE, DECLINE_INVITE};

pub(super) const JOIN: i16 = 400;
pub(super) const LEAVE: i16 = 401;
pub(super) const READY: i16 = 402;
pub(super) const NOT_READY: i16 = 403;
pub(super) const ASSIGN: i16 = 404;
pub(super) const SWAP_TEAMS: i16 = 405;
pub(super) const CHANGE_ARENA: i16 = 406;
pub(super) const CHANGE_DURATION: i16 = 407;
pub(super) const CHANGE_OPEN_STATUS: i16 = 408;
pub(super) const CHANGE_COMMENT: i16 = 409;
pub(super) const CHANGE_VOICE_CHAT: i16 = 410;
pub(super) const KICK: i16 = 414;

pub(super) const TRAINING: i32 = 2;
pub(super) const LIST_PAGE_LIMIT: usize = 50;

const TRAINING_BONUS_TYPE: i32 = 2;
const MAX_TEAM_PLAYERS: usize = 15;
const MAX_TEAM_OBSERVERS: usize = 5;
const MAX_ASSIGNED_PLAYERS: usize = MAX_TEAM_PLAYERS + MAX_TEAM_OBSERVERS;
const MAX_ROOM_PLAYERS: usize = 2 * MAX_TEAM_PLAYERS;
const LOCAL_CLUSTER: i64 = 0;
const NOT_FOUND: u8 = 2;
const ACCOUNT_LOCK: u8 = 3;
const ROOM_FULL: u8 = 5;
const PRIVACY: u8 = 10;
const WRONG_ARGS: u8 = 6;
const EVENT_DISABLED: u8 = 14;
const PLAYER_KICK: u8 = 10;
const ROSTER: u8 = 1;
const PLAYER_ADDED: u8 = 2;
const PLAYER_REMOVED: u8 = 3;
const PLAYER_STATE: u8 = 4;
const SETTINGS: u8 = 7;
const SETTING: u8 = 8;
const PROPERTIES: u8 = 10;
const HOST_ROLE: i32 = 0xfe7f;
const GUEST_ROLE: i32 = 0x60;
const PLAYER_NOT_READY: i32 = 1;
const PLAYER_READY: i32 = 4;
const PLAYER_IN_BATTLE: i32 = 8;
const OBSERVER_INVENTORY_ID: i64 = -5000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum RosterSlot {
    TeamOne = 1,
    TeamTwo = 2,
    UnassignedFromTeamOne = 17,
    UnassignedFromTeamTwo = 18,
}

impl RosterSlot {
    fn swap_team(self) -> Self {
        match self {
            Self::TeamOne => Self::TeamTwo,
            Self::TeamTwo => Self::TeamOne,
            unassigned => unassigned,
        }
    }

    fn capacity(self) -> usize {
        match self {
            Self::TeamOne | Self::TeamTwo => MAX_ASSIGNED_PLAYERS,
            Self::UnassignedFromTeamOne | Self::UnassignedFromTeamTwo => MAX_ROOM_PLAYERS,
        }
    }

    fn from_wire(value: i64) -> Option<Self> {
        match value {
            1 => Some(Self::TeamOne),
            2 => Some(Self::TeamTwo),
            17 => Some(Self::UnassignedFromTeamOne),
            18 => Some(Self::UnassignedFromTeamTwo),
            _ => None,
        }
    }
}

pub(super) struct HostedRoom {
    id: u32,
    settings: TrainingRoomSettings,
    created_at: f64,
    host: ClientKey,
    members: HashMap<ClientKey, RoomPlayer>,
    invites: HashMap<i64, invites::Invitation>,
    teams_ready: (bool, bool),
}

struct RoomPlayer {
    entity: EntityKey<Account>,
    account_id: i64,
    name: String,
    joined_at: f64,
    in_battle: bool,
    ready_vehicle: Option<Vec<u8>>,
    ready_inventory: Option<i64>,
    roster: RosterSlot,
    role: i32,
}

impl RoomPlayer {
    fn new(entity: EntityKey<Account>, session: &Session, joined_at: f64, role: i32) -> Self {
        Self {
            entity,
            account_id: session.account.snapshot.id(),
            name: session.account.snapshot.display_name().to_owned(),
            joined_at,
            in_battle: false,
            ready_vehicle: None,
            ready_inventory: None,
            roster: RosterSlot::UnassignedFromTeamOne,
            role,
        }
    }

    fn roster_entry(&self) -> Value {
        let (state, vehicle) = ready_state(self.ready_vehicle.as_deref());
        wot_platform::pickle!((
            (self.entity.id().0),
            (self.name.as_str()),
            (self.account_id),
            ([], []),
            (self.roster as i32),
            (if self.in_battle {
                PLAYER_IN_BATTLE
            } else {
                state
            }),
            (self.joined_at),
            (vehicle),
            0,
            0,
            "",
            0,
            {},
            (self.role)
        ))
    }
}

pub(super) enum SettingArgument<'a> {
    Numeric([i64; 3]),
    Comment(&'a str),
}

fn voice_chat_index(mode: VoiceChatMode) -> i32 {
    match mode {
        VoiceChatMode::General => 0,
        VoiceChatMode::Team => 1,
    }
}

fn apply_setting(
    settings: &mut TrainingRoomSettings,
    battles: &BattleWorker,
    command: i16,
    argument: SettingArgument<'_>,
) -> Option<(&'static str, Value)> {
    match (command, argument) {
        (CHANGE_ARENA, SettingArgument::Numeric([arena, 0, 0])) => {
            let arena = i32::try_from(arena)
                .ok()
                .filter(|id| battles.has_map(*id))?;
            settings.arena_type_id = arena;
            Some(("arenaTypeID", Value::from(arena)))
        }
        (CHANGE_DURATION, SettingArgument::Numeric([seconds, 0, 0])) => {
            let seconds = i32::try_from(seconds)
                .ok()
                .filter(|&value| TrainingRoomSettings::accepts_duration(value))?;
            settings.duration_seconds = seconds;
            Some(("roundLength", Value::from(seconds)))
        }
        (CHANGE_OPEN_STATUS, SettingArgument::Numeric([flag @ (0 | 1), 0, 0])) => {
            settings.open = flag == 1;
            Some(("isOpened", Value::Bool(settings.open)))
        }
        (CHANGE_COMMENT, SettingArgument::Comment(comment))
            if TrainingRoomSettings::accepts_comment(comment) =>
        {
            settings.comment = comment.to_owned();
            Some(("comment", Value::from(comment)))
        }
        (CHANGE_VOICE_CHAT, SettingArgument::Numeric([index, 0, 0])) => {
            settings.voice_chat = match index {
                0 => VoiceChatMode::General,
                1 => VoiceChatMode::Team,
                _ => return None,
            };
            Some((
                "arenaVoipChannels",
                Value::from(voice_chat_index(settings.voice_chat)),
            ))
        }
        _ => None,
    }
}

impl HostedRoom {
    pub(super) fn player_count(&self) -> usize {
        self.members.len()
    }

    pub(super) fn list_entry(&self) -> Value {
        // Numeric keys are PREBATTLE_CACHE_KEY values used by PrbListItem.
        let host = self.members.get(&self.host);
        wot_platform::pickle!((
            (self.created_at), (self.id), {
                1: (TRAINING), 2: (self.settings.open), 5: (self.player_count() as i64),
                6: (host.map_or("", |player| player.name.as_str())), 7: (self.created_at),
                9: (self.settings.comment.as_str()), 11: (self.settings.arena_type_id),
                12: (self.settings.duration_seconds),
                18: (host.map_or(0, |player| player.account_id)), 20: (LOCAL_CLUSTER)
            }
        ))
    }

    fn send_initial_state(&self, base: &mut base::App, entity: EntityKey<Account>) -> Result<()> {
        let host = self
            .members
            .get(&self.host)
            .context("Training Room host is missing")?;
        let settings = &self.settings;
        let default_roster = RosterSlot::UnassignedFromTeamOne as i32;
        let mut account = base.entity(entity)?;
        account.onPrebattleJoined(EntityId(self.id))?;
        account.updatePrebattle(
            SETTINGS,
            Pickle(wot_platform::pickle!({
                "type": (TRAINING), "arenaGuiType": (TRAINING), "bonusType": (TRAINING_BONUS_TYPE),
                "arenaTypeID": (settings.arena_type_id), "roundLength": (settings.duration_seconds),
                "isOpened": (settings.open), "comment": (settings.comment.as_str()),
                "creator": (host.name.as_str()), "creatorDBID": (host.account_id),
                "defaultRoster": (default_roster), "destroyIfCreatorOut": true,
                "limits": {0: {
                    "maxCount": ((MAX_ASSIGNED_PLAYERS as i64), (MAX_ROOM_PLAYERS as i64)),
                    "maxCountTotal": (MAX_ROOM_PLAYERS as i64)
                }},
                "arenaVoipChannels": (voice_chat_index(settings.voice_chat))
            })),
        )?;
        account.updatePrebattle(
            PROPERTIES,
            Pickle(wot_platform::pickle!({"createTime": (self.created_at)})),
        )?;
        account.updatePrebattle(ROSTER, self.roster_snapshot())?;
        Ok(())
    }

    fn roster_snapshot(&self) -> Pickle {
        Pickle(Value::List(
            self.members
                .values()
                .map(RoomPlayer::roster_entry)
                .collect(),
        ))
    }

    fn broadcast(&self, base: &mut base::App, update: u8, data: Pickle, except: Option<ClientKey>) {
        for (&client, member) in &self.members {
            if Some(client) != except && !member.in_battle {
                deliver_training_update(base, client, self.id, |base| {
                    base.entity(member.entity)?
                        .updatePrebattle(update, data.clone())?;
                    Ok(())
                });
            }
        }
    }
}

fn deliver_training_update(
    base: &mut base::App,
    client: ClientKey,
    room_id: u32,
    deliver: impl FnOnce(&mut base::App) -> Result<()>,
) {
    if let Err(error) = deliver(base) {
        error!(%error, room_id, "Training Room update failed");
        base.disconnect_client(client);
    }
}

fn notify_training_left(
    base: &mut base::App,
    entity: EntityKey<Account>,
    room_id: u32,
) -> Result<()> {
    base.entity(entity)?.onPrebattleLeft()?;
    chat::update_membership(base, entity, room_id, false)
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn ready_state(descriptor: Option<&[u8]>) -> (i32, Value) {
    descriptor.map_or((PLAYER_NOT_READY, Value::I64(0)), |descriptor| {
        (PLAYER_READY, Value::Bytes(descriptor.to_vec()))
    })
}

impl Gateway {
    #[tracing::instrument(name = "player_sessions.create_training_room", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.arena_type_id = arena_type_id, input.round_length.seconds = round_length, input.room.open_flag = is_opened, input.string.bytes = comment.len(), output.room.id = tracing::field::Empty, output.outcome = tracing::field::Empty), err)]
    pub(super) fn create_training_room(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
        arena_type_id: i32,
        round_length: i32,
        is_opened: u8,
        comment: &[u8],
    ) -> Result<()> {
        let span = tracing::Span::current();
        let Some(session) = self.sessions.get_mut(&client) else {
            return Ok(());
        };
        if !self.config.training_room_visible || !self.battle_worker.has_maps() {
            span.record("output.outcome", "unavailable");
            self.base
                .entity(entity)?
                .onPrebattleJoinFailure(EVENT_DISABLED)?;
            return Ok(());
        }
        let settings = std::str::from_utf8(comment)
            .ok()
            .filter(|_| is_opened <= 1 && self.battle_worker.has_map(arena_type_id))
            .and_then(|comment| {
                TrainingRoomSettings::new(arena_type_id, round_length, is_opened == 1, comment)
            });
        let Some(settings) = settings else {
            span.record("output.outcome", "invalid_request");
            self.base
                .entity(entity)?
                .onPrebattleJoinFailure(WRONG_ARGS)?;
            return Ok(());
        };
        let (id, outcome) = if let Some(id) = session.training_room {
            let hosting = self
                .training_rooms
                .get(&id)
                .is_some_and(|room| room.host == client);
            if !hosting {
                span.record("output.outcome", "already_in_room");
                self.base
                    .entity(entity)?
                    .onPrebattleJoinFailure(ACCOUNT_LOCK)?;
                return Ok(());
            }
            (id, "already_hosting")
        } else {
            let Some(id) = self.last_training_room_id.checked_add(1) else {
                span.record("output.outcome", "unavailable");
                self.base
                    .entity(entity)?
                    .onPrebattleJoinFailure(EVENT_DISABLED)?;
                return Ok(());
            };
            let created_at = now();
            self.last_training_room_id = id;
            self.training_rooms.insert(
                id,
                HostedRoom {
                    id,
                    settings,
                    created_at,
                    host: client,
                    invites: HashMap::new(),
                    teams_ready: (false, false),
                    members: HashMap::from([(
                        client,
                        RoomPlayer::new(entity, session, created_at, HOST_ROLE),
                    )]),
                },
            );
            session.training_room = Some(id);
            (id, "created")
        };
        span.record("output.room.id", id);
        self.send_training_room_state(client, entity, id, outcome == "created")?;
        span.record("output.outcome", outcome);
        Ok(())
    }

    #[tracing::instrument(name = "player_sessions.join_training_room", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.room.id = args[0], input.cluster.id = args[1], input.invitation = invitation, output.players.count = tracing::field::Empty, output.outcome = tracing::field::Empty), err)]
    pub(super) fn join_training_room(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
        args: [i64; 3],
        invitation: bool,
    ) -> Result<()> {
        let span = tracing::Span::current();
        let Some(session) = self.sessions.get_mut(&client) else {
            return Ok(());
        };
        let cluster = if invitation {
            i64::from(self.config.periphery_id)
        } else {
            LOCAL_CLUSTER
        };
        let outcome = match args {
            [id, requested_cluster, 0] if id > 0 && requested_cluster == cluster => {
                match u32::try_from(id)
                    .ok()
                    .and_then(|id| self.training_rooms.get_mut(&id))
                {
                    Some(room) if session.training_room == Some(room.id) => {
                        let room_id = room.id;
                        span.record("output.players.count", room.player_count());
                        self.send_training_room_state(client, entity, room_id, false)?;
                        span.record("output.outcome", "already_joined");
                        return Ok(());
                    }
                    Some(_) if session.training_room.is_some() => (ACCOUNT_LOCK, "already_in_room"),
                    Some(room)
                        if invitation
                            && !room.invites.contains_key(&session.account.snapshot.id()) =>
                    {
                        (PRIVACY, "invitation_unavailable")
                    }
                    Some(room) if !invitation && !room.settings.open => {
                        (PRIVACY, "invitation_only")
                    }
                    Some(room) if room.player_count() >= MAX_ROOM_PLAYERS => {
                        (ROOM_FULL, "room_full")
                    }
                    Some(room) => {
                        let id = room.id;
                        room.members
                            .insert(client, RoomPlayer::new(entity, session, now(), GUEST_ROLE));
                        session.training_room = Some(id);
                        let count = room.player_count();
                        let invited = room.invites.remove(&session.account.snapshot.id());
                        if invited.is_some() {
                            invites::settle(
                                &mut self.base,
                                session,
                                entity,
                                id,
                                self.config.periphery_id,
                                invites::State::Accepted,
                            );
                        }
                        self.send_training_room_state(client, entity, id, true)?;
                        if let Some(room) = self.training_rooms.get(&id)
                            && let Some(member) = room.members.get(&client)
                        {
                            room.broadcast(
                                &mut self.base,
                                PLAYER_ADDED,
                                Pickle(member.roster_entry()),
                                Some(client),
                            );
                        }
                        span.record("output.players.count", count);
                        span.record("output.outcome", "joined");
                        return Ok(());
                    }
                    None => (NOT_FOUND, "room_unavailable"),
                }
            }
            _ => (WRONG_ARGS, "invalid_request"),
        };
        span.record("output.outcome", outcome.1);
        self.base
            .entity(entity)?
            .onPrebattleJoinFailure(outcome.0)?;
        Ok(())
    }

    fn send_training_room_state(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
        room_id: u32,
        joined: bool,
    ) -> Result<()> {
        let result = self
            .training_rooms
            .get(&room_id)
            .context("Training Room is missing")?
            .send_initial_state(&mut self.base, entity)
            .and_then(|()| {
                if joined {
                    chat::update_membership(&mut self.base, entity, room_id, true)?;
                }
                self.base.flush_client(client).map_err(Into::into)
            });
        if result.is_err() {
            self.base.disconnect_client(client);
            self.remove_session(client);
        }
        result
    }

    #[tracing::instrument(name = "player_sessions.restore_training_room", skip_all, fields(
        ddd.bounded_context.name = "player_sessions", input.entity_id = entity.id().0,
        input.room.id = tracing::field::Empty, output.players.count = tracing::field::Empty,
        output.players.in_battle.count = tracing::field::Empty,
        output.outcome = tracing::field::Empty, otel.status_code = tracing::field::Empty,
    ), err)]
    pub(super) fn restore_training_room(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
    ) -> Result<()> {
        let span = tracing::Span::current();
        let Some(room) = self
            .sessions
            .get(&client)
            .and_then(|session| session.training_room)
            .and_then(|id| self.training_rooms.get_mut(&id))
        else {
            span.record("output.outcome", "no_membership");
            return Ok(());
        };
        let Some(member) = room
            .members
            .get_mut(&client)
            .filter(|member| member.in_battle)
        else {
            span.record("output.outcome", "already_in_room");
            return Ok(());
        };
        member.entity = entity;
        member.in_battle = false;
        member.ready_vehicle = None;
        let id = room.id;
        span.record("input.room.id", id);
        span.record("output.players.count", room.player_count());
        span.record(
            "output.players.in_battle.count",
            room.members
                .values()
                .filter(|member| member.in_battle)
                .count(),
        );
        room.broadcast(&mut self.base, ROSTER, room.roster_snapshot(), Some(client));
        self.send_training_room_state(client, entity, id, true)
            .inspect_err(|_| {
                span.record("otel.status_code", "ERROR");
            })?;
        span.record("output.outcome", "restored");
        Ok(())
    }

    #[tracing::instrument(name = "player_sessions.leave_training_room", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.request_id = request_id, output.room.id = tracing::field::Empty, output.outcome = tracing::field::Empty), err)]
    pub(super) fn leave_training_room(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
        request_id: i16,
    ) -> Result<()> {
        let span = tracing::Span::current();
        let membership = self.remove_training_member(client);
        let outcome = if let Some((room_id, outcome)) = membership {
            span.record("output.room.id", room_id);
            notify_training_left(&mut self.base, entity, room_id)?;
            outcome
        } else {
            "already_left"
        };
        self.base
            .entity(entity)?
            .onCmdResponse(request_id, SUCCESS, String::new())?;
        span.record("output.outcome", outcome);
        Ok(())
    }

    #[tracing::instrument(name = "player_sessions.kick_training_player", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.request_id = request_id, input.player_entity_id = args[0], output.room.id = tracing::field::Empty, output.outcome = tracing::field::Empty), err)]
    pub(super) fn kick_training_player(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
        request_id: i16,
        args: [i64; 3],
    ) -> Result<()> {
        let span = tracing::Span::current();
        let room = self
            .sessions
            .get(&client)
            .and_then(|session| session.training_room)
            .and_then(|id| self.training_rooms.get(&id));
        if let Some(room) = room {
            span.record("output.room.id", room.id);
        }
        let target = match (room, args) {
            (None, _) => Err(("room_unavailable", "Training Room is unavailable")),
            (Some(room), _) if room.host != client => Err((
                "host_required",
                "Only the Training Room host can remove players",
            )),
            (Some(room), [player_id, 0, 0]) => room
                .members
                .iter()
                .find(|(key, member)| {
                    **key != client
                        && !member.in_battle
                        && i64::from(member.entity.id().0) == player_id
                })
                .map(|(&key, member)| (key, member.entity))
                .ok_or((
                    "invalid_target",
                    "Select another player in this Training Room",
                )),
            _ => Err(("invalid_request", "Invalid Training Room removal")),
        };
        let result = target.and_then(|(guest, guest_entity)| {
            self.remove_training_member(guest)
                .map(|(room_id, _)| (guest, guest_entity, room_id))
                .ok_or(("invalid_target", "The player has already left"))
        });
        let (outcome, code, message) = match result {
            Ok((guest, guest_entity, room_id)) => {
                deliver_training_update(&mut self.base, guest, room_id, |base| {
                    base.entity(guest_entity)?
                        .onKickedFromPrebattle(PLAYER_KICK)?;
                    chat::update_membership(base, guest_entity, room_id, false)
                });
                ("kicked", SUCCESS, "")
            }
            Err((outcome, message)) => (outcome, FAILURE, message),
        };
        span.record("output.outcome", outcome);
        self.base
            .entity(entity)?
            .onCmdResponse(request_id, code, message.into())?;
        Ok(())
    }

    #[tracing::instrument(name = "player_sessions.remove_training_member", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.client.address = %client, output.room.id = tracing::field::Empty, output.players.count = tracing::field::Empty, output.outcome = tracing::field::Empty))]
    pub(super) fn remove_training_member(
        &mut self,
        client: ClientKey,
    ) -> Option<(u32, &'static str)> {
        let span = tracing::Span::current();
        let Some(room_id) = self
            .sessions
            .get_mut(&client)
            .and_then(|session| session.training_room.take())
        else {
            span.record("output.outcome", "already_left");
            return None;
        };
        span.record("output.room.id", room_id);
        let Some(room) = self.training_rooms.get_mut(&room_id) else {
            span.record("output.outcome", "already_left");
            return None;
        };
        let Some(member) = room.members.remove(&client) else {
            span.record("output.outcome", "already_left");
            return None;
        };
        let outcome = if room.host == client {
            if let Some(room) = self.training_rooms.remove(&room_id) {
                for session in self.sessions.values() {
                    if room.invites.contains_key(&session.account.snapshot.id())
                        && session.garage_synced
                        && let Some(entity) = session.entity
                    {
                        invites::settle(
                            &mut self.base,
                            session,
                            entity,
                            room_id,
                            self.config.periphery_id,
                            invites::State::Expired,
                        );
                    }
                }
                for (guest, member) in room.members {
                    if let Some(session) = self.sessions.get_mut(&guest) {
                        session.training_room = None;
                    }
                    if !member.in_battle {
                        deliver_training_update(&mut self.base, guest, room_id, |base| {
                            notify_training_left(base, member.entity, room_id)
                        });
                    }
                }
            }
            span.record("output.players.count", 0);
            "deleted"
        } else {
            if matches!(member.roster, RosterSlot::TeamOne | RosterSlot::TeamTwo) {
                room.clear_team_readiness();
            }
            let removed = Pickle(wot_platform::pickle!((
                (member.entity.id().0),
                (member.roster as i32)
            )));
            room.broadcast(&mut self.base, PLAYER_REMOVED, removed, None);
            span.record("output.players.count", room.player_count());
            "left"
        };
        span.record("output.outcome", outcome);
        Some((room_id, outcome))
    }

    #[tracing::instrument(name = "player_sessions.assign_training_roster", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.request_id = request_id, input.player_entity_id = player_entity_id, input.roster.slot = roster_slot, input.roster.group = group, output.room.id = tracing::field::Empty, output.roster.previous_slot = tracing::field::Empty, output.outcome = tracing::field::Empty), err)]
    pub(super) fn assign_training_roster(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
        request_id: i16,
        player_entity_id: i64,
        roster_slot: i64,
        group: i64,
    ) -> Result<()> {
        let span = tracing::Span::current();
        let Some(session) = self.sessions.get(&client) else {
            return Ok(());
        };
        let Some(room) = session
            .training_room
            .and_then(|id| self.training_rooms.get_mut(&id))
        else {
            span.record("output.outcome", "room_unavailable");
            self.base.entity(entity)?.onCmdResponse(
                request_id,
                FAILURE,
                "Training Room is unavailable".into(),
            )?;
            return Ok(());
        };
        span.record("output.room.id", room.id);
        if room.host != client {
            span.record("output.outcome", "host_required");
            self.base.entity(entity)?.onCmdResponse(
                request_id,
                FAILURE,
                "Only the Training Room host can do this".into(),
            )?;
            return Ok(());
        }
        let assignment = RosterSlot::from_wire(roster_slot)
            .filter(|_| group == 0)
            .map(|destination| {
                let occupied = room
                    .members
                    .values()
                    .filter(|member| member.roster == destination)
                    .count();
                (destination, occupied >= destination.capacity())
            })
            .zip(
                room.members
                    .values_mut()
                    .find(|member| i64::from(member.entity.id().0) == player_entity_id),
            );
        let Some(((destination, full), member)) = assignment else {
            span.record("output.outcome", "invalid_request");
            self.base.entity(entity)?.onCmdResponse(
                request_id,
                FAILURE,
                "Invalid Training Room assignment".into(),
            )?;
            return Ok(());
        };
        let previous = member.roster;
        span.record("output.roster.previous_slot", previous as i32);
        if previous != destination && full {
            span.record("output.outcome", "roster_full");
            self.base
                .entity(entity)?
                .onCmdResponse(request_id, FAILURE, "ROSTER_LIMIT".into())?;
            return Ok(());
        }
        let outcome = if previous == destination {
            "unchanged"
        } else {
            "assigned"
        };
        member.roster = destination;
        if previous != destination {
            room.clear_team_readiness();
        }
        room.broadcast(&mut self.base, ROSTER, room.roster_snapshot(), None);
        span.record("output.outcome", outcome);
        self.base
            .entity(entity)?
            .onCmdResponse(request_id, SUCCESS, String::new())?;
        Ok(())
    }

    #[tracing::instrument(name = "player_sessions.swap_training_teams", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.request_id = request_id, output.room.id = tracing::field::Empty, output.roster.previous_slot = tracing::field::Empty, output.roster.slot = tracing::field::Empty, output.outcome = tracing::field::Empty), err)]
    pub(super) fn swap_training_teams(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
        request_id: i16,
        args: [i64; 3],
    ) -> Result<()> {
        let span = tracing::Span::current();
        let Some(session) = self.sessions.get(&client) else {
            return Ok(());
        };
        let Some(room) = session
            .training_room
            .and_then(|id| self.training_rooms.get_mut(&id))
        else {
            span.record("output.outcome", "room_unavailable");
            self.base.entity(entity)?.onCmdResponse(
                request_id,
                FAILURE,
                "Training Room is unavailable".into(),
            )?;
            return Ok(());
        };
        span.record("output.room.id", room.id);
        if room.host != client {
            span.record("output.outcome", "host_required");
            self.base.entity(entity)?.onCmdResponse(
                request_id,
                FAILURE,
                "Only the Training Room host can do this".into(),
            )?;
            return Ok(());
        }
        if args != [0, 0, 0] {
            span.record("output.outcome", "invalid_request");
            self.base.entity(entity)?.onCmdResponse(
                request_id,
                FAILURE,
                "Invalid Training Room team swap".into(),
            )?;
            return Ok(());
        }
        if let Some(host) = room.members.get(&client) {
            span.record("output.roster.previous_slot", host.roster as i32);
            span.record("output.roster.slot", host.roster.swap_team() as i32);
        }
        let mut changed = false;
        for member in room.members.values_mut() {
            let destination = member.roster.swap_team();
            changed |= destination != member.roster;
            member.roster = destination;
        }
        let outcome = if changed {
            room.clear_team_readiness();
            "swapped"
        } else {
            "unchanged"
        };
        room.broadcast(&mut self.base, ROSTER, room.roster_snapshot(), None);
        span.record("output.outcome", outcome);
        self.base
            .entity(entity)?
            .onCmdResponse(request_id, SUCCESS, String::new())?;
        Ok(())
    }

    #[tracing::instrument(name = "player_sessions.change_training_setting", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.request_id = request_id, input.command_id = command, output.room.id = tracing::field::Empty, output.outcome = tracing::field::Empty), err)]
    pub(super) fn change_training_setting(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
        request_id: i16,
        command: i16,
        argument: SettingArgument<'_>,
    ) -> Result<()> {
        let span = tracing::Span::current();
        let Some(session) = self.sessions.get(&client) else {
            return Ok(());
        };
        let Some(room) = session
            .training_room
            .and_then(|id| self.training_rooms.get_mut(&id))
        else {
            span.record("output.outcome", "room_unavailable");
            self.base.entity(entity)?.onCmdResponse(
                request_id,
                FAILURE,
                "Training Room is unavailable".into(),
            )?;
            return Ok(());
        };
        span.record("output.room.id", room.id);
        if room.host != client {
            span.record("output.outcome", "host_required");
            self.base.entity(entity)?.onCmdResponse(
                request_id,
                FAILURE,
                "Only the Training Room host can do this".into(),
            )?;
            return Ok(());
        }
        let mut next = room.settings.clone();
        let Some((key, value)) = apply_setting(&mut next, &self.battle_worker, command, argument)
        else {
            span.record("output.outcome", "invalid_request");
            self.base.entity(entity)?.onCmdResponse(
                request_id,
                FAILURE,
                "Invalid Training Room setting".into(),
            )?;
            return Ok(());
        };
        if next != room.settings {
            room.broadcast(
                &mut self.base,
                SETTING,
                Pickle(wot_platform::pickle!(((key), (value)))),
                None,
            );
            room.settings = next;
            room.clear_team_readiness();
            span.record("output.outcome", "changed");
        } else {
            span.record("output.outcome", "unchanged");
        }
        self.base
            .entity(entity)?
            .onCmdResponse(request_id, SUCCESS, String::new())?;
        Ok(())
    }

    #[tracing::instrument(name = "player_sessions.set_training_ready", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.request_id = request_id, input.inventory_id = vehicle_id, output.room.id = tracing::field::Empty, output.outcome = tracing::field::Empty), err)]
    pub(super) fn set_training_ready(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
        request_id: i16,
        vehicle_id: Option<i64>,
    ) -> Result<()> {
        let span = tracing::Span::current();
        let Some(session) = self.sessions.get(&client) else {
            return Ok(());
        };
        let Some(room) = session
            .training_room
            .and_then(|id| self.training_rooms.get_mut(&id))
        else {
            span.record("output.outcome", "room_unavailable");
            self.base.entity(entity)?.onCmdResponse(
                request_id,
                FAILURE,
                "Training Room is unavailable".into(),
            )?;
            return Ok(());
        };
        span.record("output.room.id", room.id);
        let Some(member) = room.members.get_mut(&client) else {
            span.record("output.outcome", "room_unavailable");
            self.base.entity(entity)?.onCmdResponse(
                request_id,
                FAILURE,
                "Training Room is unavailable".into(),
            )?;
            return Ok(());
        };
        let (descriptor, outcome) = match vehicle_id {
            Some(OBSERVER_INVENTORY_ID) => (
                Ok(self
                    .catalog
                    .training_observer_descriptor()
                    .map(<[u8]>::to_vec)),
                "observer",
            ),
            Some(vehicle_id) => (
                self.catalog
                    .training_vehicle_descriptor(&session.account.snapshot, vehicle_id),
                "ready",
            ),
            None => (Ok(None), "not_ready"),
        };
        let descriptor = match descriptor {
            Ok(None) if vehicle_id.is_some() => {
                span.record("output.outcome", "vehicle_unavailable");
                self.base.entity(entity)?.onCmdResponse(
                    request_id,
                    FAILURE,
                    "Training vehicle is unavailable".into(),
                )?;
                return Ok(());
            }
            Ok(descriptor) => descriptor,
            Err(error) => {
                self.base.entity(entity)?.onCmdResponse(
                    request_id,
                    FAILURE,
                    "Training vehicle is unavailable".into(),
                )?;
                return Err(error);
            }
        };
        let (state, vehicle) = ready_state(descriptor.as_deref());
        let player_entity = entity.id().0;
        let roster = member.roster as i32;
        let role = member.role;
        let readiness_changed =
            member.ready_inventory != vehicle_id || member.ready_vehicle != descriptor;
        member.ready_vehicle = descriptor;
        member.ready_inventory = vehicle_id;
        if readiness_changed && matches!(roster, 1 | 2) {
            room.clear_team_readiness();
        }
        room.broadcast(
            &mut self.base,
            PLAYER_STATE,
            Pickle(wot_platform::pickle!((
                (player_entity),
                (roster),
                (state),
                (vehicle),
                0,
                ([], []),
                0,
                "",
                {},
                (role)
            ))),
            None,
        );
        self.base
            .entity(entity)?
            .onCmdResponse(request_id, SUCCESS, String::new())?;
        span.record("output.outcome", outcome);
        Ok(())
    }
}
