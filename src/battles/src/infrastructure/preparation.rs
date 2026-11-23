//! Owns the bounded mailbox for arena preparation and keeps map loads off the tick task.

use super::worker::BattleQueueError;
use crate::contract::{BattleEntityIds, BattleLaunch, BattlePreparationId, PreparedBattle};
use anyhow::Result;
use tokio::{runtime::Handle, sync::mpsc, task::JoinHandle};
use tracing::Span;

const REQUEST_CAPACITY: usize = 8;
const RESULT_CAPACITY: usize = 8;

struct Request {
    id: BattlePreparationId,
    launch: BattleLaunch,
    entity_ids: Vec<BattleEntityIds>,
    parent: Span,
}

pub struct BattlePrepared {
    pub request: BattlePreparationId,
    pub result: Result<Option<Box<PreparedBattle>>>,
}

pub(super) struct PreparationWorker {
    requests: mpsc::Sender<Request>,
    results: mpsc::Receiver<BattlePrepared>,
    task: JoinHandle<()>,
}

impl PreparationWorker {
    pub(super) fn spawn(
        runtime: &Handle,
        mut prepare: impl FnMut(BattleLaunch, Vec<BattleEntityIds>) -> Result<Option<PreparedBattle>>
        + Send
        + 'static,
    ) -> Self {
        let (requests, mut inbox) = mpsc::channel::<Request>(REQUEST_CAPACITY);
        let (outbox, results) = mpsc::channel(RESULT_CAPACITY);
        let task = runtime.spawn_blocking(move || {
            while !outbox.is_closed()
                && let Some(request) = inbox.blocking_recv()
            {
                let result = request
                    .parent
                    .in_scope(|| prepare(request.launch, request.entity_ids))
                    .map(|battle| battle.map(Box::new));
                if outbox
                    .blocking_send(BattlePrepared {
                        request: request.id,
                        result,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            requests,
            results,
            task,
        }
    }

    pub(super) fn submit(
        &self,
        id: BattlePreparationId,
        launch: BattleLaunch,
        entity_ids: Vec<BattleEntityIds>,
    ) -> Result<(), BattleQueueError> {
        Ok(self.requests.try_send(Request {
            id,
            launch,
            entity_ids,
            parent: Span::current(),
        })?)
    }

    pub(super) fn try_prepared(&mut self) -> Option<BattlePrepared> {
        self.results.try_recv().ok()
    }

    pub(super) fn is_stopped(&self) -> bool {
        self.requests.is_closed() && self.results.is_empty()
    }

    pub(super) fn close(self) -> JoinHandle<()> {
        drop(self.requests);
        drop(self.results);
        self.task
    }
}
