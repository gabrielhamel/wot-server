use super::*;
use byteorder::ReadBytesExt;
use wot_platform::mercury::net::element::ElementLength;

const DIRECT_RAYTRACE: i16 = 0;

impl ArenaWire {
    pub(in super::super) fn publish_visible_vehicle(
        &self,
        delivery: &mut impl ArenaDelivery,
        peer: ParticipantId,
        player: &Participant,
        started_at: Instant,
    ) -> Result<()> {
        let schema = self
            .dispatch
            .entity_from_id(self.vehicle_type)
            .context("Missing Vehicle")?;
        let mut vehicle = player.vehicle.clone();
        let mut encoded = vehicle.properties.as_slice();
        let mut state = BTreeMap::new();
        let count = encoded.read_u8()?;
        for _ in 0..count {
            let property = schema
                .properties
                .get(usize::from(encoded.read_u8()?))
                .context("Invalid Vehicle creation property")?;
            state.insert(
                property.name.as_ref(),
                WireValue::read(&mut encoded, &property.ty)?,
            );
        }
        state.extend([
            ("health", WireValue::Int16(player.tank.health)),
            (
                "isCrewActive",
                WireValue::UInt8(u8::from(player.tank.health > 0)),
            ),
            (
                "gunAnglesPacked",
                WireValue::UInt16(player.tank.packed_gun_angles()),
            ),
            (
                "engineMode",
                WireValue::Seq(
                    player
                        .last_engine_mode
                        .into_iter()
                        .map(WireValue::UInt8)
                        .collect(),
                ),
            ),
            (
                "publicStateModifiers",
                self.broken_tracks_state(&player.tank),
            ),
        ]);
        for (name, values) in ["wheelsScroll", "steeringAngles"]
            .into_iter()
            .zip(&player.last_wheels)
        {
            if !values.is_empty() {
                state.insert(
                    name,
                    WireValue::Seq(values.iter().copied().map(WireValue::UInt8).collect()),
                );
            }
        }
        if let Some((end, _)) = player.pending_stun.as_ref() {
            let expiration = end.saturating_duration_since(started_at).as_secs_f64();
            state.insert("stunInfo", WireValue::Float64(expiration));
        }
        if !player.damage_stickers.is_empty() {
            state.insert("damageStickers", player.damage_stickers.value());
        }
        vehicle.properties = properties_by_id(schema, state)?;
        publish_entity(delivery, peer, vehicle)?;
        Ok(())
    }

    pub(super) fn sixth_sense_message(&self) -> Result<NrlData> {
        let component = self
            .dispatch
            .entity_from_id(self.own_vehicle_type)
            .context("Missing OwnVehicle component")?;
        let (method_index, method) = component
            .client_methods
            .iter()
            .enumerate()
            .find(|(_, method)| method.name.as_ref() == "onObservedByEnemy")
            .context("Missing OwnVehicle.onObservedByEnemy")?;
        let argument = WireValue::Int16(DIRECT_RAYTRACE);
        anyhow::ensure!(
            method.length == ElementLength::Fixed(2)
                && matches!(method.args.as_slice(), [ty] if ty.is_instance(&argument)),
            "Invalid OwnVehicle.onObservedByEnemy schema"
        );
        let offset = u16::try_from(2 + component.properties.len() + method_index)?;
        let node = controls::OWN_ENTITY_NODE
            .checked_add(offset)
            .context("OwnVehicle method node overflow")?;
        let mut message = node.to_be_bytes().to_vec();
        method.write_args(&mut message, &[argument])?;
        Ok(NrlData(message))
    }

    pub(in super::super) fn publish_sixth_sense(
        &self,
        delivery: &mut impl ArenaDelivery,
        peer: ParticipantId,
    ) -> Result<()> {
        delivery.send_arena_element(peer, self.sixth_sense_message()?)?;
        delivery.send_arena_element(peer, ApplyNrlUpdate {})?;
        Ok(())
    }
}
