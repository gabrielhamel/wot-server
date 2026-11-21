//! Transition one Training Room roster into a shared arena on the existing BaseApp channels.

use super::*;
use crate::{
    accounts::contract::TrainingVehicle,
    player_sessions::{
        domain::{
            training::{LOAD_TIMEOUT, RoundPhase, TrainingRound},
            training_vehicle::TrainingTank,
        },
        infrastructure::schema::{Avatar, AvatarBase},
        mercury::app::{
            client::element::arena::{CreateArenaEntity, LeaveArenaEntity},
            entity::Bytes,
        },
    },
};
use glam::{Vec2, Vec3};
use std::io::Write;
use wire::ArenaEntry;
pub(in crate::player_sessions::infrastructure::gateway) use wire::ArenaWire;

mod controls;
mod wire;

pub(in crate::player_sessions::infrastructure::gateway) const TEAM_READY: i16 = 411;
pub(in crate::player_sessions::infrastructure::gateway) const TEAM_NOT_READY: i16 = 412;
const TEAM_STATES: u8 = 6;
const ARENA_PERIOD: u8 = 3;
const TIME_LIMIT: u8 = 3;
const TICK: Duration = Duration::from_millis(100);

#[derive(Clone)]
pub(in crate::player_sessions::infrastructure::gateway) struct ArenaMap {
    pub(super) geometry: String,
    spawns: [Vec2; 2],
    bounds: [Vec2; 2],
}

impl ArenaMap {
    pub(super) fn from_definition(definition: &pxml::Element, gameplay: &str) -> Option<Self> {
        let geometry = definition.get_child_string("geometry")?;
        let variant = definition
            .get_child_element("gameplayTypes")?
            .get_child_element(gameplay)?;
        let spawn = |team| {
            variant
                .get_child_value_at(&["teamSpawnPoints", team, "position"])
                .or_else(|| variant.get_child_value_at(&["teamBasePositions", team, "position1"]))
                .and_then(pxml::Value::as_vec2)
                .filter(|point| point.is_finite())
        };
        let corner = |name| {
            definition
                .get_child_value_at(&["boundingBox", name])
                .and_then(pxml::Value::as_vec2)
                .filter(|point| point.is_finite())
        };
        let bounds = [corner("bottomLeft")?, corner("upperRight")?];
        if !bounds[0].cmplt(bounds[1]).all() {
            return None;
        }
        Some(Self {
            bounds,
            geometry: geometry.to_owned(),
            spawns: [spawn("team1")?, spawn("team2")?],
        })
    }
}

struct Entrant {
    account_id: i64,
    name: String,
    team: u8,
    is_host: bool,
    observer: bool,
    vehicle: TrainingVehicle,
    avatar_id: u32,
    vehicle_id: u32,
    info_id: u32,
    position: Vec3,
    direction: Vec3,
}

impl Entrant {
    fn sample() -> Self {
        Self {
            account_id: 1,
            name: "Training Player".into(),
            team: 1,
            is_host: true,
            observer: false,
            vehicle: TrainingVehicle {
                descriptor: vec![1],
                health: 100,
                crew_size: 3,
                ammunition: vec![(10, 5)],
            },
            avatar_id: 1,
            vehicle_id: 2,
            info_id: 3,
            position: Vec3::ZERO,
            direction: Vec3::ZERO,
        }
    }
}

pub(in crate::player_sessions::infrastructure::gateway) struct TrainingBattle {
    map: ArenaMap,
    arena_type: i32,
    players: HashMap<ClientKey, Participant>,
    started_at: Instant,
    round: TrainingRound,
    launch: Option<tracing::Span>,
    shot_sequence: i32,
    result: Option<(i8, u8)>,
}

struct Participant {
    vehicle: CreateArenaEntity,
    avatar_id: u32,
    team: u8,
    entry: ArenaEntry,
    avatar: Option<EntityKey<Avatar>>,
    ready: bool,
    tank: TrainingTank,
    observer: bool,
    last_gun_angles: u16,
    pending_drive: Option<tracing::Span>,
    pending_aim: Option<tracing::Span>,
}

