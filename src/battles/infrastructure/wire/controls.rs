use super::*;
use crate::battles::domain::vehicle::{GunRay, SHOT_RANGE, Shot, Tank};
use crate::platform::mercury::{
    app::{
        client::element::{FixedEntityProperty, id::ENTITY_PROPERTY},
        dispatch::PropertyDef,
    },
    net::element::ElementLength,
};
use std::time::Instant;

const OWN_ENTITY_NODE: u16 = 1024;
const GUN_MARKER_COMPONENT_NODE: u16 = 2048;
const SHELL_SPEED: f32 = 450.0;
const DEFAULT_SHELL_KIND: u8 = 2;
const DEFAULT_EFFECTS_INDEX: u8 = 0;
const DEFAULT_PREFAB_EFFECTS_INDEX: u8 = 0;
const SHELL_CALIBER_MM: u16 = 45;
const HULL_PART_INDEX: u8 = 1;
const ARMOR_PIERCED_NO_DAMAGE: u8 = 0;
const ARMOR_PIERCED: u8 = 4;
type Fields = BTreeMap<&'static str, WireValue>;

#[derive(Clone, Copy)]
pub(in super::super) struct ShotImpact {
    attacker: u32,
    damage: i16,
    damage_factor: u8,
    segment: u64,
    params: u32,
}

impl ShotImpact {
    pub(in super::super) fn new(
        attacker: u32,
        damage: i16,
        max_health: i16,
        local_direction: Vec3,
    ) -> Self {
        let effect = if damage > 0 {
            ARMOR_PIERCED
        } else {
            ARMOR_PIERCED_NO_DAMAGE
        };
        // The training hit volume has no part mesh. Give the client a ray
        // through the center of its own hull bounding box for the impact effect.
        let reach = local_direction * (0.5 / local_direction.abs().max_element());
        let quantize = |point: Vec3| point.to_array().map(|axis| (axis * 255.0).round() as u8);
        let center = Vec3::splat(0.5);
        let start = quantize(center - reach);
        let end = quantize(center + reach);
        let segment = u64::from_le_bytes([
            effect,
            HULL_PART_INDEX,
            start[0],
            start[1],
            start[2],
            end[0],
            end[1],
            end[2],
        ]);
        Self {
            attacker,
            damage,
            damage_factor: (i32::from(damage) * 255 / i32::from(max_health.max(1))).clamp(0, 255)
                as u8,
            segment,
            params: ((u32::from(SHELL_CALIBER_MM) * 10) << 8)
                | (u32::from(DEFAULT_SHELL_KIND) << 4)
                | u32::from(effect),
        }
    }

    fn args(self) -> Vec<WireValue> {
        let hit_point = WireValue::Dict(BTreeMap::from([
            ("networkID".into(), WireValue::UInt32(0)),
            ("segment".into(), WireValue::UInt64(self.segment)),
            ("params".into(), WireValue::UInt32(self.params)),
        ]));
        vec![
            WireValue::Int32(self.attacker as i32),
            WireValue::Seq(vec![hit_point]),
            WireValue::UInt8(DEFAULT_EFFECTS_INDEX),
            WireValue::UInt8(DEFAULT_PREFAB_EFFECTS_INDEX),
            WireValue::Int32(i32::from(self.damage)),
            WireValue::UInt8(self.damage_factor),
            WireValue::UInt8(0),
            WireValue::Float32(SHELL_SPEED),
            WireValue::UInt8(0),
        ]
    }
}

impl ArenaWire {
    pub(super) fn bind_fixed_property(
        dispatch: &ScriptDispatch,
        entity_type: u16,
        name: &str,
        value: &WireValue,
        bytes: u32,
    ) -> Result<u8> {
        let properties = &dispatch
            .entity_from_id(entity_type)
            .context("Missing arena entity")?
            .properties;
        let (index, property) = properties
            .iter()
            .enumerate()
            .find(|(_, property)| &*property.name == name)
            .with_context(|| format!("Missing arena property {name}"))?;
        let (element, sub_id) = ENTITY_PROPERTY
            .from_exposed_id(u16::try_from(properties.len())?, u16::try_from(index)?);
        anyhow::ensure!(
            sub_id.is_none()
                && property.length == ElementLength::Fixed(bytes)
                && property.ty.is_instance(value),
            "Invalid arena property {name} framing"
        );
        Ok(element)
    }

    pub(in super::super) fn publish_gun_angles(
        &self,
        base: &mut base::App,
        client: ClientKey,
        entity_id: u32,
        value: u16,
    ) -> Result<()> {
        base.send_arena_property(
            client,
            Some(entity_id),
            FixedEntityProperty {
                element_id: self.gun_angles_element,
                bytes: value.to_le_bytes(),
            },
        )?;
        Ok(())
    }

    pub(in super::super) fn publish_hull_aim(
        &self,
        base: &mut base::App,
        client: ClientKey,
        direction: Vec3,
        speed: f32,
    ) -> Result<()> {
        base.send_arena_property(
            client,
            None,
            FixedEntityProperty {
                element_id: self.hull_aim_element,
                bytes: pack_hull_aim(direction, speed).to_le_bytes(),
            },
        )?;
        Ok(())
    }

