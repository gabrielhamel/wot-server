//! Synchronous delivery port used by the arena's Mercury encoders.

use super::{AvatarUpdate, ParticipantId, ScriptValue};
use std::{io, net::SocketAddrV4};
use wot_platform::mercury::{
    app::client::element::arena::{AvatarPosition, VehiclePosition},
    net::element::{Element, SimpleElement},
};

pub(crate) trait ArenaDelivery {
    fn reserve_entity_id(&mut self) -> u32;
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
    fn send_arena_element<E: SimpleElement>(
        &mut self,
        participant: ParticipantId,
        element: E,
    ) -> io::Result<()>;
    fn send_arena_property(
        &mut self,
        participant: ParticipantId,
        entity_id: Option<u32>,
        element_id: u8,
        property: impl Element<()>,
    ) -> io::Result<()>;
    fn arena_vehicle_method(
        &mut self,
        participant: ParticipantId,
        entity_id: u32,
        name: &str,
        args: Vec<ScriptValue>,
    ) -> io::Result<()>;
    fn arena_tick(
        &mut self,
        participant: ParticipantId,
        tick: u8,
        avatar: AvatarPosition,
        vehicles: impl Iterator<Item = VehiclePosition>,
    ) -> io::Result<()>;
}