impl TrainingBattle {
    fn ticks(&self, now: Instant) -> u32 {
        (now.saturating_duration_since(self.started_at).as_millis() / TICK.as_millis()) as u32
    }

    fn has_both_teams(&self) -> bool {
        [1, 2]
            .into_iter()
            .all(|team| self.players.values().any(|player| player.team == team))
    }

    #[tracing::instrument(name = "player_sessions.training_battle_period", skip_all, fields(
        ddd.bounded_context.name = "player_sessions", input.room.id = battle_id,
        input.players.count = self.players.len(), output.phase = match self.round.phase {
            RoundPhase::Loading => "loading",
            RoundPhase::Countdown => "countdown",
            RoundPhase::Battle => "battle",
            RoundPhase::Finished => "finished",
        }, otel.status_code = tracing::field::Empty,
    ))]
    fn publish_period(&self, base: &mut base::App, battle_id: u32, now: Instant) {
        for (&client, player) in &self.players {
            if let Some(avatar) = player.avatar
                && let Err(error) = self.send_period(base, avatar, now)
            {
                tracing::Span::current().record("otel.status_code", "ERROR");
                error!(%error, "Could not send training battle period");
                base.disconnect_client(client);
            }
        }
    }

    fn send_period(
        &self,
        base: &mut base::App,
        avatar: EntityKey<Avatar>,
        now: Instant,
    ) -> Result<()> {
        let (winner, reason) = self.result.unwrap_or((0, TIME_LIMIT));
        let (period, extra) = match self.round.phase {
            RoundPhase::Loading => return Ok(()),
            RoundPhase::Countdown => (2, Value::List(Vec::new())),
            RoundPhase::Battle => (3, Value::List(Vec::new())),
            RoundPhase::Finished => (
                4,
                crate::pickle!(((i32::from(winner)), (i32::from(reason)))),
            ),
        };
        let end = f64::from(self.ticks(now)) * TICK.as_secs_f64()
            + self
                .round
                .deadline
                .saturating_duration_since(now)
                .as_secs_f64();
        let length = self.round.phase_length.as_secs() as i64;
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(&pickle::to_bytes(&crate::pickle!((
            (period),
            (end),
            (length),
            (extra)
        ))))?;
        let mut avatar = base.entity(avatar)?;
        if matches!(self.round.phase, RoundPhase::Finished) {
            avatar.onRoundFinished(winner, reason)?;
        }
        avatar.updateArena(ARENA_PERIOD, Bytes(encoder.finish()?))?;
        Ok(())
    }
}

impl HostedRoom {
    fn set_team_ready(&mut self, team: u8, ready: bool) {
        match team {
            1 => self.teams_ready.0 = ready,
            2 => self.teams_ready.1 = ready,
            _ => {}
        }
    }

    pub(super) fn clear_team_readiness(&mut self) {
        self.teams_ready = (false, false);
    }
}

