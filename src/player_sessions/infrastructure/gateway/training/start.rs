//! Translate a locked Training Room roster into a battle launch.

use super::*;
use crate::battles::contract::{
    BattleLaunch, BattleMode, BattleParticipant, PreparedBattle, VehicleLoadout,
};

pub(in crate::player_sessions::infrastructure::gateway) const TEAM_READY: i16 = 411;
pub(in crate::player_sessions::infrastructure::gateway) const TEAM_NOT_READY: i16 = 412;
const TEAM_STATES: u8 = 6;

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
        span.record("output.players.count", battle.participant_count());
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
    fn prepare_training_battle(&mut self, room_id: u32) -> Result<Option<PreparedBattle>> {
        let Some(room) = self.training_rooms.get(&room_id) else {
            return Ok(None);
        };
        let mut members = room.members.iter().collect::<Vec<_>>();
        members.sort_by_key(|(_, player)| player.entity.id().0);
        let mut entrants = Vec::new();
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
            let observer = inventory_id == OBSERVER_INVENTORY_ID;
            let vehicle = if observer {
                self.catalog
                    .training_observer_descriptor()
                    .map(|descriptor| VehicleLoadout {
                        descriptor: descriptor.to_vec(),
                        health: 1,
                        crew_size: 0,
                        ammunition: Vec::new(),
                        performance: None,
                    })
            } else {
                self.catalog
                    .battle_vehicle(&session.account.snapshot, inventory_id)?
                    .map(|vehicle| VehicleLoadout {
                        descriptor: vehicle.descriptor,
                        health: vehicle.health,
                        crew_size: vehicle.crew_size,
                        ammunition: vehicle.ammunition,
                        performance: Some(vehicle.performance),
                    })
            };
            let Some(vehicle) = vehicle else {
                return Ok(None);
            };
            entrants.push(BattleParticipant {
                client,
                account_id: member.account_id,
                name: member.name.clone(),
                team,
                is_host: room.host == client,
                observer,
                vehicle,
            });
        }
        if !entrants.iter().any(|entrant| entrant.is_host) {
            return Ok(None);
        }
        let prepared = self.battles.prepare(
            &mut self.base,
            BattleLaunch {
                arena_type: room.settings.arena_type_id,
                duration: Duration::from_secs(u64::try_from(room.settings.duration_seconds)?),
                mode: BattleMode::Training,
                participants: entrants,
            },
        )?;
        if let Some(battle) = &prepared {
            tracing::Span::current().record("output.players.count", battle.participant_count());
        }
        Ok(prepared)
    }

    fn launch_training_battle(&mut self, room_id: u32, battle: PreparedBattle) {
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
            let entering = battle.contains(client);
            let Some(session) = self.sessions.get_mut(&client) else {
                continue;
            };
            session.training_room = None;
            if entering {
                session.entity = None;
                session.garage_synced = false;
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
        self.battles.start(battle);
    }
}

fn team_of(roster: RosterSlot) -> Option<u8> {
    match roster {
        RosterSlot::TeamOne => Some(1),
        RosterSlot::TeamTwo => Some(2),
        RosterSlot::UnassignedFromTeamOne | RosterSlot::UnassignedFromTeamTwo => None,
    }
}
