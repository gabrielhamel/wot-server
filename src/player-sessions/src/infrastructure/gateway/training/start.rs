//! Translate a locked Training Room roster into a battle launch.

use super::*;
use crate::infrastructure::gateway::battles::vehicle_loadout;
use wot_battles::contract::{
    BattleLaunch, BattleMode, BattleParticipant, BattlePreparationId, BattleQueueError,
    BattleStart, PreparedBattle,
};

pub(in crate::infrastructure::gateway) const TEAM_READY: i16 = 411;
pub(in crate::infrastructure::gateway) const TEAM_NOT_READY: i16 = 412;
const TEAM_STATES: u8 = 6;

pub(in crate::infrastructure::gateway) struct PendingTrainingBattle {
    client: ClientKey,
    entity: EntityKey<Account>,
    request_id: i16,
    roster: TrainingLaunchRoster,
    launch: tracing::Span,
}

#[derive(PartialEq, Eq)]
struct TrainingLaunchRoster {
    arena_type: i32,
    duration: Duration,
    entrants: Vec<TrainingEntrant>,
}

#[derive(PartialEq, Eq)]
struct TrainingEntrant {
    client: ClientKey,
    participant: ParticipantId,
    entity_id: u32,
    inventory_id: i64,
    team: u8,
    revision: i64,
}

impl Gateway {
    #[tracing::instrument(name = "player_sessions.start_training_battle", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.request_id = request_id, input.team = args[0], input.force = args[1], input.ready = ready, output.room.id = tracing::field::Empty, output.arena_type_id = tracing::field::Empty, output.players.count = tracing::field::Empty, output.outcome = tracing::field::Empty, otel.status_code = tracing::field::Empty), err)]
    pub(in crate::infrastructure::gateway) fn change_training_team_ready(
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
        let room_id = room.id();
        span.record("output.room.id", room_id);
        span.record("output.arena_type_id", room.settings().arena_type_id);
        if room.host() != client {
            return reject(
                &mut self.base,
                "not_host",
                "Only the Training Room host can start the battle",
            );
        }
        if room.members().values().any(|member| member.in_battle()) {
            return reject(
                &mut self.base,
                "battle_in_progress",
                "The previous battle is in progress",
            );
        }
        if !ready {
            room.set_team_ready(team, false, OBSERVER_INVENTORY_ID);
            self.base
                .entity(entity)?
                .onCmdResponse(request_id, SUCCESS, String::new())?;
            span.record("output.outcome", "team_unlocked");
            return Ok(());
        }
        if self.pending_battle_launches.contains_key(&room_id)
            || self
                .pending_battle_launches
                .values()
                .any(|pending| pending.client == client)
        {
            return reject(
                &mut self.base,
                "preparation_in_progress",
                "A training arena is already being prepared",
            );
        }
        if !room.set_team_ready(team, true, OBSERVER_INVENTORY_ID) {
            return reject(
                &mut self.base,
                "team_not_ready",
                "Assign a ready player to this team and ready all assigned players",
            );
        }
        if !room.teams_ready() {
            self.base
                .entity(entity)?
                .onCmdResponse(request_id, SUCCESS, String::new())?;
            span.record("output.outcome", "team_ready");
            return Ok(());
        }
        let Some(roster) = self.training_battle_roster(room_id) else {
            self.unlock_training_teams(room_id);
            return reject(
                &mut self.base,
                "roster_or_map_unavailable",
                "The map or a team's assigned vehicles are unavailable",
            );
        };
        if self.pending_battle_launches.len() >= self.config.max_clients {
            self.unlock_training_teams(room_id);
            return reject(
                &mut self.base,
                "preparation_in_progress",
                "The battle server is busy",
            );
        }
        match self.queue_training_preparation(room_id, &roster) {
            Ok(true) => {
                self.pending_battle_launches.insert(
                    room_id,
                    PendingTrainingBattle {
                        client,
                        entity,
                        request_id,
                        roster,
                        launch: span,
                    },
                );
            }
            Ok(false) => {
                self.unlock_training_teams(room_id);
                return reject(
                    &mut self.base,
                    "roster_or_map_unavailable",
                    "The map or a team's assigned vehicles are unavailable",
                );
            }
            Err(error) => {
                self.unlock_training_teams(room_id);
                span.record("otel.status_code", "ERROR");
                error!(%error, room_id, "Could not queue training arena preparation");
                if error.is::<BattleQueueError>() {
                    return reject(
                        &mut self.base,
                        "battle_queue_unavailable",
                        "The battle server is unavailable",
                    );
                }
                return reject(
                    &mut self.base,
                    "preparation_failed",
                    "The training arena could not be prepared",
                );
            }
        }
        Ok(())
    }