impl Gateway {
    #[tracing::instrument(name = "player_sessions.start_training_battle", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.request_id = request_id, input.team = args[0], input.force = args[1], input.ready = ready, output.room.id = tracing::field::Empty, output.arena_type_id = tracing::field::Empty, output.players.count = tracing::field::Empty, output.outcome = tracing::field::Empty, otel.status_code = tracing::field::Empty), err)]
    pub(in crate::player_sessions::infrastructure::gateway) fn change_training_team_ready(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
        request_id: i16,
        args: [i64; 3],
        ready: bool,
    ) -> Result<()> {
        let span = tracing::Span::current();
        let reject = |base: &mut base::App, outcome, message: &str| -> Result<()> {
            span.record("output.outcome", outcome);
            base.entity(entity)?
                .onCmdResponse(request_id, FAILURE, message.to_owned())?;
            Ok(())
        };
        let team = match (ready, args) {
            (true, [team @ (1 | 2), 0 | 1, 0]) | (false, [team @ (1 | 2), 0, 0]) => team as u8,
            _ => return reject(&mut self.base, "invalid_arguments", "Invalid training team"),
        };
        let Some(room) = self
            .sessions
            .get(&client)
            .and_then(|session| session.training_room)
            .and_then(|id| self.training_rooms.get_mut(&id))
        else {
            return reject(
                &mut self.base,
                "room_unavailable",
                "Training Room is unavailable",
            );
        };
        let room_id = room.id;
        span.record("output.room.id", room_id);
        span.record("output.arena_type_id", room.settings.arena_type_id);
        if room.host != client {
            return reject(
                &mut self.base,
                "not_host",
                "Only the Training Room host can start the battle",
            );
        }
        if !ready {
            room.set_team_ready(team, false);
            self.base
                .entity(entity)?
                .onCmdResponse(request_id, SUCCESS, String::new())?;
            span.record("output.outcome", "team_unlocked");
            return Ok(());
        }
        let has_team = |wanted| {
            room.members.values().any(|p| {
                team_of(p.roster) == Some(wanted)
                    && p.ready_inventory
                        .is_some_and(|id| id != OBSERVER_INVENTORY_ID)
            })
        };
        if !has_team(team)
            || room
                .members
                .values()
                .any(|p| team_of(p.roster) == Some(team) && p.ready_inventory.is_none())
        {
            return reject(
                &mut self.base,
                "team_not_ready",
                "Assign a ready player to this team and ready all assigned players",
            );
        }
        room.set_team_ready(team, true);
        if room.teams_ready != (true, true) {
            self.base
                .entity(entity)?
                .onCmdResponse(request_id, SUCCESS, String::new())?;
            span.record("output.outcome", "team_ready");
            return Ok(());
        }
        let battle = match self.prepare_training_battle(room_id) {
            Ok(Some(battle)) => battle,
            Ok(None) => {
                self.unlock_training_teams(room_id);
                return reject(
                    &mut self.base,
                    "roster_or_map_unavailable",
                    "The map or a team's assigned vehicles are unavailable",
                );
            }
            Err(error) => {
                self.unlock_training_teams(room_id);
                reject(
                    &mut self.base,
                    "preparation_failed",
                    "The training arena could not be prepared",
                )?;
                span.record("otel.status_code", "ERROR");
                return Err(error);
            }
        };
        span.record("output.players.count", battle.players.len());
        if let Err(error) = self
            .base
            .entity(entity)
            .and_then(|mut entity| entity.onCmdResponse(request_id, SUCCESS, String::new()))
        {
            self.unlock_training_teams(room_id);
            return Err(error.into());
        }
        self.launch_training_battle(room_id, battle);
        span.record("output.outcome", "launched");
        Ok(())
    }

    fn unlock_training_teams(&mut self, room_id: u32) {
        if let Some(room) = self.training_rooms.get_mut(&room_id) {
            room.clear_team_readiness();
        }
    }

