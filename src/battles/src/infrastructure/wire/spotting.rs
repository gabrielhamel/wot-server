use super::*;

const DIRECT_RAYTRACE: i16 = 0;

impl ArenaWire {
    pub(in super::super) fn publish_visible_vehicle(
        &self,
        delivery: &mut impl ArenaDelivery,
        peer: ParticipantId,
        player: &Participant,
        started_at: Instant,
    ) -> Result<()> {
        let id = player.vehicle.entity_id;
        publish_entity(delivery, peer, player.vehicle.clone())?;
        self.vehicle_properties(
            delivery,
            peer,
            id,
            [
                ("health", WireValue::Int16(player.tank.health)),
                (
                    "isCrewActive",
                    WireValue::UInt8(u8::from(player.tank.health > 0)),
                ),
            ],
        )?;
        self.publish_gun_angles(delivery, peer, id, player.tank.packed_gun_angles())?;
        self.publish_engine_mode(delivery, peer, id, player.last_engine_mode)?;
        for (steering, values) in [false, true].into_iter().zip(&player.last_wheels) {
            if !values.is_empty() {
                self.publish_wheels(delivery, peer, id, steering, values)?;
            }
        }
        if let Some((end, _)) = player.pending_stun.as_ref() {
            let expiration = end.saturating_duration_since(started_at).as_secs_f64();
            self.vehicle_properties(
                delivery,
                peer,
                id,
                [("stunInfo", WireValue::Float64(expiration))],
            )?;
        }
        self.publish_broken_tracks(delivery, peer, id, &player.tank)?;
        if !player.damage_stickers.is_empty() {
            self.publish_damage_stickers(delivery, peer, id, &player.damage_stickers)?;
        }
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
            matches!(method.args.as_slice(), [ty] if ty.is_instance(&argument)),
            "Invalid OwnVehicle.onObservedByEnemy argument"
        );
        let offset = u16::try_from(2 + component.properties.len() + method_index)?;
        let node = controls::OWN_ENTITY_NODE
            .checked_add(offset)
            .context("OwnVehicle method node overflow")?;
        let mut body = Vec::new();
        method.write_args(&mut body, &[argument])?;
        let mut message = node.to_be_bytes().to_vec();
        message.push(0);
        message.write_blob_variable(&body)?;
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
