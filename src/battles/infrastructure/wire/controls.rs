use super::*;
use crate::battles::domain::vehicle::Tank;
use std::time::Instant;

const OWN_ENTITY_NODE: u16 = 1024;
type Fields = BTreeMap<&'static str, WireValue>;

impl ArenaWire {
    pub(super) fn validate_controls(&self, dispatch: &ScriptDispatch) -> Result<()> {
        let methods = &dispatch
            .entity_from_id(self.vehicle_type)
            .context("Missing Vehicle")?
            .client_methods;
        for (name, args) in [
            (
                "showShooting",
                vec![WireValue::UInt8(1), WireValue::Int8(0), WireValue::UInt8(0)],
            ),
            (
                "onHealthChanged",
                vec![
                    WireValue::Int16(0),
                    WireValue::Int16(1),
                    WireValue::Int32(1),
                    WireValue::UInt8(0),
                    WireValue::Int8(-1),
                ],
            ),
        ] {
            let method = methods
                .iter()
                .find(|method| &*method.name == name)
                .with_context(|| format!("Missing Vehicle.{name}"))?;
            anyhow::ensure!(
                method.args.len() == args.len()
                    && method
                        .args
                        .iter()
                        .zip(&args)
                        .all(|(ty, arg)| ty.is_instance(arg)),
                "Invalid Vehicle.{name} arguments"
            );
        }
        Ok(())
    }

    pub(super) fn own_component(
        &self,
        dispatch: &ScriptDispatch,
        player: &Entrant,
    ) -> Result<NrlCreateNode> {
        let component = dispatch
            .entity_from_id(self.own_vehicle_type)
            .context("Missing OwnVehicle")?;
        let tank = player.tank();
        let gun = tank.performance.as_ref().map(|profile| &profile.gun);
        let mut fields = own_state(component, player.vehicle_id, &tank, Instant::now(), 0.0)?;
        let ty = property_type(component, "syncVehicleAttrs")?;
        fields.insert(
            "syncVehicleAttrs",
            dict(
                ty,
                [
                    ("circularVisionRadius", WireValue::UInt16(350)),
                    ("gunPiercing", WireValue::Float32(1.0)),
                    ("gunShotsSpeed", WireValue::Float32(1.0)),
                ],
            )?,
        );
        fields.insert(
            "targetingInfo",
            dict(
                property_type(component, "targetingInfo")?,
                [
                    (
                        "maxTurretRotationSpeed",
                        WireValue::Float32(gun.map_or(0.0, |gun| gun.turret_traverse)),
                    ),
                    (
                        "maxGunRotationSpeed",
                        WireValue::Float32(gun.map_or(0.0, |gun| gun.gun_traverse)),
                    ),
                    ("shotDispMultiplierFactor", WireValue::Float32(1.0)),
                    (
                        "aimingTime",
                        WireValue::Float32(gun.map_or(0.0, |gun| gun.aiming_time)),
                    ),
                ],
            )?,
        );
        component_tree(
            component,
            self.own_vehicle_nrl_type,
            player.vehicle_id,
            OWN_ENTITY_NODE,
            fields,
        )
    }

    pub(in super::super) fn publish_own_state(
        &self,
        base: &mut base::App,
        client: ClientKey,
        vehicle_id: u32,
        tank: &Tank,
        now: Instant,
        time: f32,
    ) -> Result<()> {
        let component = base
            .dispatch()
            .entity_from_id(self.own_vehicle_type)
            .context("Missing OwnVehicle")?;
        let mut values = own_state(component, vehicle_id, tank, now, time)?;
        let mut records = Vec::new();
        for (index, property) in component.properties.iter().enumerate() {
            let Some(value) = values.remove(&*property.name) else {
                continue;
            };
            let node = OWN_ENTITY_NODE
                .checked_add(2 + u16::try_from(index)?)
                .context("OwnVehicle node overflow")?;
            let mut record = node.to_be_bytes().to_vec();
            if property.volatile {
                value.write(&mut record, &property.ty)?;
            } else {
                let mut body = Vec::new();
                value.write(&mut body, &property.ty)?;
                record.push(0);
                record.write_blob_variable(&body)?;
            }
            records.push(record);
        }
        for record in records {
            base.send_arena_element(client, NrlData(record))?;
            base.send_arena_element(client, ApplyNrlUpdate {})?;
        }
        Ok(())
    }