    pub(in crate::infrastructure::gateway) fn complete_training_preparation(
        &mut self,
        room_id: u32,
        result: Result<Option<Box<PreparedBattle>>>,
    ) {
        let Some(pending) = self.pending_battle_launches.remove(&room_id) else {
            return;
        };
        let launch = pending.launch.clone();
        launch.in_scope(|| {
            if let Err(error) = self.finish_training_preparation(room_id, pending, result) {
                launch.record("otel.status_code", "ERROR");
                error!(%error, room_id, "Could not complete training arena launch");
            }
        });
    }

    fn finish_training_preparation(
        &mut self,
        room_id: u32,
        pending: PendingTrainingBattle,
        result: Result<Option<Box<PreparedBattle>>>,
    ) -> Result<()> {
        let span = tracing::Span::current();
        let reject = |gateway: &mut Self, outcome, message: &str| -> Result<()> {
            gateway.unlock_training_teams(room_id);
            span.record("output.outcome", outcome);
            if gateway
                .sessions
                .get(&pending.client)
                .is_some_and(|session| session.entity == Some(pending.entity))
            {
                gateway.base.entity(pending.entity)?.onCmdResponse(
                    pending.request_id,
                    FAILURE,
                    message.to_owned(),
                )?;
            }
            Ok(())
        };
        let battle = match result {
            Ok(Some(battle)) => battle,
            Ok(None) => {
                return reject(
                    self,
                    "roster_or_map_unavailable",
                    "The map or a team's assigned vehicles are unavailable",
                );
            }
            Err(error) => {
                span.record("otel.status_code", "ERROR");
                error!(%error, room_id, "Training arena preparation failed");
                return reject(
                    self,
                    "preparation_failed",
                    "The training arena could not be prepared",
                );
            }
        };
        if self.training_battle_roster(room_id).as_ref() != Some(&pending.roster)
            || !self
                .training_rooms
                .get(&room_id)
                .is_some_and(|room| room.host() == pending.client)
        {
            return reject(
                self,
                "preparation_stale",
                "The roster or settings changed while the arena was being prepared",
            );
        }
        let start = match self.battle_worker.reserve_start() {
            Ok(start) => start,
            Err(error) => {
                span.record("otel.status_code", "ERROR");
                error!(%error, room_id, "Could not queue training arena activation");
                return reject(
                    self,
                    "battle_queue_unavailable",
                    "The battle server is unavailable",
                );
            }
        };
        span.record("output.players.count", battle.participant_count());
        if let Err(error) = self
            .base
            .entity(pending.entity)
            .and_then(|mut entity| entity.onCmdResponse(pending.request_id, SUCCESS, String::new()))
        {
            self.unlock_training_teams(room_id);
            return Err(error.into());
        }
        self.launch_training_battle(room_id, *battle, start);
        span.record("output.outcome", "launched");
        Ok(())
    }

    fn unlock_training_teams(&mut self, room_id: u32) {
        if let Some(room) = self.training_rooms.get_mut(&room_id) {
            room.clear_team_readiness();
        }
    }

    fn training_battle_roster(&self, room_id: u32) -> Option<TrainingLaunchRoster> {
        let room = self.training_rooms.get(&room_id)?;
        if !room.teams_ready() {
            return None;
        }
        let mut entrants = Vec::new();
        for (&client, member) in room.members() {
            let Some(team) = member.roster().assigned_team() else {
                continue;
            };
            let inventory_id = member.ready_inventory()?;
            let session = self.sessions.get(&client).filter(|session| {
                session.entity == Some(member.delivery.entity)
                    && !session.saving
                    && !session.crew_running
            })?;
            let participant = ParticipantId(session.generation);
            if member.in_battle() || self.battle_participants.contains(&participant) {
                return None;
            }
            entrants.push(TrainingEntrant {
                client,
                participant,
                entity_id: member.delivery.entity.id().0,
                inventory_id,
                team,
                revision: session.account.snapshot.revision(),
            });
        }
        if !entrants.iter().any(|entrant| entrant.client == room.host())
            || entrants.len() + self.battle_participants.len() > self.config.max_clients
        {
            return None;
        }
        entrants.sort_unstable_by_key(|entrant| entrant.entity_id);
        Some(TrainingLaunchRoster {
            arena_type: room.settings().arena_type_id,
            duration: Duration::from_secs(u64::try_from(room.settings().duration_seconds).ok()?),
            entrants,
        })
    }

