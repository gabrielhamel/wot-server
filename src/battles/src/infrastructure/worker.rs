//! Owns the live simulation and queues work for the Mercury adapter.

use super::{
    AvatarAction, AvatarCommand, BattleRuntime, BattleSimulation, ParticipantDeparture,
    ParticipantId, PreparedBattle, TICK,
    delivery::{BattleDelivery, BattlePositions, QueuedDelivery},
    preparation::{BattlePrepared, PreparationWorker},
    wire::ArenaWire,
};
use crate::application::{BattleEntityIds, BattleLaunch, BattlePreparationId, VehicleControl};
use std::{
    collections::BTreeMap,
    net::SocketAddrV4,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{runtime::Handle, sync::mpsc, task::JoinHandle};
use tracing::{Span, error};

const COMMAND_CAPACITY: usize = 1024;
const DELIVERY_CAPACITY: usize = 4096;

enum Command {
    Start(Box<PreparedBattle>),
    Enter(ParticipantId),
    Avatar(ParticipantId, u32, AvatarCommand),
    Vehicle(VehicleControl),
    Disconnect(ParticipantId),
}

type TracedCommand = (Span, Command);

#[derive(Clone, Copy, Debug, thiserror::Error)]
pub enum BattleQueueError {
    #[error("battle command queue is full")]
    Full,
    #[error("battle worker stopped")]
    Closed,
}

impl<T> From<mpsc::error::TrySendError<T>> for BattleQueueError {
    fn from(error: mpsc::error::TrySendError<T>) -> Self {
        match error {
            mpsc::error::TrySendError::Full(_) => Self::Full,
            mpsc::error::TrySendError::Closed(_) => Self::Closed,
        }
    }
}

pub struct BattleStart(mpsc::OwnedPermit<TracedCommand>);

impl BattleStart {
    pub fn start(self, battle: PreparedBattle) {
        self.0
            .send((Span::current(), Command::Start(Box::new(battle))));
    }
}

pub enum BattleOutput {
    Deliver(BattleDelivery),
    Positions(BattlePositions),
    Return {
        participant: ParticipantId,
        departure: ParticipantDeparture,
        parent: Span,
    },
    Tick {
        delay: Duration,
        duration: Duration,
    },
}

pub struct BattleWorker {
    commands: mpsc::Sender<TracedCommand>,
    outputs: mpsc::Receiver<BattleOutput>,
    preparation: PreparationWorker,
    active: Arc<AtomicUsize>,
    snapshot_queue_drops: Arc<AtomicU64>,
    maps: BTreeMap<i32, Option<String>>,
    task: JoinHandle<()>,
}

impl BattleRuntime {
    pub fn spawn_worker(mut self, runtime: &Handle, address: SocketAddrV4) -> BattleWorker {
        let maps = self
            .maps
            .iter()
            .map(|(&id, map)| {
                let name = self
                    .wire
                    .as_ref()
                    .and(map.as_ref())
                    .filter(|map| {
                        self.resources
                            .read_dir(&format!("{}/terrain", map.geometry))
                            .is_ok_and(|mut tiles| tiles.next().is_some())
                    })
                    .map(|map| map.name.clone());
                (id, name)
            })
            .collect();
        let wire = self.wire.clone();
        let preparation =
            PreparationWorker::spawn(runtime, move |launch, ids| self.prepare(launch, ids));
        BattleWorker::spawn(runtime, address, wire, maps, preparation)
    }
}

impl BattleWorker {
    fn spawn(
        runtime: &Handle,
        address: SocketAddrV4,
        wire: Option<Arc<ArenaWire>>,
        maps: BTreeMap<i32, Option<String>>,
        preparation: PreparationWorker,
    ) -> Self {
        let (commands, inbox) = mpsc::channel(COMMAND_CAPACITY);
        let (outbox, outputs) = mpsc::channel(DELIVERY_CAPACITY);
        let active = Arc::new(AtomicUsize::new(0));
        let worker_active = Arc::clone(&active);
        let snapshot_queue_drops = Arc::new(AtomicU64::new(0));
        let worker_snapshot_queue_drops = Arc::clone(&snapshot_queue_drops);
        let simulation = BattleSimulation {
            active: Default::default(),
            engagements: Default::default(),
            wire,
        };
        let handle = runtime.clone();
        let task = runtime.spawn_blocking(move || {
            simulation.run(
                inbox,
                QueuedDelivery {
                    address,
                    outbox,
                    snapshot_queue_drops: &worker_snapshot_queue_drops,
                },
                &worker_active,
                &handle,
            );
        });
        Self {
            commands,
            outputs,
            preparation,
            active,
            snapshot_queue_drops,
            maps,
            task,
        }
    }

    pub fn has_maps(&self) -> bool {
        !self.maps.is_empty()
    }

    pub fn has_map(&self, arena_type: i32) -> bool {
        self.maps.contains_key(&arena_type)
    }

    pub fn maps(&self) -> impl Iterator<Item = (i32, &str)> {
        self.maps
            .iter()
            .filter_map(|(&id, name)| Some((id, name.as_deref()?)))
    }

    pub fn prepare(
        &self,
        request: BattlePreparationId,
        launch: BattleLaunch,
        entity_ids: Vec<BattleEntityIds>,
    ) -> Result<(), BattleQueueError> {
        if self.is_stopped() {
            return Err(BattleQueueError::Closed);
        }
        self.preparation.submit(request, launch, entity_ids)
    }

    pub fn try_prepared(&mut self) -> Option<BattlePrepared> {
        self.preparation.try_prepared()
    }

    pub fn is_preparation_stopped(&self) -> bool {
        self.preparation.is_stopped()
    }

    pub fn reserve_start(&self) -> Result<BattleStart, BattleQueueError> {
        Ok(BattleStart(self.commands.clone().try_reserve_owned()?))
    }

    pub fn enter(&self, participant: ParticipantId) -> Result<(), BattleQueueError> {
        self.send(Command::Enter(participant))
    }

    pub fn avatar_command(
        &self,
        participant: ParticipantId,
        entity_id: u32,
        command: AvatarCommand,
    ) -> Result<(), BattleQueueError> {
        self.send(Command::Avatar(participant, entity_id, command))
    }

    pub fn vehicle_control(&self, control: VehicleControl) -> Result<(), BattleQueueError> {
        self.send(Command::Vehicle(control))
    }

    pub fn disconnect(&self, participant: ParticipantId) -> Result<(), BattleQueueError> {
        self.send(Command::Disconnect(participant))
    }

    fn send(&self, command: Command) -> Result<(), BattleQueueError> {
        Ok(self.commands.try_send((Span::current(), command))?)
    }

    pub fn try_output(&mut self) -> Option<BattleOutput> {
        self.outputs.try_recv().ok()
    }

    pub fn is_stopped(&self) -> bool {
        self.commands.is_closed()
    }

    pub fn take_snapshot_queue_drops(&self) -> u64 {
        self.snapshot_queue_drops.swap(0, Ordering::Relaxed)
    }

    pub fn active_count(&self) -> usize {
        if self.is_stopped() {
            return 0;
        }
        self.active.load(Ordering::Relaxed)
    }

    pub fn shutdown(self, runtime: &Handle) -> Result<(), tokio::task::JoinError> {
        drop(self.commands);
        drop(self.outputs);
        let preparation = self.preparation.close();
        runtime.block_on(async {
            let (simulation, preparation) = tokio::join!(self.task, preparation);
            simulation.and(preparation)
        })
    }
}

impl BattleSimulation {
    fn run(
        mut self,
        mut commands: mpsc::Receiver<TracedCommand>,
        mut delivery: QueuedDelivery<'_>,
        active: &AtomicUsize,
        runtime: &Handle,
    ) {
        let mut deadline = Instant::now() + TICK;
        while !delivery.outbox.is_closed() {
            let now = Instant::now();
            if now >= deadline {
                if !self.active.is_empty() {
                    for (participant, parent) in self.tick_with_delivery(&mut delivery, now) {
                        parent.in_scope(|| self.return_participant(&mut delivery, participant));
                    }
                    active.store(self.active.len(), Ordering::Relaxed);
                    let _ = delivery.outbox.blocking_send(BattleOutput::Tick {
                        delay: now.duration_since(deadline),
                        duration: now.elapsed(),
                    });
                }
                deadline += TICK;
                if deadline <= now {
                    deadline = now + TICK;
                }
            }
            match runtime
                .block_on(async { tokio::time::timeout_at(deadline.into(), commands.recv()).await })
            {
                Ok(Some((parent, command))) => parent.in_scope(|| {
                    self.apply(&mut delivery, command);
                    active.store(self.active.len(), Ordering::Relaxed);
                }),
                Ok(None) => break,
                Err(_) => {}
            }
        }
        active.store(0, Ordering::Relaxed);
    }

    fn apply(&mut self, delivery: &mut QueuedDelivery<'_>, command: Command) {
        match command {
            Command::Start(battle) => self.start(*battle),
            Command::Enter(participant) => {
                if let Err(error) = self.enter_with_delivery(delivery, participant) {
                    error!(%error, participant = participant.0, "Arena entry failed");
                }
            }
            Command::Avatar(participant, entity, command) => {
                if let AvatarAction::Leave =
                    self.avatar_command_with_delivery(delivery, participant, entity, command)
                {
                    self.return_participant(delivery, participant);
                }
            }
            Command::Vehicle(control) => self.vehicle_control_with_delivery(delivery, control),
            Command::Disconnect(participant) => {
                self.remove_participant_with_delivery(delivery, participant);
            }
        }
    }

    fn return_participant(
        &mut self,
        delivery: &mut QueuedDelivery<'_>,
        participant: ParticipantId,
    ) {
        if let Some(departure) = self.remove_participant_with_delivery(delivery, participant) {
            let _ = delivery.outbox.blocking_send(BattleOutput::Return {
                participant,
                departure,
                parent: Span::current(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn commands_respect_reserved_capacity_without_waiting() {
        let (commands, mut inbox) = mpsc::channel(1);
        let (_outbox, outputs) = mpsc::channel(1);
        let worker = BattleWorker {
            commands,
            outputs,
            preparation: PreparationWorker::spawn(&Handle::current(), |_, _| Ok(None)),
            active: Arc::new(AtomicUsize::new(0)),
            snapshot_queue_drops: Arc::new(AtomicU64::new(0)),
            maps: BTreeMap::new(),
            task: tokio::spawn(async {}),
        };
        let participant = ParticipantId(3);
        let reservation = worker.reserve_start().unwrap();
        assert!(matches!(
            worker.enter(participant),
            Err(BattleQueueError::Full)
        ));

        drop(reservation);
        worker.enter(participant).unwrap();
        assert!(matches!(
            worker.reserve_start(),
            Err(BattleQueueError::Full)
        ));
        assert!(matches!(inbox.try_recv(), Ok((_, Command::Enter(id))) if id == participant));

        drop(inbox);
        assert!(matches!(
            worker.disconnect(participant),
            Err(BattleQueueError::Closed)
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn live_arena_ticks_while_preparation_is_blocked() {
        use super::super::{Battle, Round, maps::ArenaMap, obstacles::Scenery, terrain::Terrain};
        use crate::application::BattleMode;
        use glam::Vec2;

        let handle = Handle::current();
        let (started, mut loading) = mpsc::channel(1);
        let (resume, release) = std::sync::mpsc::channel::<()>();
        let preparation = PreparationWorker::spawn(&handle, move |_, _| {
            started.blocking_send(()).unwrap();
            let _ = release.recv();
            Ok(None)
        });
        let mut worker = BattleWorker::spawn(
            &handle,
            SocketAddrV4::new(std::net::Ipv4Addr::LOCALHOST, 0),
            None,
            BTreeMap::new(),
            preparation,
        );
        let now = Instant::now();
        worker.reserve_start().unwrap().start(PreparedBattle {
            id: 1,
            battle: Battle {
                parameters: Default::default(),
                map: ArenaMap {
                    season: Default::default(),
                    name: "Test Map".into(),
                    geometry: String::new(),
                    spawns: [Vec2::ZERO; 2],
                    bounds: [Vec2::ZERO; 2],
                },
                ground: Arc::new(Terrain::empty()),
                obstacles: Scenery::default(),
                areas: Default::default(),
                arena_type: 1,
                players: Default::default(),
                started_at: now,
                last_snapshot_tick: None,
                round: Round::new(now, Duration::from_secs(300), &Default::default()),
                launch: None,
                shot_sequence: 0,
                projectiles: Vec::new(),
                result: None,
                finish_span: None,
                roster_count: 0,
            },
        });

        let timeout = Duration::from_secs(3);
        assert!(matches!(
            tokio::time::timeout(timeout, worker.outputs.recv()).await,
            Ok(Some(BattleOutput::Tick { .. }))
        ));
        worker
            .prepare(
                BattlePreparationId::TrainingRoom(2),
                BattleLaunch {
                    parameters: Default::default(),
                    arena_type: 2,
                    duration: Duration::from_secs(300),
                    mode: BattleMode::Training,
                    participants: Vec::new(),
                },
                Vec::new(),
            )
            .unwrap();
        assert_eq!(
            tokio::time::timeout(timeout, loading.recv()).await.unwrap(),
            Some(())
        );
        while worker.try_output().is_some() {}
        for _ in 0..2 {
            assert!(matches!(
                tokio::time::timeout(timeout, worker.outputs.recv()).await,
                Ok(Some(BattleOutput::Tick { .. }))
            ));
        }
        assert!(worker.try_prepared().is_none());
        drop(resume);

        tokio::task::spawn_blocking(move || worker.shutdown(&handle))
            .await
            .unwrap()
            .unwrap();
    }
}
