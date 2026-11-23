//! Adapts the gateway's live Mercury connection to the in-memory Battles contract.

use super::avatar::Avatar;
use super::{ArenaDelivery, AvatarUpdate, BattleRuntime, ParticipantId, ScriptValue};
use crate::battles::contract::{AvatarCommand, VehicleCommand, VehicleControl};
use crate::platform::mercury::{
    app::{
        base::{self, ClientKey},
        client::element::arena::{AvatarPosition, VehiclePosition},
        entity::MethodDecodeError,
    },
    net::element::{Element, SimpleElement},
};
use anyhow::{Context, Result};
use std::{
    collections::HashMap,
    io,
    net::{SocketAddr, SocketAddrV4},
};
use wot_resources::ResourceBundle;

pub(crate) fn bind(
    base: &mut base::App,
    resources: &ResourceBundle,
    enabled: bool,
) -> Result<BattleRuntime> {
    let runtime = BattleRuntime::load(base.shared_dispatch(), resources, enabled)?;
    if runtime.wire.is_some() {
        base.register::<Avatar>()
            .context("Bind battle Avatar schema")?;
    }
    Ok(runtime)
}

pub(crate) struct MercuryArena<'a> {
    base: &'a mut base::App,
    clients: &'a HashMap<ParticipantId, ClientKey>,
}

impl<'a> MercuryArena<'a> {
    pub(crate) fn new(
        base: &'a mut base::App,
        clients: &'a HashMap<ParticipantId, ClientKey>,
    ) -> Self {
        Self { base, clients }
    }

    fn client(&self, participant: ParticipantId) -> io::Result<ClientKey> {
        self.clients.get(&participant).copied().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotConnected,
                "Arena participant has no live session",
            )
        })
    }

    pub(crate) fn avatar_command(&self, event: &base::BaseMethodEvent) -> Option<AvatarCommand> {
        use super::avatar::AvatarBase;

        match self.base.decode::<AvatarBase>(event.entity, &event.call) {
            Ok((_, AvatarBase::SetClientReady {})) => Some(AvatarCommand::Ready),
            Ok((_, AvatarBase::LeaveArena {})) => Some(AvatarCommand::Leave),
            Err(error) => {
                let outcome = if matches!(error, MethodDecodeError::Unimplemented) {
                    "unimplemented"
                } else {
                    "decode_failed"
                };
                event.span.record("output.outcome", outcome);
                event.span.record("otel.status_code", "ERROR");
                tracing::error!(outcome, "Avatar base method could not be dispatched");
                None
            }
        }
    }

    pub(crate) fn vehicle_control(
        participant: ParticipantId,
        event: &base::CellMethodEvent,
    ) -> VehicleControl {
        use ScriptValue as Value;

        let command = match (&*event.name, event.args.as_slice()) {
            ("setServerMarker", [Value::UInt8(enabled @ (0 | 1))]) => {
                VehicleCommand::SetServerMarker(*enabled != 0)
            }
            ("vehicle_moveWith", [Value::UInt8(flags)]) => VehicleCommand::Drive(*flags),
            (
                "trackRelativePointWithGun" | "vehicle_trackRelativePointWithGun",
                [Value::Vector3(offset)],
            ) => VehicleCommand::AimRelative(*offset),
            ("vehicle_trackWorldPointWithGun", [Value::Vector3(point)]) => {
                VehicleCommand::AimWorld(*point)
            }
            (
                "stopTrackingWithGun" | "vehicle_stopTrackingWithGun",
                [Value::Float32(yaw), Value::Float32(pitch)],
            ) => VehicleCommand::AimAngles {
                yaw: *yaw,
                pitch: *pitch,
            },
            ("vehicle_shoot", []) => VehicleCommand::Shoot,
            (
                "vehicle_changeSetting",
                [
                    Value::UInt8(setting @ (0 | 1 | 17 | 18)),
                    Value::Int32(shell),
                ],
            ) => VehicleCommand::ChangeSetting {
                setting: *setting,
                shell: *shell,
            },
            _ => VehicleCommand::Unsupported,
        };
        VehicleControl {
            participant,
            entity_id: event.entity_id,
            method: event.name.clone(),
            argument_count: event.args.len(),
            command,
        }
    }
}

impl ArenaDelivery for MercuryArena<'_> {
    fn reserve_entity_id(&mut self) -> u32 {
        self.base.arena_entity_id()
    }

    fn geometry_address(&self) -> io::Result<SocketAddrV4> {
        match self.base.addr()? {
            SocketAddr::V4(address) => Ok(address),
            SocketAddr::V6(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "arena requires an IPv4 BaseApp",
            )),
        }
    }

    fn disconnect(&mut self, participant: ParticipantId) {
        if let Some(&client) = self.clients.get(&participant) {
            self.base.disconnect_client(client);
        }
    }

    fn create_avatar(
        &mut self,
        participant: ParticipantId,
        id: u32,
        properties: ScriptValue,
    ) -> io::Result<()> {
        let client = self.client(participant)?;
        self.base
            .create_base_player_with_id::<Avatar>(client, id, properties)?;
        Ok(())
    }

    fn enable_arena_cell(&mut self, participant: ParticipantId, vehicle_id: u32) -> io::Result<()> {
        let client = self.client(participant)?;
        self.base.enable_arena_cell(client, vehicle_id);
        Ok(())
    }

    fn send_avatar(&mut self, participant: ParticipantId, update: AvatarUpdate) -> io::Result<()> {
        let client = self.client(participant)?;
        update.send(self.base.player::<Avatar>(client)?)
    }

    fn send_arena_element<E: SimpleElement>(
        &mut self,
        participant: ParticipantId,
        element: E,
    ) -> io::Result<()> {
        let client = self.client(participant)?;
        self.base.send_arena_element(client, element)
    }

    fn send_arena_property(
        &mut self,
        participant: ParticipantId,
        entity_id: Option<u32>,
        element_id: u8,
        property: impl Element<()>,
    ) -> io::Result<()> {
        let client = self.client(participant)?;
        self.base
            .send_arena_property(client, entity_id, element_id, property)
    }

    fn arena_vehicle_method(
        &mut self,
        participant: ParticipantId,
        entity_id: u32,
        name: &str,
        args: Vec<ScriptValue>,
    ) -> io::Result<()> {
        let client = self.client(participant)?;
        self.base
            .arena_vehicle_method(client, entity_id, name, args)
    }

    fn arena_tick(
        &mut self,
        participant: ParticipantId,
        tick: u8,
        avatar: AvatarPosition,
        vehicles: impl Iterator<Item = VehiclePosition>,
    ) -> io::Result<()> {
        let client = self.client(participant)?;
        self.base.arena_tick(client, tick, avatar, vehicles)
    }
}