    #[tracing::instrument(name = "player_sessions.prepare_training_battle", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.room.id = room_id, output.players.count = tracing::field::Empty), err)]
    fn queue_training_preparation(
        &mut self,
        room_id: u32,
        roster: &TrainingLaunchRoster,
    ) -> Result<bool> {
        let Some(room) = self.training_rooms.get(&room_id) else {
            return Ok(false);
        };
        let mut entrants = Vec::with_capacity(roster.entrants.len());
        for &TrainingEntrant {
            client,
            participant,
            inventory_id,
            team,
            ..
        } in &roster.entrants
        {
            let (Some(member), Some(session)) =
                (room.members().get(&client), self.sessions.get(&client))
            else {
                return Ok(false);
            };
            let observer = inventory_id == OBSERVER_INVENTORY_ID;
            let vehicle = if observer {
                self.observer_battle_loadout()
            } else {
                self.catalog
                    .battle_vehicle(&session.account.snapshot, inventory_id)?
                    .map(vehicle_loadout)
            };
            let Some(vehicle) = vehicle else {
                return Ok(false);
            };
            entrants.push(BattleParticipant {
                id: participant,
                account_id: member.account_id,
                name: member.name.clone(),
                team,
                is_host: room.host() == client,
                observer,
                stationary_dummy: false,
                customization_display_type: session.account.snapshot.customization_display_type(),
                vehicle,
            });
        }
        let entity_ids = entrants
            .iter()
            .map(|_| self.reserve_battle_entity_ids())
            .collect();
        tracing::Span::current().record("output.players.count", entrants.len());
        self.battle_worker.prepare(
            BattlePreparationId::TrainingRoom(room_id),
            BattleLaunch {
                parameters: Default::default(),
                arena_type: roster.arena_type,
                duration: roster.duration,
                mode: BattleMode::Training,
                participants: entrants,
            },
            entity_ids,
        )?;
        Ok(true)
    }

    fn launch_training_battle(
        &mut self,
        room_id: u32,
        mut battle: PreparedBattle,
        start: BattleStart,
    ) {
        let Some(room) = self.training_rooms.get_mut(&room_id) else {
            return;
        };
        room.broadcast(
            &mut self.base,
            TEAM_STATES,
            Pickle(wot_platform::pickle!((2, 2))),
            None,
        );
        room.clear_team_readiness();
        for (&client, member) in room.members_mut() {
            let Some(session) = self
                .sessions
                .get_mut(&client)
                .filter(|session| battle.contains(ParticipantId(session.generation)))
            else {
                member.begin_round(false);
                continue;
            };
            self.battle_participants
                .insert(ParticipantId(session.generation));
            // The client renders a vehicle descriptor for every in-battle roster entry.
            member.begin_round(true);
            session.entity = None;
            session.garage_synced = false;
            let result = (|| -> Result<()> {
                chat::update_membership(&mut self.base, member.delivery.entity, room_id, false)?;
                self.base.entity(member.delivery.entity)?.onArenaCreated()?;
                self.base.reset_player(client)?;
                Ok(())
            })();
            if let Err(error) = result {
                error!(%error, room_id, "Training arena handoff failed");
                self.base.disconnect_client(client);
            }
        }
        room.broadcast(
            &mut self.base,
            TEAM_STATES,
            Pickle(wot_platform::pickle!((1, 1))),
            None,
        );
        room.broadcast(&mut self.base, ROSTER, room.roster_snapshot(), None);
        battle.assign_scenery_entities(|| self.base.arena_entity_id());
        start.start(battle);
    }
}
