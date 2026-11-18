use super::*;
use crate::platform::client_formats::pxml;
use crate::player_sessions::{
    domain::training::TrainingRoomSettings, mercury::app::entity::EntityId,
};
use std::{
    io::BufReader,
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) const LEAVE: i16 = 401;
pub(super) const READY: i16 = 402;
pub(super) const NOT_READY: i16 = 403;

const TRAINING: i32 = 2;
const WRONG_ARGS: u8 = 6;
const EVENT_DISABLED: u8 = 14;
const ROSTER: u8 = 1;
const PLAYER_STATE: u8 = 4;
const SETTINGS: u8 = 7;
const PROPERTIES: u8 = 10;
const UNASSIGNED_TEAM_ONE: i32 = 17;
const HOST_ROLE: i32 = 0xfe7f;
const HOST_NOT_READY: i32 = 1;
const HOST_READY: i32 = 4;

pub(super) struct HostedRoom {
    id: u32,
    settings: TrainingRoomSettings,
    created_at: f64,
    ready_vehicle: Option<Vec<u8>>,
}

impl HostedRoom {
    fn send_initial_state(
        &self,
        base: &mut base::App,
        entity: EntityKey<Account>,
        snapshot: &crate::accounts::AccountSnapshot,
    ) -> Result<()> {
        let name = snapshot.display_name.as_str();
        let account_id = snapshot.id;
        let player_entity = entity.id().0;
        let settings = &self.settings;
        let (state, vehicle) = ready_state(self.ready_vehicle.as_deref());
        let mut account = base.entity(entity)?;
        account.onPrebattleJoined(EntityId(self.id))?;
        account.updatePrebattle(SETTINGS, Pickle(crate::pickle!({
            "type": (TRAINING), "arenaGuiType": (TRAINING),
            "arenaTypeID": (settings.arena_type_id), "roundLength": (settings.duration_seconds),
            "isOpened": (settings.open), "comment": (settings.comment.as_str()),
            "creator": (name), "creatorDBID": (account_id), "defaultRoster": (UNASSIGNED_TEAM_ONE),
            "limits": {0: {}, 1: {}, 2: {}}
        })))?;
        account.updatePrebattle(
            PROPERTIES,
            Pickle(crate::pickle!({"createTime": (self.created_at)})),
        )?;
        account.updatePrebattle(
            ROSTER,
            Pickle(crate::pickle!([(
                (player_entity),
                (name),
                (account_id),
                ([], []),
                (UNASSIGNED_TEAM_ONE),
                (state),
                (self.created_at),
                (vehicle),
                0,
                0,
                "",
                0,
                {},
                (HOST_ROLE)
            )])),
        )?;
        Ok(())
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
        let descriptor = if let Some(vehicle_id) = vehicle_id {
            match self
                .catalog
                .training_vehicle_descriptor(&session.account.snapshot, vehicle_id)
            {
                Ok(Some(descriptor)) => Some(descriptor),
                Ok(None) => {
                    span.record("output.outcome", "vehicle_unavailable");
                    account.onCmdResponse(
                        request_id,
                        FAILURE,
                        "Training vehicle is unavailable".into(),
                    )?;
                    return Ok(());
                }
                Err(error) => {
                    account.onCmdResponse(
                        request_id,
                        FAILURE,
                        "Training vehicle is unavailable".into(),
                    )?;
                    return Err(error);
                }
            }
        } else {
            None
        };
        let (state, vehicle) = ready_state(descriptor.as_deref());
        let player_entity = entity.id().0;
        account.updatePrebattle(
            PLAYER_STATE,
            Pickle(crate::pickle!((
                (player_entity),
                (UNASSIGNED_TEAM_ONE),
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
        span.record(
            "output.outcome",
            if vehicle_id.is_some() {
                "ready"
            } else {
                "not_ready"
            },
        );
        Ok(())
    }
}
