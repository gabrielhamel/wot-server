use super::*;
use crate::platform::client_formats::pxml;
use crate::player_sessions::{
    domain::training::{TrainingRoomSettings, VoiceChatMode},
    mercury::app::entity::EntityId,
};
use std::{
    io::BufReader,
    time::{SystemTime, UNIX_EPOCH},
};

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

const TRAINING: i32 = 2;
const WRONG_ARGS: u8 = 6;
const EVENT_DISABLED: u8 = 14;
const ROSTER: u8 = 1;
const PLAYER_STATE: u8 = 4;
const SETTINGS: u8 = 7;
const SETTING: u8 = 8;
const PROPERTIES: u8 = 10;
const HOST_ROLE: i32 = 0xfe7f;
const HOST_NOT_READY: i32 = 1;
const HOST_READY: i32 = 4;
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
    ready_vehicle: Option<Vec<u8>>,
    roster: RosterSlot,
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
    arenas: &HashSet<i32>,
    command: i16,
    argument: SettingArgument<'_>,
) -> Option<(&'static str, Value)> {
    match (command, argument) {
        (CHANGE_ARENA, SettingArgument::Numeric([arena, 0, 0])) => {
            let arena = i32::try_from(arena).ok().filter(|id| arenas.contains(id))?;
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
    fn send_initial_state(
        &self,
        base: &mut base::App,
        entity: EntityKey<Account>,
        snapshot: &crate::accounts::contract::AccountView,
    ) -> Result<()> {
        let name = snapshot.display_name();
        let account_id = snapshot.id();
        let settings = &self.settings;
        let default_roster = RosterSlot::UnassignedFromTeamOne as i32;
        let mut account = base.entity(entity)?;
        account.onPrebattleJoined(EntityId(self.id))?;
        account.updatePrebattle(
            SETTINGS,
            Pickle(crate::pickle!({
                "type": (TRAINING), "arenaGuiType": (TRAINING),
                "arenaTypeID": (settings.arena_type_id), "roundLength": (settings.duration_seconds),
                "isOpened": (settings.open), "comment": (settings.comment.as_str()),
                "creator": (name), "creatorDBID": (account_id), "defaultRoster": (default_roster),
                "limits": {0: {}, 1: {}, 2: {}},
                "arenaVoipChannels": (voice_chat_index(settings.voice_chat))
            })),
        )?;
        account.updatePrebattle(
            PROPERTIES,
            Pickle(crate::pickle!({"createTime": (self.created_at)})),
        )?;
        account.updatePrebattle(ROSTER, self.roster_snapshot(entity, snapshot, self.roster))?;
        Ok(())
    }

    fn roster_snapshot(
        &self,
        entity: EntityKey<Account>,
        snapshot: &crate::accounts::contract::AccountView,
        slot: RosterSlot,
    ) -> Pickle {
        let player_entity = entity.id().0;
        let name = snapshot.display_name();
        let account_id = snapshot.id();
        let roster = slot as i32;
        let (state, vehicle) = ready_state(self.ready_vehicle.as_deref());
        Pickle(crate::pickle!([(
            (player_entity),
            (name),
            (account_id),
            ([], []),
            (roster),
            (state),
            (self.created_at),
            (vehicle),
            0,
            0,
            "",
            0,
            {},
            (HOST_ROLE)
        )]))
    }
}

fn ready_state(descriptor: Option<&[u8]>) -> (i32, Value) {
    descriptor.map_or((HOST_NOT_READY, Value::I64(0)), |descriptor| {
        (HOST_READY, Value::Bytes(descriptor.to_vec()))
    })
}

pub(super) fn load_arenas(resources: &ResourceBundle) -> Result<HashSet<i32>> {
    const LIST: &str = "scripts/arena_defs/_list_.xml";
    if !resources.has_file(LIST) {
        return Ok(HashSet::new());
    }
    let list = pxml::from_reader(BufReader::new(resources.read(LIST)?))?;
    let defaults = pxml::from_reader(BufReader::new(
        resources.read("scripts/arena_defs/_default_.xml")?,
    ))?;
    let mut arenas = HashSet::new();
    for map in list
        .iter_children("map")
        .filter_map(pxml::Value::as_element)
    {
        if map.get_child_boolean("isHangar") == Some(true) {
            continue;
        }
        let Some(geometry) = map
            .get_child_integer("id")
            .and_then(|id| u16::try_from(id).ok())
        else {
            continue;
        };
        let name = map
            .get_child_string("name")
            .context("Arena entry has no name")?;
        let path = format!("scripts/arena_defs/{name}.xml");
        if !resources.has_file(&path) && map.get_child_boolean("isDevelopment") == Some(true) {
            continue;
        }
        let map = pxml::from_reader(BufReader::new(resources.read(&path)?))
            .with_context(|| path.clone())?;
        if let Some(gameplays) = map.get_child_element("gameplayTypes") {
            for (gameplay, id) in [
                ("ctf", 0),
                ("domination", 1),
                ("assault", 2),
                ("assault2", 6),
            ] {
                if let Some(variant) = gameplays.get_child(gameplay) {
                    let explicit = variant
                        .as_element()
                        .and_then(|data| data.get_child_boolean("explicitRequestOnly"))
                        .or_else(|| {
                            defaults
                                .get_child_element("gameplayTypes")?
                                .get_child_element(gameplay)?
                                .get_child_boolean("explicitRequestOnly")
                        });
                    if explicit != Some(true) {
                        arenas.insert((id << 16) | i32::from(geometry));
                    }
                }
            }
        } else {
            arenas.insert(i32::from(geometry));
        }
    }
    Ok(arenas)
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
        if !self.config.training_room_visible || self.training_arenas.is_empty() {
            span.record("output.outcome", "unavailable");
            self.base
                .entity(entity)?
                .onPrebattleJoinFailure(EVENT_DISABLED)?;
            return Ok(());
        }
        let settings = std::str::from_utf8(comment)
            .ok()
            .filter(|_| is_opened <= 1 && self.training_arenas.contains(&arena_type_id))
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
        let outcome = if session.training_room.is_some() {
            "already_hosting"
        } else {
            "created"
        };
        if session.training_room.is_none() {
            let Some(id) = self.last_training_room_id.checked_add(1) else {
                span.record("output.outcome", "unavailable");
                self.base
                    .entity(entity)?
                    .onPrebattleJoinFailure(EVENT_DISABLED)?;
                return Ok(());
            };
            self.last_training_room_id = id;
            session.training_room = Some(HostedRoom {
                id,
                settings,
                created_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs_f64(),
                ready_vehicle: None,
                roster: RosterSlot::UnassignedFromTeamOne,
            });
        }
        let Some(room) = &session.training_room else {
            return Ok(());
        };
        span.record("output.room.id", room.id);
        let result = room
            .send_initial_state(&mut self.base, entity, &session.account.snapshot)
            .and_then(|()| self.base.flush_client(client).map_err(Into::into));
        if result.is_err() {
            self.base.disconnect_client(client);
            self.remove_session(client);
        } else {
            span.record("output.outcome", outcome);
        }
        result
    }

    #[tracing::instrument(name = "player_sessions.leave_training_room", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.request_id = request_id, output.room.id = tracing::field::Empty, output.outcome = tracing::field::Empty), err)]
    pub(super) fn leave_training_room(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
        request_id: i16,
    ) -> Result<()> {
        let span = tracing::Span::current();
        let Some(session) = self.sessions.get_mut(&client) else {
            return Ok(());
        };
        let mut account = self.base.entity(entity)?;
        if let Some(room) = &session.training_room {
            account.onPrebattleLeft()?;
            span.record("output.room.id", room.id);
            session.training_room = None;
            span.record("output.outcome", "left");
        } else {
            span.record("output.outcome", "already_left");
        }
        account.onCmdResponse(request_id, SUCCESS, String::new())?;
        Ok(())
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
        let Some(session) = self.sessions.get_mut(&client) else {
            return Ok(());
        };
        let mut account = self.base.entity(entity)?;
        let Some(room) = &mut session.training_room else {
            span.record("output.outcome", "room_unavailable");
            account.onCmdResponse(request_id, FAILURE, "Training Room is unavailable".into())?;
            return Ok(());
        };
        span.record("output.room.id", room.id);
        let player = entity.id().0;
        let Some(destination) = RosterSlot::from_wire(roster_slot)
            .filter(|_| player_entity_id == i64::from(player) && group == 0)
        else {
            span.record("output.outcome", "invalid_request");
            account.onCmdResponse(
                request_id,
                FAILURE,
                "Invalid Training Room assignment".into(),
            )?;
            return Ok(());
        };
        let previous = room.roster as i32;
        span.record("output.roster.previous_slot", previous);
        let outcome = if destination == room.roster {
            "unchanged"
        } else {
            "assigned"
        };
        account.updatePrebattle(
            ROSTER,
            room.roster_snapshot(entity, &session.account.snapshot, destination),
        )?;
        room.roster = destination;
        span.record("output.outcome", outcome);
        account.onCmdResponse(request_id, SUCCESS, String::new())?;
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
        let Some(session) = self.sessions.get_mut(&client) else {
            return Ok(());
        };
        let mut account = self.base.entity(entity)?;
        let Some(room) = &mut session.training_room else {
            span.record("output.outcome", "room_unavailable");
            account.onCmdResponse(request_id, FAILURE, "Training Room is unavailable".into())?;
            return Ok(());
        };
        span.record("output.room.id", room.id);
        if args != [0, 0, 0] {
            span.record("output.outcome", "invalid_request");
            account.onCmdResponse(
                request_id,
                FAILURE,
                "Invalid Training Room team swap".into(),
            )?;
            return Ok(());
        }
        let destination = room.roster.swap_team();
        let outcome = if destination == room.roster {
            "unchanged"
        } else {
            "swapped"
        };
        span.record("output.roster.previous_slot", room.roster as i32);
        account.updatePrebattle(
            ROSTER,
            room.roster_snapshot(entity, &session.account.snapshot, destination),
        )?;
        room.roster = destination;
        span.record("output.roster.slot", destination as i32);
        span.record("output.outcome", outcome);
        account.onCmdResponse(request_id, SUCCESS, String::new())?;
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
        let Some(session) = self.sessions.get_mut(&client) else {
            return Ok(());
        };
        let mut account = self.base.entity(entity)?;
        let Some(room) = &mut session.training_room else {
            span.record("output.outcome", "room_unavailable");
            account.onCmdResponse(request_id, FAILURE, "Training Room is unavailable".into())?;
            return Ok(());
        };
        span.record("output.room.id", room.id);
        let mut next = room.settings.clone();
        let Some((key, value)) = apply_setting(&mut next, &self.training_arenas, command, argument)
        else {
            span.record("output.outcome", "invalid_request");
            account.onCmdResponse(request_id, FAILURE, "Invalid Training Room setting".into())?;
            return Ok(());
        };
        if next != room.settings {
            account.updatePrebattle(SETTING, Pickle(crate::pickle!(((key), (value)))))?;
            room.settings = next;
            span.record("output.outcome", "changed");
        } else {
            span.record("output.outcome", "unchanged");
        }
        account.onCmdResponse(request_id, SUCCESS, String::new())?;
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
        let Some(session) = self.sessions.get_mut(&client) else {
            return Ok(());
        };
        let mut account = self.base.entity(entity)?;
        let Some(room) = &mut session.training_room else {
            span.record("output.outcome", "room_unavailable");
            account.onCmdResponse(request_id, FAILURE, "Training Room is unavailable".into())?;
            return Ok(());
        };
        span.record("output.room.id", room.id);
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
                account.onCmdResponse(
                    request_id,
                    FAILURE,
                    "Training vehicle is unavailable".into(),
                )?;
                return Ok(());
            }
            Ok(descriptor) => descriptor,
            Err(error) => {
                account.onCmdResponse(
                    request_id,
                    FAILURE,
                    "Training vehicle is unavailable".into(),
                )?;
                return Err(error);
            }
        };
        let (state, vehicle) = ready_state(descriptor.as_deref());
        let player_entity = entity.id().0;
        let roster = room.roster as i32;
        account.updatePrebattle(
            PLAYER_STATE,
            Pickle(crate::pickle!((
                (player_entity),
                (roster),
                (state),
                (vehicle),
                0,
                ([], []),
                0,
                "",
                {},
                (HOST_ROLE)
            ))),
        )?;
        room.ready_vehicle = descriptor;
        account.onCmdResponse(request_id, SUCCESS, String::new())?;
        span.record("output.outcome", outcome);
        Ok(())
    }
}
