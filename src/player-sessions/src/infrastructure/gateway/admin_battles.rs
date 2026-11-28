use super::{battles::vehicle_loadout, *};
use crate::application::admin_battles::{
    AvailablePlayer, BattleChoice, BattleLaunchReceipt, BattleOptions, BattleRequest,
    TrainingBattleError, TrainingBattlePlayer, TrainingBattleSetup,
};
use tokio::sync::oneshot;
use wot_battles::contract::{
    BattleLaunch, BattleMode, BattleParameters, BattleParticipant, BattlePreparationId,
    PreparedBattle,
};
use wot_training_rooms::contract::TrainingRoomSettings;

const MAX_ENTRANTS: usize = 30;
const MAX_PENDING: usize = 4;

pub(super) struct PendingAdminBattle {
    players: Vec<SelectedPlayer>,
    reply: oneshot::Sender<Result<BattleLaunchReceipt, TrainingBattleError>>,
    span: tracing::Span,
}

struct SelectedPlayer {
    client: ClientKey,
    entity: EntityKey<Account>,
    generation: u64,
    revision: i64,
}

impl SelectedPlayer {
    fn matches(&self, gateway: &Gateway) -> bool {
        gateway.sessions.get(&self.client).is_some_and(|session| {
            gateway.admin_battle_unavailable_reason(session).is_none()
                && session.generation == self.generation
                && session.entity == Some(self.entity)
                && session.account.snapshot.revision() == self.revision
        })
    }
}

impl Gateway {
    pub(super) fn handle_admin_battle_request(&mut self, request: BattleRequest) {
        match request {
            BattleRequest::Options(span, reply) => span.in_scope(|| {
                if !reply.is_closed() {
                    let _ = reply.send(self.admin_battle_options());
                }
            }),
            BattleRequest::OwnedLoadout(span, account_id, inventory_id, reply) => {
                span.in_scope(|| {
                    if !reply.is_closed() {
                        let result =
                            self.selected_admin_session(account_id)
                                .and_then(|(_, session)| {
                                    self.catalog
                                        .owned_battle_loadout_options(
                                            &session.account.snapshot,
                                            inventory_id,
                                        )
                                        .map_err(projection_failed)?
                                        .map_err(TrainingBattleError::VehicleUnavailable)
                                });
                        let _ = reply.send(result);
                    }
                })
            }
            BattleRequest::Launch(span, setup, reply) => span.in_scope(|| {
                if reply.is_closed() {
                    return;
                }
                let launch = tracing::info_span!(
                    "player_sessions.start_admin_battle",
                    "ddd.bounded_context.name" = "player_sessions",
                    "input.arena_type_id" = setup.arena_type,
                    "input.round_length.seconds" = setup.duration_seconds,
                    "input.players.count" = setup.players.len(),
                    "input.dummies.count" = setup.dummies.len(),
                    "input.players.custom_loadout_count" =
                        setup.players.iter().filter(|p| p.loadout.is_some()).count(),
                    "input.players.owned_override_count" = setup
                        .players
                        .iter()
                        .filter(|p| p.owned_overrides.is_some())
                        .count(),
                    "input.battle.parameters.count" = setup.parameters.len(),
                    "output.battle.id" = tracing::field::Empty,
                    "output.players.handoff_count" = tracing::field::Empty,
                    "output.outcome" = tracing::field::Empty,
                    "otel.status_code" = tracing::field::Empty
                );
                let result = launch.in_scope(|| self.queue_admin_battle(setup));
                match result {
                    Ok((id, players)) => {
                        self.pending_admin_battles.insert(
                            id,
                            PendingAdminBattle {
                                players,
                                reply,
                                span: launch,
                            },
                        );
                    }
                    Err(error) => {
                        launch.in_scope(|| record_launch_failure(error));
                        let _ = reply.send(Err(error));
                    }
                }
            }),
        }
    }