    pub(in super::super) fn vehicle_properties<const N: usize>(
        &self,
        base: &mut base::App,
        client: ClientKey,
        entity_id: u32,
        fields: [(&str, WireValue); N],
    ) -> Result<()> {
        let schema = base
            .dispatch()
            .entity_from_id(self.vehicle_type)
            .context("Missing Vehicle")?;
        let update = properties_by_id(schema, fields)?;
        base.send_arena_element(client, SelectEntity { entity_id })?;
        base.send_arena_element(client, ArenaEntityProperties(update))?;
        Ok(())
    }
}

fn property_type<'a>(component: &'a EntityDispatch, name: &str) -> Result<&'a Ty> {
    component
        .properties
        .iter()
        .find(|p| &*p.name == name)
        .map(|p| &p.ty)
        .with_context(|| format!("Missing OwnVehicle.{name}"))
}

fn rows(ty: &Ty) -> Result<&Ty> {
    match unalias(ty) {
        TyKind::Array(array) => Ok(&array.ty),
        _ => anyhow::bail!("Expected array for {}", ty.name()),
    }
}

fn own_state(
    component: &EntityDispatch,
    vehicle_id: u32,
    tank: &Tank,
    now: Instant,
    time: f32,
) -> Result<Fields> {
    let ammunition = rows(property_type(component, "vehicleAmmoList")?)?;
    let gun = tank
        .performance
        .as_ref()
        .map(|profile| &profile.gun.loading);
    let has_magazine = gun.is_some_and(|gun| gun.capacity > 1);
    let ammo = tank
        .rounds()
        .map(|(shell, count, loaded)| {
            dict(
                ammunition,
                [
                    ("compactDescr", WireValue::Int32(shell)),
                    ("quantity", WireValue::UInt16(count)),
                    (
                        "quantityInClip",
                        WireValue::UInt16(if has_magazine { loaded } else { 0 }),
                    ),
                ],
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let setting = rows(property_type(component, "vehicleSettings")?)?;
    let settings = [0, 1]
        .into_iter()
        .zip(tank.shell_settings())
        .map(|(code, shell)| {
            dict(
                setting,
                [
                    ("code", WireValue::UInt8(code)),
                    ("value", WireValue::Int32(shell)),
                    ("vehicleID", id(setting, "vehicleID", vehicle_id)?),
                ],
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let [gun_reload, clip_reload] = tank.reload_timers();
    let reload_ty = property_type(component, "vehicleGunReloadTime")?;
    let clip_reload_ty = property_type(component, "vehicleClipReloadTime")?;
    let clip_time = gun.map_or(0.0, |gun| gun.intra_clip_time);
    let reload_left = gun_reload.map_or(0.0, |timer| timer.remaining(now));
    let base_time = gun_reload.map_or_else(
        || gun.map_or(0.0, |gun| gun.reload_time),
        |timer| timer.seconds,
    );
    let reload_time = dict(
        reload_ty,
        [
            ("baseTime", WireValue::Float32(base_time)),
            ("timeLeft", WireValue::Float32(reload_left)),
            ("endTime", WireValue::Float32(time + reload_left)),
            ("clipTime", WireValue::Float32(clip_time)),
        ],
    )?;
    let refill_left = clip_reload.map_or(0.0, |timer| timer.remaining(now));
    let first_round_time = gun
        .and_then(|gun| gun.autoreload_times.last())
        .copied()
        .unwrap_or(0.0);
    let clip_time_value = dict(
        clip_reload_ty,
        [
            (
                "baseTime",
                WireValue::Float32(clip_reload.map_or(0.0, |timer| timer.seconds)),
            ),
            ("timeLeft", WireValue::Float32(refill_left)),
            ("endTime", WireValue::Float32(time + refill_left)),
            ("firstTime", WireValue::Float32(first_round_time)),
            ("clipTime", WireValue::Float32(clip_time)),
        ],
    )?;
    let health_ty = property_type(component, "vehicleHealthInfo")?;
    let health = dict(
        health_ty,
        [
            ("id", id(health_ty, "id", vehicle_id)?),
            ("health", WireValue::Int16(tank.health)),
            (
                "deathReasonID",
                WireValue::Int8(if tank.health > 0 { -1 } else { 0 }),
            ),
            ("deathReasonExtID", WireValue::Int8(-1)),
            ("isCrewActive", WireValue::UInt8(u8::from(tank.health > 0))),
        ],
    )?;
    Ok(BTreeMap::from([
        ("vehicleAmmoList", WireValue::Seq(ammo)),
        ("vehicleSettings", WireValue::Seq(settings)),
        ("vehicleGunReloadTime", reload_time),
        ("vehicleClipReloadTime", clip_time_value),
        ("vehicleHealthInfo", health),
    ]))
}
