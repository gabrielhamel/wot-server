//! Routes typed battle work and delivers the worker outputs to live sessions.

use super::*;
use wot_battles::contract::{BattleOutput, BattleQueueError, ParticipantDeparture};

const DELIVERY_BUDGET: usize = 256;
const PREPARATION_BUDGET: usize = 4;

impl Gateway {
    pub(super) fn poll_battles(&mut self) {
        for _ in 0..DELIVERY_BUDGET {
            let Some(output) = self.battle_worker.try_output() else {
                break;
            };
            match output {
                BattleOutput::Deliver(delivery) => {
                    let participant = delivery.participant();
                    if self.battle_participants.contains(&participant)
                        && let Some(&client) = self.battle_clients.get(&participant)
                    {
                        let mut adapter = MercuryArena::new(&mut self.base, &self.battle_clients);
                        if delivery.deliver(&mut adapter).is_err() {
                            self.base.disconnect_client(client);
                        }
                    }
                }
                BattleOutput::Positions(positions) => {
                    let mut adapter = MercuryArena::new(&mut self.base, &self.battle_clients);
                    positions.deliver(&mut adapter);
                }
                BattleOutput::Return {
                    participant,
                    departure,
                    parent,
                } => {
                    if self.battle_participants.remove(&participant)
                        && let Some(&client) = self.battle_clients.get(&participant)
                    {
                        parent.in_scope(|| self.return_from_battle(client, departure));
                    }
                }
                BattleOutput::Tick { delay, duration } => {
                    if let Some(metrics) = &self.metrics {
                        metrics.record_battle_tick(delay, duration);
                    }
                }
            }
        }
        while let Some((participant, parent)) = self.battle_departures.front() {
            match parent.in_scope(|| self.battle_worker.disconnect(*participant)) {
                Ok(()) => {
                    self.battle_participants.remove(participant);
                    self.battle_departures.pop_front();
                }
                Err(BattleQueueError::Full | BattleQueueError::Closed) => break,
            }
        }
        for _ in 0..PREPARATION_BUDGET {
            let Some(prepared) = self.battle_worker.try_prepared() else {
                break;
            };
            self.complete_training_preparation(prepared.request.0, prepared.result);
        }
        let failed_task = if self.battle_worker.is_stopped() {
            Some("Battle simulation stopped")
        } else if self.battle_worker.is_preparation_stopped() {
            Some("Arena preparation stopped")
        } else {
            None
        };
        if let Some(reason) = failed_task {
            while let Some(&room_id) = self.pending_battle_launches.keys().next() {
                self.complete_training_preparation(room_id, Err(anyhow::anyhow!("{reason}")));
            }
        }
        if self.battle_worker.is_stopped() && !self.battle_participants.is_empty() {
            error!(
                participants = self.battle_participants.len(),
                "Battle worker stopped during active battles"
            );
            for participant in self.battle_participants.drain() {
                if let Some(&client) = self.battle_clients.get(&participant) {
                    self.base.disconnect_client(client);
                }
            }
            self.battle_departures.clear();
        }
    }

    pub(super) fn handle_battle_control(&mut self, event: base::CellMethodEvent) {
        let Some(session) = self.sessions.get(&event.client) else {
            return;
        };
        let participant = ParticipantId(session.generation);
        event.span.in_scope(|| {
            if !self.battle_participants.contains(&participant) {
                event.span.record("output.outcome", "arena_unavailable");
                return;
            }
            let control = MercuryArena::vehicle_control(participant, &event);
            let result = self.battle_worker.vehicle_control(control);
            self.handle_battle_submission(event.client, result);
        });
    }

    pub(super) fn handle_battle_submission(
        &mut self,
        client: ClientKey,
        result: Result<(), BattleQueueError>,
    ) {
        if let Err(error) = result {
            let span = tracing::Span::current();
            span.record("output.outcome", "battle_queue_unavailable");
            span.record("otel.status_code", "ERROR");
            error!(%error, "Could not submit battle command");
            self.base.disconnect_client(client);
        }
    }

    #[tracing::instrument(name = "player_sessions.return_from_battle", skip_all, fields(
        ddd.bounded_context.name = "player_sessions", input.client.address = %client,
        output.outcome = tracing::field::Empty, otel.status_code = tracing::field::Empty,
    ))]
    fn return_from_battle(&mut self, client: ClientKey, departure: ParticipantDeparture) {
        let span = tracing::Span::current();
        match departure {
            ParticipantDeparture::Entered => match self.base.reset_player(client) {
                Ok(()) => {
                    span.record("output.outcome", "garage_reset");
                }
                Err(error) => {
                    span.record("output.outcome", "return_failed");
                    span.record("otel.status_code", "ERROR");
                    error!(%error, "Could not return to the garage");
                    self.base.disconnect_client(client);
                }
            },
            ParticipantDeparture::Loading => {
                self.base.disconnect_client(client);
                span.record("output.outcome", "handoff_aborted");
            }
        }
    }
}
