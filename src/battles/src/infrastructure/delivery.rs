//! Delivery port used by the arena's Mercury encoders.

use super::{
    AvatarUpdate, ParticipantId, ScriptValue, mercury::MercuryArena, worker::BattleOutput,
};
use std::{
    io,
    net::SocketAddrV4,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use tracing::{Span, error};
use wot_platform::mercury::{
    app::{
        base::ArenaSnapshot,
        client::element::arena::{AvatarPosition, VehiclePosition},
    },
    net::element::{Element, SimpleElement},
};

pub(crate) trait ArenaDelivery {
    fn geometry_address(&self) -> io::Result<SocketAddrV4>;
    fn disconnect(&mut self, participant: ParticipantId);
    fn create_avatar(
        &mut self,
        participant: ParticipantId,
        id: u32,
        properties: ScriptValue,
    ) -> io::Result<()>;
    fn enable_arena_cell(&mut self, participant: ParticipantId, vehicle_id: u32) -> io::Result<()>;
    fn send_avatar(&mut self, participant: ParticipantId, update: AvatarUpdate) -> io::Result<()>;
    fn send_arena_element<E: SimpleElement + Send + 'static>(
        &mut self,
        participant: ParticipantId,
        element: E,
    ) -> io::Result<()>;
    fn send_arena_property(
        &mut self,
        participant: ParticipantId,
        entity_id: Option<u32>,
        element_id: u8,
        property: impl Element<()> + Send + 'static,
    ) -> io::Result<()>;
    fn arena_vehicle_method(
        &mut self,
        participant: ParticipantId,
        entity_id: u32,
        name: &str,
        args: Vec<ScriptValue>,
    ) -> io::Result<()>;
    fn publish_positions(&mut self, positions: BattlePositions);
}

type DeliveryAction = Box<dyn FnOnce(&mut MercuryArena<'_>) -> io::Result<()> + Send>;

pub struct BattleDelivery {
    participant: ParticipantId,
    parent: Span,
    action: DeliveryAction,
}

impl BattleDelivery {
    pub fn participant(&self) -> ParticipantId {
        self.participant
    }

    pub fn deliver(self, adapter: &mut MercuryArena<'_>) -> io::Result<()> {
        self.parent.in_scope(|| {
            let result = (self.action)(adapter);
            if let Err(error) = &result {
                self.parent.record("otel.status_code", "ERROR");
                error!(%error, "Queued arena message could not be delivered");
            }
            result
        })
    }
}

pub struct BattlePositions {
    pub(super) battle_id: u32,
    pub(super) tick: u8,
    pub(super) queued_at: Instant,
    pub(super) recipients: Vec<ParticipantId>,
    pub(super) vehicles: Vec<VehiclePosition>,
    pub(super) spans: Vec<Span>,
}

impl BattlePositions {
    pub fn deliver(self, adapter: &mut MercuryArena<'_>) -> Option<Duration> {
        let mut delivered = 0;
        let avatar = AvatarPosition {
            reference: 0,
            // The Avatar rides its Vehicle, so its pose is relative to the hull.
            position: glam::Vec3::ZERO,
            direction: glam::Vec3::ZERO,
        };
        let replication = self
            .spans
            .first()
            .cloned()
            .unwrap_or_else(tracing::Span::none);
        let recipients = self.recipients.iter().copied();
        let snapshot = ArenaSnapshot::encode(
            self.tick,
            avatar,
            self.vehicles.iter().cloned(),
            self.queued_at,
        );
        let mut snapshot = match snapshot {
            Ok(snapshot) => snapshot,
            Err(error) => {
                replication.in_scope(|| {
                    error!(%error, battle_id = self.battle_id, "Could not encode battle tick");
                });
                for participant in recipients {
                    adapter.disconnect(participant);
                }
                let count = self.recipients.len();
                self.finish(count, 0, Some("Arena pose snapshot encoding failed"));
                return None;
            }
        };
        let mut attempted = 0;
        for participant in recipients {
            if !adapter.has_client(participant) {
                continue;
            }
            attempted += 1;
            match replication.in_scope(|| adapter.deliver_positions(participant, &mut snapshot)) {
                Ok(()) => delivered += 1,
                Err(error) => {
                    replication.in_scope(|| {
                        error!(%error, battle_id = self.battle_id, "Could not send battle tick");
                    });
                    adapter.disconnect(participant);
                }
            }
        }
        let age = (attempted > 0).then(|| self.queued_at.elapsed());
        if delivered == attempted {
            self.finish(attempted, delivered, None);
        } else {
            self.finish(
                attempted,
                delivered,
                Some("Arena pose snapshot delivery failed"),
            );
        }
        age
    }

    fn finish(self, recipients: usize, delivered: usize, error: Option<&str>) {
        for span in self.spans {
            span.record("output.recipients.count", recipients);
            span.record("output.delivered.count", delivered);
            if let Some(error) = error {
                span.record("otel.status_code", "ERROR");
                span.in_scope(|| error!(reason = error, "Arena pose replication incomplete"));
            }
        }
    }
}

pub(super) struct QueuedDelivery<'a> {
    pub(super) address: SocketAddrV4,
    pub(super) outbox: mpsc::Sender<BattleOutput>,
    pub(super) snapshot_queue_drops: &'a AtomicU64,
}

