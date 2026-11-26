use super::*;
use crate::{domain::devices::FireOrigin, infrastructure::vehicle_damage::ActiveFire};

const FIRE_ENTITY_NODE_START: u16 = 4096;
const FIRE_NODES_PER_VEHICLE: u16 = 4;
const FIRE_STARTED_AT_SHOT: u8 = 0;

pub(super) struct FireWire {
    component_type: u16,
    nrl_type: u16,
    engine_extra: u8,
    fuel_extra: u8,
}

impl FireWire {
    pub(super) fn bind(dispatch: &ScriptDispatch, resources: &ResourceBundle) -> Result<Self> {
        let (component_type, component) = dispatch
            .entity_from_name("Fire")
            .context("Missing Fire component")?;
        let nrl_type = dispatch
            .component_nrl_id(component_type)
            .context("Fire has no client replication type")?;
        anyhow::ensure!(
            component.properties.len() == 1
                && component.properties[0].name.as_ref() == "fireInfo"
                && !component.properties[0].volatile
                && component.client_methods.is_empty(),
            "Unexpected Fire component layout"
        );
        let definition = wot_platform::client_formats::pxml::from_reader(BufReader::new(
            resources.read("scripts/item_defs/vehicles/common/vehicle.xml")?,
        ))?;
        let extras = definition
            .get_child_element("extras")
            .context("Missing vehicle extras")?;
        let extra = |name| {
            let index = extras
                .iter_children_all()
                .position(|(key, _)| key == name)
                .with_context(|| format!("Missing {name} extra"))?;
            Ok::<_, anyhow::Error>(u8::try_from(index + 1)?)
        };
        let wire = Self {
            component_type,
            nrl_type,
            engine_extra: extra("engineHealth")?,
            fuel_extra: extra("fuelTankHealth")?,
        };
        let ty = &component.properties[0].ty;
        anyhow::ensure!(
            ty.is_instance(&WireValue::None),
            "Fire.fireInfo must allow an empty value"
        );
        wire.info(ty, FireOrigin::Engine, 0, 0.0)?
            .write(&mut Vec::new(), ty)?;
        Ok(wire)
    }

    fn info(
        &self,
        ty: &Ty,
        origin: FireOrigin,
        attacker: u32,
        started_at: f64,
    ) -> Result<WireValue> {
        let extra = match origin {
            FireOrigin::Engine => self.engine_extra,
            FireOrigin::Fuel => self.fuel_extra,
        };
        dict(
            ty,
            [
                ("deviceExtraIndex", WireValue::UInt8(extra)),
                ("notificationIndex", WireValue::UInt8(FIRE_STARTED_AT_SHOT)),
                ("attackerID", id(ty, "attackerID", attacker)?),
                ("equipmentID", WireValue::UInt16(0)),
                ("startTime", WireValue::Float64(started_at)),
            ],
        )
    }
}

fn nodes(row: usize) -> Result<(u16, u16)> {
    let row = u8::try_from(row).context("Fire vehicle row exceeds arena capacity")?;
    let root = FIRE_ENTITY_NODE_START + u16::from(row) * FIRE_NODES_PER_VEHICLE;
    Ok((root, root + 1))
}

impl ArenaWire {
    pub(in super::super) fn publish_fire_start(
        &self,
        delivery: &mut impl ArenaDelivery,
        peer: ParticipantId,
        row: usize,
        own: bool,
        fire: &ActiveFire,
        missing_remote_root: Option<u32>,
    ) -> Result<()> {
        let (root, node) = nodes(row)?;
        let parent = if own { controls::OWN_ENTITY_NODE } else { root };
        let component = self
            .dispatch
            .entity_from_id(self.fire.component_type)
            .context("Missing Fire component")?;
        let mut data = Vec::new();
        if let Some(vehicle) = missing_remote_root {
            write_entity_node(&mut data, vehicle, root)?;
            data.push(0); // another create-node instruction
        }
        write_component_node(
            &mut data,
            component,
            self.fire.nrl_type,
            parent,
            node,
            BTreeMap::from([("fireInfo", WireValue::None)]),
        )?;
        delivery.send_arena_element(peer, NrlCreateNode(data))?;
        if own {
            let ty = &component.properties[0].ty;
            let mut body = Vec::new();
            self.fire
                .info(ty, fire.origin, fire.attacker, fire.started_at)?
                .write(&mut body, ty)?;
            let mut record = (node + 1).to_be_bytes().to_vec();
            record.push(0);
            record.write_blob_variable(&body)?;
            delivery.send_arena_element(peer, NrlData(record))?;
            delivery.send_arena_element(peer, ApplyNrlUpdate {})?;
        }
        Ok(())
    }

    pub(in super::super) fn publish_fire_stop(
        &self,
        delivery: &mut impl ArenaDelivery,
        peer: ParticipantId,
        row: usize,
    ) -> Result<()> {
        let (_, node) = nodes(row)?;
        delivery.send_arena_element(peer, NrlData(node.to_be_bytes().to_vec()))?;
        delivery.send_arena_element(peer, ApplyNrlUnlink {})?;
        Ok(())
    }
}