    fn admin_battle_unavailable_reason(&self, session: &Session) -> Option<&'static str> {
        if self
            .battle_participants
            .contains(&ParticipantId(session.generation))
        {
            return Some("In battle");
        }
        if session.training_room.is_some() {
            return Some("In a Training Room");
        }
        if self.pending_admin_battles.values().any(|pending| {
            pending
                .players
                .iter()
                .any(|player| player.generation == session.generation)
        }) {
            return Some("Battle is being prepared");
        }
        if session.entity.is_none() || !session.garage_synced {
            return Some("Garage is loading");
        }
        if session.saving
            || session.crew_running
            || self.pending_writes.contains(&session.account.snapshot.id())
        {
            return Some("Garage update in progress");
        }
        None
    }

    #[tracing::instrument(name = "player_sessions.admin_battle_options", skip_all, fields(
        ddd.bounded_context.name = "player_sessions", input.sessions.count = self.sessions.len(),
        output.maps.count = tracing::field::Empty, output.players.count = tracing::field::Empty,
    ))]
    fn admin_battle_options(&self) -> BattleOptions {
        let mut maps: Vec<_> = self
            .battle_worker
            .maps()
            .map(|(id, name)| BattleChoice {
                id,
                name: name.into(),
            })
            .collect();
        maps.sort_unstable_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        let mut players: Vec<_> = self
            .sessions
            .values()
            .map(|session| {
                let account = &session.account.snapshot;
                let mut vehicles = self.catalog.battle_vehicle_choices(account);
                vehicles.sort_unstable_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
                AvailablePlayer {
                    id: account.id(),
                    name: account.display_name().into(),
                    unavailable_reason: self.admin_battle_unavailable_reason(session),
                    vehicles,
                }
            })
            .collect();
        players.sort_unstable_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        let span = tracing::Span::current();
        span.record("output.maps.count", maps.len());
        span.record("output.players.count", players.len());
        BattleOptions {
            maps,
            players,
            observer_available: self.catalog.training_observer_descriptor().is_some(),
            parameters: BattleParameters::definitions(),
        }
    }

    fn queue_admin_battle(
        &mut self,
        setup: TrainingBattleSetup,
    ) -> Result<(u32, Vec<SelectedPlayer>), TrainingBattleError> {
        use TrainingBattleError::*;
        if setup.players.iter().any(|player| {
            (player.inventory_id.is_some() && player.loadout.is_some())
                || (player.inventory_id.is_none() && player.owned_overrides.is_some())
        }) {
            return Err(InvalidPlayerLoadout);
        }
        let parameters =
            BattleParameters::from_overrides(&setup.parameters).map_err(|_| InvalidParameters)?;
        if !self
            .battle_worker
            .maps()
            .any(|(id, _)| id == setup.arena_type)
            || !TrainingRoomSettings::accepts_duration(setup.duration_seconds)
            || !valid_roster(&setup)
        {
            return Err(InvalidSetup);
        }
        parameters
            .validate_round_length(setup.duration_seconds as u64)
            .map_err(|_| InvalidParameters)?;
        if self.pending_admin_battles.len() >= MAX_PENDING
            || self.battle_participants.len() + setup.players.len() > self.config.max_clients
        {
            return Err(Unavailable);
        }
        let mut participants = Vec::new();
        let mut players = Vec::new();
        for player in setup.players {
            let (selected, participant) = self.select_admin_battle_player(player)?;
            players.push(selected);
            participants.push(participant);
        }
        let mut ids = (0..participants.len())
            .map(|_| self.reserve_battle_entity_ids())
            .collect::<Vec<_>>();
        for (index, dummy) in setup.dummies.into_iter().enumerate() {
            let vehicle = self
                .catalog
                .custom_battle_vehicle(&dummy.loadout, None)
                .map_err(projection_failed)?
                .ok_or(InvalidDummy)?;
            let entities = self.reserve_battle_entity_ids();
            participants.push(BattleParticipant {
                id: ParticipantId::stationary_dummy(entities.vehicle),
                account_id: 0,
                name: format!("Dummy {}", index + 1),
                team: dummy.team,
                is_host: false,
                observer: false,
                stationary_dummy: true,
                customization_display_type: 2,
                vehicle: vehicle_loadout(vehicle),
            });
            ids.push(entities);
        }
        let id = self
            .last_admin_battle_request
            .checked_add(1)
            .ok_or(Unavailable)?;
        self.battle_worker
            .prepare(
                BattlePreparationId::Admin(id),
                BattleLaunch {
                    arena_type: setup.arena_type,
                    duration: Duration::from_secs(
                        u64::try_from(setup.duration_seconds).map_err(|_| InvalidSetup)?,
                    ),
                    mode: BattleMode::Training,
                    parameters,
                    participants,
                },
                ids,
            )
            .map_err(|_| Unavailable)?;
        self.last_admin_battle_request = id;
        Ok((id, players))
    }

    fn selected_admin_session(
        &self,
        account_id: i64,
    ) -> Result<(ClientKey, &Session), TrainingBattleError> {
        self.sessions
            .iter()
            .find(|(_, session)| {
                session.account.snapshot.id() == account_id
                    && self.admin_battle_unavailable_reason(session).is_none()
            })
            .map(|(&client, session)| (client, session))
            .ok_or(TrainingBattleError::PlayerUnavailable)
    }

    fn select_admin_battle_player(
        &self,
        player: TrainingBattlePlayer,
    ) -> Result<(SelectedPlayer, BattleParticipant), TrainingBattleError> {
        use TrainingBattleError::PlayerUnavailable;
        let (client, session) = self.selected_admin_session(player.account_id)?;
        let entity = session.entity.ok_or(PlayerUnavailable)?;
        let account = &session.account.snapshot;
        let (vehicle, observer) = if let Some(loadout) = &player.loadout {
            let custom = self
                .catalog
                .custom_battle_vehicle(loadout, Some(account.id()))
                .map_err(projection_failed)?
                .ok_or(TrainingBattleError::InvalidPlayerLoadout)?;
            (vehicle_loadout(custom), false)
        } else if let Some(id) = player.inventory_id {
            let owned = self
                .catalog
                .admin_battle_vehicle(account, id, player.owned_overrides.as_ref())
                .map_err(projection_failed)?
                .map_err(TrainingBattleError::VehicleUnavailable)?;
            (vehicle_loadout(owned), false)
        } else {
            (
                self.observer_battle_loadout().ok_or(PlayerUnavailable)?,
                true,
            )
        };
        Ok((
            SelectedPlayer {
                client,
                entity,
                generation: session.generation,
                revision: account.revision(),
            },
            BattleParticipant {
                id: ParticipantId(session.generation),
                account_id: account.id(),
                name: account.display_name().to_owned(),
                team: player.team,
                is_host: false,
                observer,
                stationary_dummy: false,
                customization_display_type: account.customization_display_type(),
                vehicle,
            },
        ))
    }

    pub(super) fn complete_admin_battle(
        &mut self,
        id: u32,
        result: Result<Option<Box<PreparedBattle>>>,
    ) {
        let Some(pending) = self.pending_admin_battles.remove(&id) else {
            return;
        };
        pending.span.in_scope(|| {
            let result = self.activate_admin_battle(&pending.players, result);
            match &result {
                Ok(receipt) => {
                    pending.span.record(
                        "output.outcome",
                        if receipt.players_handed_off < pending.players.len() {
                            "launched_partial"
                        } else {
                            "launched"
                        },
                    );
                }
                Err(error) => record_launch_failure(*error),
            };
            let _ = pending.reply.send(result);
        });
    }

    fn activate_admin_battle(
        &mut self,
        players: &[SelectedPlayer],
        result: Result<Option<Box<PreparedBattle>>>,
    ) -> Result<BattleLaunchReceipt, TrainingBattleError> {
        use TrainingBattleError::*;
        let span = tracing::Span::current();
        let mut battle = result
            .map_err(projection_failed)?
            .ok_or(PreparationFailed)?;
        if !players.iter().all(|player| player.matches(self)) {
            return Err(PlayerUnavailable);
        }
        let start = self
            .battle_worker
            .reserve_start()
            .map_err(|_| Unavailable)?;
        let mut handed_off = 0;
        for player in players {
            let result = (|| -> Result<()> {
                self.base.entity(player.entity)?.onArenaCreated()?;
                self.base.reset_player(player.client)?;
                Ok(())
            })();
            if let Err(error) = result {
                error!(%error, "Admin battle handoff failed");
                span.record("otel.status_code", "ERROR");
                self.base.disconnect_client(player.client);
                continue;
            }
            if let Some(session) = self.sessions.get_mut(&player.client) {
                session.entity = None;
                session.garage_synced = false;
            }
            self.battle_participants
                .insert(ParticipantId(player.generation));
            handed_off += 1;
        }
        span.record("output.players.handoff_count", handed_off);
        if handed_off == 0 {
            return Err(PreparationFailed);
        }
        battle.assign_scenery_entities(|| self.base.arena_entity_id());
        let id = battle.id();
        span.record("output.battle.id", id);
        start.start(*battle);
        // The simulation needs an explicit departure if the gateway could not hand off a seat.
        for player in players {
            let participant = ParticipantId(player.generation);
            if !self.battle_participants.contains(&participant) {
                self.battle_departures
                    .push_back((participant, span.clone()));
            }
        }
        Ok(BattleLaunchReceipt {
            battle_id: id,
            players_handed_off: handed_off,
        })
    }
}

