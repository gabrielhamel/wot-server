//! Delivery port used by the arena's Mercury encoders.

use super::{
    AvatarUpdate, ParticipantId, ScriptValue, mercury::MercuryArena, worker::BattleOutput,
};
use std::{io, net::SocketAddrV4};
use tokio::sync::mpsc;
use tracing::{Span, error};
use wot_platform::mercury::{
    app::client::element::arena::{AvatarPosition, VehiclePosition},
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
    pub(super) recipients: Vec<ParticipantId>,
    pub(super) vehicles: Vec<VehiclePosition>,
    pub(super) spans: Vec<Span>,
}

impl BattlePositions {
    pub fn deliver(self, adapter: &mut MercuryArena<'_>) {
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
        let mut attempted = 0;
        for &participant in &self.recipients {
            if !adapter.has_client(participant) {
                continue;
            }
            attempted += 1;
            let result = replication.in_scope(|| {
                adapter.deliver_positions(
                    participant,
                    self.tick,
                    avatar.clone(),
                    self.vehicles.iter().cloned(),
                )
            });
            match result {
                Ok(()) => delivered += 1,
                Err(error) => {
                    error!(%error, battle_id = self.battle_id, "Could not send battle tick");
                    adapter.disconnect(participant);
                }
            }
        }
        if delivered == attempted {
            self.finish(attempted, delivered, None);
        } else {
            self.finish(
                attempted,
                delivered,
                Some("Arena pose snapshot delivery failed"),
            );
        }
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

pub(super) struct QueuedDelivery {
    pub(super) address: SocketAddrV4,
    pub(super) outbox: mpsc::Sender<BattleOutput>,
}

impl QueuedDelivery {
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

impl ArenaDelivery for QueuedDelivery {
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

    fn publish_positions(&mut self, positions: BattlePositions) {
        if let Err(mpsc::error::TrySendError::Full(BattleOutput::Positions(positions))) =
            self.outbox.try_send(BattleOutput::Positions(positions))
        {
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
        };
        let participant = ParticipantId(3);
        delivery.disconnect(participant);
        let snapshot = || BattlePositions {
            battle_id: 1,
            tick: 7,
            recipients: vec![participant],
            vehicles: Vec::new(),
            spans: Vec::new(),
        };

        delivery.publish_positions(snapshot());
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