    #[tracing::instrument(name = "player_sessions.prepare_training_battle", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.room.id = room_id, output.players.count = tracing::field::Empty), err)]
    fn prepare_training_battle(&mut self, room_id: u32) -> Result<Option<TrainingBattle>> {
        let Some(room) = self.training_rooms.get(&room_id) else {
            return Ok(None);
        };
        let arena_type = room.settings.arena_type_id;
        let Some(Some(map)) = self.training_arenas.get(&arena_type) else {
            return Ok(None);
        };
        let mut members = room.members.iter().collect::<Vec<_>>();
        members.sort_by_key(|(_, player)| player.entity.id().0);
        let mut entrants = Vec::new();
        let mut counts = [0usize; 2];
        let mut combatants = [0usize; 2];
        for (&client, member) in members {
            let Some(team) = team_of(member.roster) else {
                continue;
            };
            let Some(inventory_id) = member.ready_inventory else {
                return Ok(None);
            };
            let Some(session) = self.sessions.get(&client).filter(|session| {
                session.entity == Some(member.entity) && !session.saving && !session.crew_running
            }) else {
                return Ok(None);
            };
            let side = usize::from(team - 1);
            let vehicle = if inventory_id == OBSERVER_INVENTORY_ID {
                self.catalog
                    .training_observer_descriptor()
                    .map(|descriptor| TrainingVehicle {
                        descriptor: descriptor.to_vec(),
                        health: 1,
                        crew_size: 0,
                        ammunition: Vec::new(),
                    })
            } else {
                combatants[side] += 1;
                self.catalog
                    .training_vehicle(&session.account.snapshot, inventory_id)?
            };
            let Some(vehicle) = vehicle else {
                return Ok(None);
            };
            let [first, second] = map.spawns;
            let (from, to) = if team == 1 {
                (first, second)
            } else {
                (second, first)
            };
            let forward = (to - from).normalize_or_zero();
            let across = Vec2::new(forward.y, -forward.x);
            let position = from + across * (counts[side] as f32 * 5.0);
            counts[side] += 1;
            entrants.push((
                client,
                Entrant {
                    account_id: member.account_id,
                    name: member.name.clone(),
                    team,
                    is_host: room.host == client,
                    observer: inventory_id == OBSERVER_INVENTORY_ID,
                    vehicle,
                    avatar_id: self.base.arena_entity_id(),
                    vehicle_id: self.base.arena_entity_id(),
                    info_id: self.base.arena_entity_id(),
                    position: Vec3::new(position.x, 0.0, position.y),
                    direction: Vec3::new(0.0, 0.0, forward.x.atan2(forward.y)),
                },
            ));
        }
        if combatants.contains(&0) || !entrants.iter().any(|(client, _)| *client == room.host) {
            return Ok(None);
        }
        let roster = entrants
            .iter()
            .map(|(_, entrant)| entrant)
            .collect::<Vec<_>>();
        let dispatch = self.base.dispatch();
        let wire = self
            .training_wire
            .as_ref()
            .context("Missing training arena schema")?;
        let players = entrants
            .iter()
            .map(|(client, player)| {
                Ok((
                    *client,
                    Participant {
                        vehicle: wire.vehicle(dispatch, player)?,
                        avatar_id: player.avatar_id,
                        team: player.team,
                        entry: wire.entry(dispatch, room_id, arena_type, player, &roster)?,
                        avatar: None,
                        ready: false,
                        tank: TrainingTank::new(
                            player.position,
                            player.direction.z,
                            player.vehicle.health,
                            player.vehicle.ammunition.clone(),
                        ),
                        observer: player.observer,
                        last_gun_angles: 0x8020,
                        pending_drive: None,
                        pending_aim: None,
                    },
                ))
            })
            .collect::<Result<HashMap<_, _>>>()?;
        tracing::Span::current().record("output.players.count", players.len());
        let now = Instant::now();
        Ok(Some(TrainingBattle {
            map: map.clone(),
            arena_type,
            players,
            started_at: now,
            round: TrainingRound::new(
                now,
                Duration::from_secs(u64::try_from(room.settings.duration_seconds)?),
            ),
            launch: Some(tracing::Span::current()),
            shot_sequence: 0,
            result: None,
        }))
    }