fn valid_roster(setup: &TrainingBattleSetup) -> bool {
    let mut accounts = HashSet::new();
    let valid_team = |team| matches!(team, 1 | 2);
    !setup.players.is_empty()
        && setup.players.len() + setup.dummies.len() <= MAX_ENTRANTS
        && setup
            .players
            .iter()
            .all(|player| valid_team(player.team) && accounts.insert(player.account_id))
        && setup.dummies.iter().all(|dummy| valid_team(dummy.team))
        && [1, 2].into_iter().all(|team| {
            setup.players.iter().any(|player| {
                player.team == team && (player.inventory_id.is_some() || player.loadout.is_some())
            }) || setup.dummies.iter().any(|dummy| dummy.team == team)
        })
}

fn projection_failed(error: anyhow::Error) -> TrainingBattleError {
    tracing::Span::current().record("otel.status_code", "ERROR");
    error!(%error, "Could not prepare admin battle");
    TrainingBattleError::PreparationFailed
}

fn record_launch_failure(error: TrainingBattleError) {
    let span = tracing::Span::current();
    span.record("output.outcome", error.outcome());
    if matches!(
        error,
        TrainingBattleError::Unavailable | TrainingBattleError::PreparationFailed
    ) {
        span.record("otel.status_code", "ERROR");
        error!(outcome = error.outcome(), "Admin battle launch failed");
    }
}