impl QueuedDelivery<'_> {
    fn send(
        &self,
        participant: ParticipantId,
        action: impl FnOnce(&mut MercuryArena<'_>) -> io::Result<()> + Send + 'static,
    ) -> io::Result<()> {
        self.outbox
            .blocking_send(BattleOutput::Deliver(BattleDelivery {
                participant,
                parent: Span::current(),
                action: Box::new(action),
            }))
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "Arena output queue closed"))
    }
}

impl ArenaDelivery for QueuedDelivery<'_> {
    fn geometry_address(&self) -> io::Result<SocketAddrV4> {
        Ok(self.address)
    }

    fn disconnect(&mut self, participant: ParticipantId) {
        let _ = self.send(participant, move |adapter| {
            adapter.disconnect(participant);
            Ok(())
        });
    }

    fn create_avatar(
        &mut self,
        participant: ParticipantId,
        id: u32,
        properties: ScriptValue,
    ) -> io::Result<()> {
        self.send(participant, move |adapter| {
            adapter.create_avatar(participant, id, properties)
        })
    }

    fn enable_arena_cell(&mut self, participant: ParticipantId, vehicle: u32) -> io::Result<()> {
        self.send(participant, move |adapter| {
            adapter.enable_arena_cell(participant, vehicle)
        })
    }

    fn send_avatar(&mut self, participant: ParticipantId, update: AvatarUpdate) -> io::Result<()> {
        self.send(participant, move |adapter| {
            adapter.send_avatar(participant, update)
        })
    }

    fn send_arena_element<E: SimpleElement + Send + 'static>(
        &mut self,
        participant: ParticipantId,
        element: E,
    ) -> io::Result<()> {
        self.send(participant, move |adapter| {
            adapter.send_arena_element(participant, element)
        })
    }

    fn send_arena_property(
        &mut self,
        participant: ParticipantId,
        entity: Option<u32>,
        element: u8,
        property: impl Element<()> + Send + 'static,
    ) -> io::Result<()> {
        self.send(participant, move |adapter| {
            adapter.send_arena_property(participant, entity, element, property)
        })
    }

    fn arena_vehicle_method(
        &mut self,
        participant: ParticipantId,
        entity: u32,
        name: &str,
        args: Vec<ScriptValue>,
    ) -> io::Result<()> {
        let name = name.to_owned();
        self.send(participant, move |adapter| {
            adapter.arena_vehicle_method(participant, entity, &name, args)
        })
    }

    fn publish_positions(&mut self, mut positions: BattlePositions) {
        positions.queued_at = Instant::now();
        if let Err(mpsc::error::TrySendError::Full(BattleOutput::Positions(positions))) =
            self.outbox.try_send(BattleOutput::Positions(positions))
        {
            self.snapshot_queue_drops.fetch_add(1, Ordering::Relaxed);
            let recipients = positions.recipients.len();
            positions.finish(recipients, 0, Some("arena output queue full"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_output_queue_drops_only_the_unreliable_snapshot() {
        let (outbox, mut outputs) = mpsc::channel(1);
        let mut delivery = QueuedDelivery {
            address: SocketAddrV4::new(std::net::Ipv4Addr::LOCALHOST, 0),
            outbox,
            snapshot_queue_drops: &std::sync::atomic::AtomicU64::new(0),
        };
        let participant = ParticipantId(3);
        delivery.disconnect(participant);
        let snapshot = || BattlePositions {
            battle_id: 1,
            tick: 7,
            queued_at: Instant::now(),
            recipients: vec![participant],
            vehicles: Vec::new(),
            spans: Vec::new(),
        };

        delivery.publish_positions(snapshot());
        assert_eq!(delivery.snapshot_queue_drops.load(Ordering::Relaxed), 1);
        let BattleOutput::Deliver(disconnect) = outputs.try_recv().unwrap() else {
            panic!("disconnect should keep its slot");
        };
        assert_eq!(disconnect.participant(), participant);
        assert!(matches!(
            outputs.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));

        delivery.publish_positions(snapshot());
        assert!(matches!(outputs.try_recv(), Ok(BattleOutput::Positions(_))));
    }
}