    fn launch_training_battle(&mut self, room_id: u32, battle: TrainingBattle) {
        let Some(room) = self.training_rooms.remove(&room_id) else {
            return;
        };
        // The host checks its own team state before sending the second confirmation.
        // Publish both only when both commands have arrived.
        room.broadcast(
            &mut self.base,
            TEAM_STATES,
            Pickle(crate::pickle!((2, 2))),
            None,
        );
        for guest in room.invites {
            if let Some(session) = self.sessions.get(&guest)
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
        for (client, member) in room.members {
            let entering = battle.players.contains_key(&client);
            let Some(session) = self.sessions.get_mut(&client) else {
                continue;
            };
            session.training_room = None;
            if entering {
                session.training_battle = Some(room_id);
                session.entity = None;
                session.garage_synced = false;
                session.store_token = None;
                let result = (|| -> Result<()> {
                    chat::update_membership(&mut self.base, member.entity, room_id, false)?;
                    self.base.entity(member.entity)?.onArenaCreated()?;
                    self.base.reset_player(client)?;
                    Ok(())
                })();
                if let Err(error) = result {
                    error!(%error, room_id, "Training arena handoff failed");
                    self.base.disconnect_client(client);
                }
            } else {
                deliver_training_update(&mut self.base, client, room_id, |base| {
                    notify_training_left(base, member.entity, room_id)
                });
            }
        }
        self.training_battles.insert(room_id, battle);
    }

    pub(in crate::player_sessions::infrastructure::gateway) fn enter_training_battle(
        &mut self,
        client: ClientKey,
        battle_id: u32,
    ) -> Result<()> {
        let Some(battle) = self.training_battles.get_mut(&battle_id) else {
            return Ok(());
        };
        let Some(player) = battle
            .players
            .get(&client)
            .filter(|player| player.avatar.is_none())
        else {
            return Ok(());
        };
        let span = tracing::info_span!(
            parent: battle.launch.as_ref().unwrap_or(&tracing::Span::none()),
            "player_sessions.enter_training_battle",
            "ddd.bounded_context.name" = "player_sessions",
            "input.room.id" = battle_id,
            "input.arena_type_id" = battle.arena_type,
            "input.team" = player.team,
            "input.entity_id" = player.avatar_id,
            "output.players.count" = battle.players.len(),
            "output.outcome" = tracing::field::Empty,
            "otel.status_code" = tracing::field::Empty,
        );
        let result = span.in_scope(|| -> Result<_> {
            let avatar = self.base.create_base_player_with_id::<Avatar>(
                client,
                player.avatar_id,
                player.entry.base_properties.clone(),
            )?;
            player.entry.publish(
                &mut self.base,
                client,
                &battle.map,
                battle.players.values().map(|other| other.vehicle.clone()),
                battle.ticks(Instant::now()),
            )?;
            battle.send_period(&mut self.base, avatar, Instant::now())?;
            self.base
                .enable_arena_cell(client, player.vehicle.entity_id);
            Ok(avatar)
        });
        span.in_scope(|| match result {
            Ok(avatar) => {
                if let Some(player) = battle.players.get_mut(&client) {
                    player.avatar = Some(avatar);
                }
                if battle
                    .players
                    .values()
                    .all(|player| player.avatar.is_some())
                {
                    battle.launch = None;
                }
                span.record("output.outcome", "entered");
                Ok(())
            }
            Err(error) => {
                span.record("output.outcome", "delivery_failed");
                span.record("otel.status_code", "ERROR");
                self.base.disconnect_client(client);
                Err(error)
            }
        })
    }

    pub(in crate::player_sessions::infrastructure::gateway) fn handle_training_avatar(
        &mut self,
        battle_id: u32,
        event: base::BaseMethodEvent,
    ) {
        use crate::player_sessions::mercury::app::entity::MethodDecodeError;
        let span = event.span;
        span.in_scope(|| {
            if let Some(session) = self.sessions.get(&event.client) {
                span.record("user.id", session.account.snapshot.id().to_string());
            }
            let Some(battle) = self.training_battles.get_mut(&battle_id) else {
                span.record("output.outcome", "arena_unavailable");
                return;
            };
            let Some(player) = battle.players.get_mut(&event.client).filter(|player| {
                player
                    .avatar
                    .is_some_and(|avatar| avatar.any() == event.entity)
            }) else {
                span.record("output.outcome", "entity_unavailable");
                return;
            };
            let result = match self.base.decode::<AvatarBase>(event.entity, &event.call) {
                Ok((_, AvatarBase::SetClientReady {})) => {
                    let was_ready = player.ready;
                    player.ready = true;
                    span.record(
                        "output.outcome",
                        if was_ready { "already_ready" } else { "ready" },
                    );
                    if battle.players.values().all(|participant| participant.ready) {
                        let now = Instant::now();
                        if battle.round.advance(now, true) {
                            battle.publish_period(&mut self.base, battle_id, now);
                        }
                    }
                    Ok(())
                }
                Ok((_, AvatarBase::LeaveArena {})) => {
                    span.record("output.outcome", "left_arena");
                    self.return_from_training_battle(event.client)
                }
                Err(error) => {
                    let outcome = if matches!(error, MethodDecodeError::Unimplemented) {
                        "unimplemented"
                    } else {
                        "decode_failed"
                    };
                    span.record("output.outcome", outcome);
                    span.record("otel.status_code", "ERROR");
                    error!(outcome, "Avatar base method could not be dispatched");
                    Ok(())
                }
            };
            if let Err(error) = result {
                span.record("otel.status_code", "ERROR");
                span.record("output.outcome", "return_failed");
                error!(%error, "Could not return to the garage");
                self.base.disconnect_client(event.client);
            }
        });
    }

    pub(in crate::player_sessions::infrastructure::gateway) fn tick_training_battles(
        &mut self,
        now: Instant,
    ) {
        let mut ending = Vec::new();
        for (&battle_id, battle) in &mut self.training_battles {
            if !battle.has_both_teams() || battle.round.ended(now) {
                ending.extend(battle.players.keys().copied());
                continue;
            }
            if battle.round.advance(now, false) {
                battle.publish_period(&mut self.base, battle_id, now);
            }
            if matches!(battle.round.phase, RoundPhase::Battle)
                && let Some(wire) = &self.training_wire
            {
                battle.advance_vehicles(&mut self.base, wire);
            }
            for (&client, player) in &battle.players {
                if player.avatar.is_some() {
                    if let Err(error) = self.base.arena_tick(client, battle.ticks(now) as u8) {
                        error!(%error, battle_id, "Could not send training battle tick");
                        self.base.disconnect_client(client);
                    }
                } else if now >= battle.started_at + LOAD_TIMEOUT {
                    self.base.disconnect_client(client);
                }
            }
        }
        for client in ending {
            if let Err(error) = self.return_from_training_battle(client) {
                error!(%error, "Could not return from training battle");
                self.base.disconnect_client(client);
            }
        }
    }

    #[tracing::instrument(name = "player_sessions.return_from_training_battle", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.client.address = %client, output.room.id = tracing::field::Empty, output.outcome = tracing::field::Empty), err)]
    fn return_from_training_battle(&mut self, client: ClientKey) -> Result<()> {
        if let Some((id, entered)) = self.remove_training_participant(client) {
            tracing::Span::current().record("output.room.id", id);
            if entered {
                self.base.reset_player(client)?;
                tracing::Span::current().record("output.outcome", "garage_reset");
            } else {
                self.base.disconnect_client(client);
                tracing::Span::current().record("output.outcome", "handoff_aborted");
            }
        }
        Ok(())
    }

    #[tracing::instrument(name = "player_sessions.remove_training_participant", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.client.address = %client, output.room.id = tracing::field::Empty, output.players.count = tracing::field::Empty, otel.status_code = tracing::field::Empty))]
    pub(in crate::player_sessions::infrastructure::gateway) fn remove_training_participant(
        &mut self,
        client: ClientKey,
    ) -> Option<(u32, bool)> {
        let id = self.sessions.get_mut(&client)?.training_battle.take()?;
        let battle = self.training_battles.get_mut(&id)?;
        let removed = battle.players.remove(&client)?;
        let span = tracing::Span::current();
        span.record("output.room.id", id);
        span.record("output.players.count", battle.players.len());
        if battle.players.is_empty() {
            self.training_battles.remove(&id);
        } else {
            for (&peer, player) in &battle.players {
                if player.avatar.is_some()
                    && let Err(error) = self.base.send_arena_element(
                        peer,
                        LeaveArenaEntity {
                            entity_id: removed.vehicle.entity_id,
                        },
                    )
                {
                    span.record("otel.status_code", "ERROR");
                    error!(%error, "Could not remove departed training vehicle");
                }
            }
            if battle.players.values().all(|p| p.avatar.is_some()) {
                battle.launch = None;
            }
        }
        Some((id, removed.avatar.is_some()))
    }
}

fn team_of(roster: RosterSlot) -> Option<u8> {
    match roster {
        RosterSlot::TeamOne => Some(1),
        RosterSlot::TeamTwo => Some(2),
        RosterSlot::UnassignedFromTeamOne | RosterSlot::UnassignedFromTeamTwo => None,
    }
}