    pub(super) fn gun_marker_component(&self, dispatch: &ScriptDispatch) -> Result<NrlCreateNode> {
        let (component, _, property) = self.gun_marker_property(dispatch)?;
        anyhow::ensure!(
            property.volatile && property.ty.is_instance(&WireValue::None),
            "Invalid GunMarkerComponent.gunMarker schema"
        );
        gun_marker_value(
            &property.ty,
            GunRay {
                origin: Vec3::ZERO,
                direction: Vec3::Z,
            },
            0.005,
        )?;
        let mut data = Vec::new();
        write_component_node(
            &mut data,
            component,
            self.gun_marker_nrl_type,
            OWN_ENTITY_NODE,
            GUN_MARKER_COMPONENT_NODE,
            BTreeMap::from([("gunMarker", WireValue::None)]),
        )?;
        Ok(NrlCreateNode(data))
    }

    pub(in super::super) fn publish_gun_marker(
        &self,
        base: &mut base::App,
        client: ClientKey,
        ray: GunRay,
        dispersion: f32,
    ) -> Result<()> {
        let (_, node, property) = self.gun_marker_property(base.dispatch())?;
        let value = gun_marker_value(&property.ty, ray, dispersion)?;
        let mut record = node.to_be_bytes().to_vec();
        value.write(&mut record, &property.ty)?;
        base.send_arena_element(client, NrlData(record))?;
        base.send_arena_element(client, ApplyNrlUpdate {})?;
        Ok(())
    }

    fn gun_marker_property<'a>(
        &self,
        dispatch: &'a ScriptDispatch,
    ) -> Result<(&'a EntityDispatch, u16, &'a PropertyDef)> {
        let component = dispatch
            .entity_from_id(self.gun_marker_type)
            .context("Missing GunMarkerComponent")?;
        let (index, property) = component
            .properties
            .iter()
            .enumerate()
            .find(|(_, property)| &*property.name == "gunMarker")
            .context("Missing GunMarkerComponent.gunMarker")?;
        let node = (GUN_MARKER_COMPONENT_NODE + 1)
            .checked_add(u16::try_from(index)?)
            .context("GunMarkerComponent node overflow")?;
        Ok((component, node, property))
    }

    pub(super) fn validate_controls(&self, dispatch: &ScriptDispatch) -> Result<()> {
        let marker_setting = dispatch
            .entity_from_id(self.avatar_type)
            .context("Missing Avatar")?
            .cell_methods
            .iter()
            .find(|method| &*method.name == "setServerMarker")
            .context("Missing Avatar.setServerMarker")?;
        anyhow::ensure!(
            matches!(marker_setting.args.as_slice(), [ty] if ty.is_instance(&WireValue::UInt8(1))),
            "Invalid Avatar.setServerMarker arguments"
        );
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
                "showDamageFromShot",
                ShotImpact::new(1, 1, 1, Vec3::Z).args(),
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

    pub(in super::super) fn show_tracer(
        &self,
        base: &mut base::App,
        avatar: EntityKey<Avatar>,
        shooter: u32,
        shot_id: i32,
        shot: &Shot,
        distance: Option<f32>,
    ) -> Result<()> {
        base.entity(avatar)?.showTracer(
            EntityId(shooter),
            shot_id,
            0,
            DEFAULT_EFFECTS_INDEX,
            DEFAULT_PREFAB_EFFECTS_INDEX,
            DEFAULT_SHELL_KIND,
            f32::from(SHELL_CALIBER_MM),
            shot.origin,
            shot.direction * SHELL_SPEED,
            0.0,
            distance.unwrap_or(SHOT_RANGE).max(0.1),
            0,
            0,
        )?;
        Ok(())
    }

    pub(in super::super) fn show_shot_impact(
        &self,
        base: &mut base::App,
        client: ClientKey,
        target: u32,
        impact: ShotImpact,
    ) -> Result<()> {
        base.arena_vehicle_method(client, target, "showDamageFromShot", impact.args())?;
        Ok(())
    }
}

pub(super) fn pack_hull_aim(direction: Vec3, speed: f32) -> u64 {
    use std::f32::consts::{FRAC_PI_2, TAU};
    let pack_angle = |angle: f32, bits: u32| {
        let slots = 1u64 << bits;
        ((angle / TAU + 0.5) * slots as f32).round() as u64 & (slots - 1u64)
    };
    let pitch = ((direction.y / FRAC_PI_2 * 0.5 + 0.5).clamp(0.0, 1.0) * 4095.0).round() as u64;
    let track = (((speed + 15.0) / 45.0).clamp(0.0, 1.0) * 255.0).round() as u64;
    let idle_rpm = 43u64;
    idle_rpm << 56
        | track << 48
        | track << 40
        | pack_angle(direction.x, 13) << 27
        | pitch << 15
        | pack_angle(direction.z, 15)
}

fn gun_marker_value(ty: &Ty, ray: GunRay, dispersion: f32) -> Result<WireValue> {
    dict(
        ty,
        [
            ("gunPosition", WireValue::Vector3(ray.origin)),
            (
                "shotVector",
                WireValue::Vector3(ray.direction * SHELL_SPEED),
            ),
            ("dispersion", WireValue::Float32(dispersion)),
        ],
    )
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
