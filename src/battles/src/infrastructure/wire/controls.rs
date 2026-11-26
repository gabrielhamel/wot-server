use super::*;
use crate::domain::{
    armor::{ArmorOutcome, ImpactSegment},
    devices::{TRACKS, TrackEvent},
    vehicle::{GunRay, Shot, Tank},
};
use std::{collections::VecDeque, time::Instant};
use wot_platform::mercury::{
    app::{
        client::element::{FixedEntityProperty, VariableEntityProperty, id::ENTITY_PROPERTY},
        dispatch::PropertyDef,
    },
    net::element::ElementLength,
};
use wot_shared_kernel::ShellEffects;

const OWN_ENTITY_NODE: u16 = 1024;
const GUN_MARKER_COMPONENT_NODE: u16 = 2048;
type Fields = BTreeMap<&'static str, WireValue>;

// One byte for the array count and 16 bytes per hit point in a Variable8 property.
const MAX_DAMAGE_STICKERS: usize = 15;

#[derive(Clone, Copy)]
struct DamageSticker {
    segment: u64,
    params: u32,
}

#[derive(Default)]
pub(in super::super) struct DamageStickers(VecDeque<DamageSticker>);

impl DamageStickers {
    pub(in super::super) fn add(&mut self, impact: ShotImpact) -> bool {
        let Some(sticker) = impact.sticker() else {
            return false;
        };
        if self
            .0
            .iter()
            .any(|previous| previous.segment == sticker.segment)
        {
            return false;
        }
        if self.0.len() == MAX_DAMAGE_STICKERS {
            self.0.pop_front();
        }
        self.0.push_back(sticker);
        true
    }

    pub(in super::super) fn len(&self) -> usize {
        self.0.len()
    }

    pub(in super::super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn value(&self) -> WireValue {
        WireValue::Seq(
            self.0
                .iter()
                .map(|sticker| hit_point(sticker.segment, sticker.params))
                .collect(),
        )
    }
}

fn hit_point(segment: u64, params: u32) -> WireValue {
    WireValue::Dict(BTreeMap::from([
        ("networkID".into(), WireValue::UInt32(0)),
        ("segment".into(), WireValue::UInt64(segment)),
        ("params".into(), WireValue::UInt32(params)),
    ]))
}

#[derive(Clone, Copy)]
pub(in super::super) struct ShotImpact {
    attacker: u32,
    damage: i16,
    damage_factor: u8,
    segment: u64,
    params: u32,
    effects: ShellEffects,
    outcome: ArmorOutcome,
    speed: f32,
}

impl ShotImpact {
    pub(in super::super) fn new(
        attacker: u32,
        damage: i16,
        max_health: i16,
        contact: ImpactSegment,
        outcome: ArmorOutcome,
        effects: ShellEffects,
        speed: f32,
    ) -> Self {
        let effect = match outcome {
            ArmorOutcome::Penetrated if damage > 0 => 4,
            ArmorOutcome::Ricochet => 2,
            ArmorOutcome::Blocked | ArmorOutcome::ScreenBlocked | ArmorOutcome::Spall => 3,
            _ => 0,
        };
        let [ax, ay, az] = contact.start;
        let [bx, by, bz] = contact.end;
        let segment = u64::from_le_bytes([effect, contact.part, ax, ay, az, bx, by, bz]);
        Self {
            attacker,
            damage,
            damage_factor: (i32::from(damage) * 255 / i32::from(max_health.max(1))).clamp(0, 255)
                as u8,
            segment,
            params: (u32::from((effects.caliber_mm * 10.0) as u16) << 8)
                | (u32::from(effects.kind) << 4)
                | u32::from(effect),
            effects,
            outcome,
            speed,
        }
    }

    fn sticker(self) -> Option<DamageSticker> {
        let id = match self.outcome {
            ArmorOutcome::Penetrated if self.damage > 0 => self.effects.pierced_sticker,
            ArmorOutcome::Ricochet | ArmorOutcome::Blocked | ArmorOutcome::ScreenBlocked => {
                self.effects.resisted_sticker
            }
            _ => None,
        }?;
        Some(DamageSticker {
            segment: (self.segment & !0xff) | u64::from(id),
            params: self.params | (u32::from(self.effects.prefab_index) << 24),
        })
    }

    fn args(self) -> Vec<WireValue> {
        vec![
            WireValue::Int32(self.attacker as i32),
            WireValue::Seq(vec![hit_point(self.segment, self.params)]),
            WireValue::UInt8(self.effects.legacy_index),
            WireValue::UInt8(self.effects.prefab_index),
            WireValue::Int32(i32::from(self.damage)),
            WireValue::UInt8(self.damage_factor),
            WireValue::UInt8(u8::from(matches!(
                self.outcome,
                ArmorOutcome::ScreenPenetrated | ArmorOutcome::ScreenBlocked
            ))),
            WireValue::Float32(self.speed),
            WireValue::UInt8(0),
        ]
    }
}

pub(super) fn track_extras(resources: &ResourceBundle) -> Result<[u8; 2]> {
    let definition = wot_platform::client_formats::pxml::from_reader(BufReader::new(
        resources.read("scripts/item_defs/vehicles/common/vehicle.xml")?,
    ))?;
    let extras = definition
        .get_child_element("extras")
        .context("Missing vehicle extras")?;
    let mut indices = [None; 2];
    for (index, (name, value)) in extras.iter_children_all().enumerate() {
        let side = match name.as_str() {
            "leftTrack0Health" => 0,
            "rightTrack0Health" => 1,
            _ => continue,
        };
        let class = value
            .as_element()
            .context("Missing track extra definition")?;
        anyhow::ensure!(
            class
                .value
                .as_string()
                .and_then(|class| class.split(':').next())
                .map(str::trim)
                == Some("TrackHealth"),
            "Unexpected class for {name}"
        );
        // The client reserves index zero for its implicit NoneExtra.
        indices[side] = Some(u8::try_from(index + 1)?);
    }
    Ok([
        indices[0].context("Missing left track extra")?,
        indices[1].context("Missing right track extra")?,
    ])
}

impl ArenaWire {
    pub(super) fn bind_fixed_property(
        dispatch: &ScriptDispatch,
        entity_type: u16,
        name: &str,
        value: &WireValue,
        bytes: u32,
    ) -> Result<u8> {
        Self::bind_property(
            dispatch,
            entity_type,
            name,
            value,
            ElementLength::Fixed(bytes),
        )
    }

    pub(super) fn bind_wheel_property(
        dispatch: &ScriptDispatch,
        entity_type: u16,
        name: &str,
    ) -> Result<u8> {
        Self::bind_property(
            dispatch,
            entity_type,
            name,
            &WireValue::Seq(vec![WireValue::UInt8(127)]),
            ElementLength::Variable8,
        )
    }

    fn bind_property(
        dispatch: &ScriptDispatch,
        entity_type: u16,
        name: &str,
        value: &WireValue,
        length: ElementLength,
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
            sub_id.is_none() && property.length == length && property.ty.is_instance(value),
            "Invalid arena property {name} framing"
        );
        Ok(element)
    }

    pub(in super::super) fn publish_gun_angles(
        &self,
        delivery: &mut impl ArenaDelivery,
        client: ParticipantId,
        entity_id: u32,
        value: u16,
    ) -> Result<()> {
        delivery.send_arena_property(
            client,
            Some(entity_id),
            self.gun_angles_element,
            FixedEntityProperty {
                element_id: self.gun_angles_element,
                bytes: value.to_le_bytes(),
            },
        )?;
        Ok(())
    }

    pub(in super::super) fn publish_hull_aim(
        &self,
        delivery: &mut impl ArenaDelivery,
        client: ParticipantId,
        direction: Vec3,
        tracks: [f32; 2],
        rpm: f32,
    ) -> Result<()> {
        delivery.send_arena_property(
            client,
            None,
            self.hull_aim_element,
            FixedEntityProperty {
                element_id: self.hull_aim_element,
                bytes: pack_hull_aim(direction, tracks, rpm).to_le_bytes(),
            },
        )?;
        Ok(())
    }

    pub(in super::super) fn publish_engine_mode(
        &self,
        delivery: &mut impl ArenaDelivery,
        client: ParticipantId,
        entity_id: u32,
        mode: [u8; 2],
    ) -> Result<()> {
        delivery.send_arena_property(
            client,
            Some(entity_id),
            self.engine_mode_element,
            FixedEntityProperty {
                element_id: self.engine_mode_element,
                bytes: mode,
            },
        )?;
        Ok(())
    }

    pub(in super::super) fn publish_wheels(
        &self,
        delivery: &mut impl ArenaDelivery,
        client: ParticipantId,
        entity_id: u32,
        steering: bool,
        values: &[u8],
    ) -> Result<()> {
        let element_id = if steering {
            self.wheels_steering_element
        } else {
            self.wheels_scroll_element
        };
        let mut bytes = Vec::with_capacity(values.len() + 1);
        bytes.write_blob_variable(values)?;
        delivery.send_arena_property(
            client,
            Some(entity_id),
            element_id,
            VariableEntityProperty { element_id, bytes },
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
                speed: crate::contract::BattleParameters::default().fallback_gun_ray_speed_mps,
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
        delivery: &mut impl ArenaDelivery,
        client: ParticipantId,
        ray: GunRay,
        dispersion: f32,
    ) -> Result<()> {
        let (_, node, property) = self.gun_marker_property(&self.dispatch)?;
        let value = gun_marker_value(&property.ty, ray, dispersion)?;
        let mut record = node.to_be_bytes().to_vec();
        value.write(&mut record, &property.ty)?;
        delivery.send_arena_element(client, NrlData(record))?;
        delivery.send_arena_element(client, ApplyNrlUpdate {})?;
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

    pub(super) fn validate_controls(&self) -> Result<()> {
        let dispatch = &self.dispatch;
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
        let own = dispatch
            .entity_from_id(self.own_vehicle_type)
            .context("Missing OwnVehicle")?;
        for (name, fields) in [
            (
                "vehicleDamageInfoList",
                [
                    ("extraIndex", WireValue::UInt8(1)),
                    ("damageIndex", WireValue::UInt8(4)),
                    ("entityID", WireValue::Int32(0)),
                    ("equipmentID", WireValue::UInt16(0)),
                    ("damageExtIndex", WireValue::Int8(-1)),
                ],
            ),
            (
                "destroyedDevicesIsRepairing",
                [
                    ("extraIndex", WireValue::UInt8(1)),
                    ("progress", WireValue::UInt8(0)),
                    ("endTime", WireValue::Float32(1.0)),
                    ("timeLeft", WireValue::Float32(1.0)),
                    ("repairMode", WireValue::UInt8(0)),
                ],
            ),
        ] {
            dict(rows(property_type(own, name)?)?, fields)?;
        }
        let vehicle = dispatch
            .entity_from_id(self.vehicle_type)
            .context("Missing Vehicle")?;
        properties_by_id(
            vehicle,
            [(
                "publicStateModifiers",
                WireValue::Seq(self.track_extras.map(WireValue::UInt8).to_vec()),
            )],
        )?;
        let methods = &vehicle.client_methods;
        for (name, args) in [
            (
                "showShooting",
                vec![WireValue::UInt8(1), WireValue::Int8(0), WireValue::UInt8(0)],
            ),
            (
                "showDamageFromShot",
                ShotImpact::new(
                    1,
                    1,
                    1,
                    ImpactSegment {
                        part: 1,
                        start: [128, 128, 0],
                        end: [128, 128, 255],
                    },
                    ArmorOutcome::Penetrated,
                    ShellEffects::default(),
                    crate::contract::BattleParameters::default().fallback_gun_ray_speed_mps,
                )
                .args(),
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
        for name in ["vehicleDamageInfoList", "destroyedDevicesIsRepairing"] {
            fields.insert(name, WireValue::Seq(Vec::new()));
        }
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
        delivery: &mut impl ArenaDelivery,
        client: ParticipantId,
        vehicle_id: u32,
        tank: &Tank,
        now: Instant,
        time: f32,
    ) -> Result<()> {
        let component = self
            .dispatch
            .entity_from_id(self.own_vehicle_type)
            .context("Missing OwnVehicle")?;
        let values = own_state(component, vehicle_id, tank, now, time)?;
        self.publish_own_properties(delivery, client, component, values)
    }

    fn publish_own_properties(
        &self,
        delivery: &mut impl ArenaDelivery,
        client: ParticipantId,
        component: &EntityDispatch,
        mut values: Fields,
    ) -> Result<()> {
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
            delivery.send_arena_element(client, NrlData(record))?;
            delivery.send_arena_element(client, ApplyNrlUpdate {})?;
        }
        Ok(())
    }

    pub(in super::super) fn publish_broken_tracks(
        &self,
        delivery: &mut impl ArenaDelivery,
        client: ParticipantId,
        vehicle: u32,
        tank: &Tank,
    ) -> Result<()> {
        let extras = TRACKS
            .into_iter()
            .zip(self.track_extras)
            .filter(|(track, _)| tank.condition.disabled(*track))
            .map(|(_, extra)| WireValue::UInt8(extra))
            .collect();
        self.vehicle_properties(
            delivery,
            client,
            vehicle,
            [("publicStateModifiers", WireValue::Seq(extras))],
        )
    }

    pub(in super::super) fn publish_track_damage(
        &self,
        delivery: &mut impl ArenaDelivery,
        client: ParticipantId,
        tank: &Tank,
        events: [Option<TrackEvent>; 2],
        attacker: u32,
        clock: (Instant, f32),
    ) -> Result<()> {
        let component = self
            .dispatch
            .entity_from_id(self.own_vehicle_type)
            .context("Missing OwnVehicle")?;
        let damage = rows(property_type(component, "vehicleDamageInfoList")?)?;
        let repairs = rows(property_type(component, "destroyedDevicesIsRepairing")?)?;
        let mut damage_rows = Vec::new();
        let mut repair_rows = Vec::new();
        let (now, time) = clock;
        for ((track, extra), event) in TRACKS.into_iter().zip(self.track_extras).zip(events) {
            if let Some(event) = event {
                damage_rows.push(dict(
                    damage,
                    [
                        ("extraIndex", WireValue::UInt8(extra)),
                        (
                            "damageIndex",
                            WireValue::UInt8(match event {
                                TrackEvent::Damaged => 3,
                                TrackEvent::Broken => 4,
                                TrackEvent::RepairedCritical => 15,
                                TrackEvent::Repaired => 16,
                            }),
                        ),
                        ("entityID", id(damage, "entityID", attacker)?),
                        ("equipmentID", WireValue::UInt16(0)),
                        ("damageExtIndex", WireValue::Int8(-1)),
                    ],
                )?);
            }
            if let Some((started, ends)) = tank.condition.track_repair(track) {
                let left = ends.saturating_duration_since(now).as_secs_f32();
                let total = ends.saturating_duration_since(started).as_secs_f32();
                let progress = ((1.0 - left / total.max(f32::EPSILON)) * 100.0) as u8;
                repair_rows.push(dict(
                    repairs,
                    [
                        ("extraIndex", WireValue::UInt8(extra)),
                        ("progress", WireValue::UInt8(progress)),
                        ("endTime", WireValue::Float32(time + left)),
                        ("timeLeft", WireValue::Float32(left)),
                        ("repairMode", WireValue::UInt8(0)),
                    ],
                )?);
            }
        }
        self.publish_own_properties(
            delivery,
            client,
            component,
            BTreeMap::from([("vehicleDamageInfoList", WireValue::Seq(damage_rows))]),
        )?;
        self.publish_own_properties(
            delivery,
            client,
            component,
            BTreeMap::from([("destroyedDevicesIsRepairing", WireValue::Seq(repair_rows))]),
        )
    }

    pub(in super::super) fn vehicle_properties<const N: usize>(
        &self,
        delivery: &mut impl ArenaDelivery,
        client: ParticipantId,
        entity_id: u32,
        fields: [(&str, WireValue); N],
    ) -> Result<()> {
        let schema = self
            .dispatch
            .entity_from_id(self.vehicle_type)
            .context("Missing Vehicle")?;
        let update = properties_by_id(schema, fields)?;
        delivery.send_arena_element(client, SelectEntity { entity_id })?;
        delivery.send_arena_element(client, ArenaEntityProperties(update))?;
        Ok(())
    }

    pub(in super::super) fn show_tracer(
        &self,
        delivery: &mut impl ArenaDelivery,
        client: ParticipantId,
        shooter: u32,
        shot_id: i32,
        shot: &Shot,
    ) -> Result<()> {
        delivery.send_avatar(
            client,
            AvatarUpdate::Tracer {
                shooter,
                ricochet: shot.ricocheted,
                id: shot_id,
                effects: shot.effects,
                start: shot.origin,
                velocity: shot.direction * shot.ballistics.speed,
                gravity: shot.ballistics.gravity,
                range: shot.max_distance,
            },
        )?;
        Ok(())
    }

    pub(in super::super) fn show_scene_impact(
        &self,
        delivery: &mut impl ArenaDelivery,
        client: ParticipantId,
        shot_id: i32,
        shot: &Shot,
        distance: f32,
        (material, destructibles): (Option<u32>, &[u32]),
    ) -> Result<()> {
        delivery.send_avatar(
            client,
            AvatarUpdate::Impact {
                id: shot_id,
                effects: shot.effects,
                material: self.impact_materials.effect_index(material),
                endpoint: shot.origin + shot.direction * distance,
                direction: shot.direction,
                speed: shot.ballistics.speed,
                destructibles: destructibles.to_vec(),
            },
        )?;
        Ok(())
    }

    #[tracing::instrument(name = "battles.damage_stickers", skip_all, fields(
        ddd.bounded_context.name = "battles", input.participant.id = client.0,
        input.entity_id = entity_id, input.damage_stickers.count = stickers.len()
    ), err)]
    pub(in super::super) fn publish_damage_stickers(
        &self,
        delivery: &mut impl ArenaDelivery,
        client: ParticipantId,
        entity_id: u32,
        stickers: &DamageStickers,
    ) -> Result<()> {
        let vehicle = self
            .dispatch
            .entity_from_id(self.vehicle_type)
            .context("Missing Vehicle")?;
        let ty = property_type(vehicle, "damageStickers")?;
        let mut bytes = Vec::new();
        stickers.value().write(&mut bytes, ty)?;
        delivery.send_arena_property(
            client,
            Some(entity_id),
            self.damage_stickers_element,
            VariableEntityProperty {
                element_id: self.damage_stickers_element,
                bytes,
            },
        )?;
        Ok(())
    }

    pub(super) fn bind_damage_stickers(dispatch: &ScriptDispatch, vehicle_type: u16) -> Result<u8> {
        Self::bind_property(
            dispatch,
            vehicle_type,
            "damageStickers",
            &WireValue::Seq(vec![hit_point(0, 0)]),
            ElementLength::Variable8,
        )
    }

    pub(in super::super) fn show_shot_impact(
        &self,
        delivery: &mut impl ArenaDelivery,
        client: ParticipantId,
        target: u32,
        impact: ShotImpact,
    ) -> Result<()> {
        delivery.arena_vehicle_method(client, target, "showDamageFromShot", impact.args())?;
        Ok(())
    }
}

pub(super) fn pack_hull_aim(direction: Vec3, tracks: [f32; 2], rpm: f32) -> u64 {
    use std::f32::consts::{FRAC_PI_2, TAU};
    let pack_angle = |angle: f32, bits: u32| {
        let slots = 1u64 << bits;
        ((angle / TAU + 0.5) * slots as f32).round() as u64 & (slots - 1u64)
    };
    let pitch = ((direction.y / FRAC_PI_2 * 0.5 + 0.5).clamp(0.0, 1.0) * 4095.0).round() as u64;
    let [left, right] =
        tracks.map(|speed| (((speed + 15.0) / 45.0).clamp(0.0, 1.0) * 255.0).round() as u64);
    let rpm = ((rpm / 1.2).clamp(0.0, 1.0) * 255.0).round() as u64;
    rpm << 56
        | right << 48
        | left << 40
        | pack_angle(direction.x, 13) << 27
        | pitch << 15
        | pack_angle(direction.z, 15)
}

pub(in super::super) fn pack_wheel_speed(speed: f32) -> u8 {
    (speed * 127.5 / 25.0 + 127.0).round().clamp(0.0, 255.0) as u8
}

pub(in super::super) fn pack_wheel_steering(angle: f32) -> u8 {
    (angle * 255.0 / std::f32::consts::PI + 127.5)
        .round()
        .clamp(0.0, 255.0) as u8
}

fn gun_marker_value(ty: &Ty, ray: GunRay, dispersion: f32) -> Result<WireValue> {
    dict(
        ty,
        [
            ("gunPosition", WireValue::Vector3(ray.origin)),
            ("shotVector", WireValue::Vector3(ray.direction * ray.speed)),
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
        .with_context(|| format!("Missing property {name}"))
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

#[cfg(test)]
mod impact_tests {
    use super::*;

    fn penetration(x: u8, damage: i16, effects: ShellEffects) -> ShotImpact {
        ShotImpact::new(
            1,
            damage,
            10,
            ImpactSegment {
                part: 1,
                start: [x, 128, 0],
                end: [x, 128, 255],
            },
            ArmorOutcome::Penetrated,
            effects,
            crate::contract::BattleParameters::default().fallback_gun_ray_speed_mps,
        )
    }

    #[test]
    fn retains_distinct_penetrations_up_to_the_wire_limit() {
        let mut marks = DamageStickers::default();
        let mut effects = ShellEffects::default();
        let impact = penetration(128, 1, effects);
        assert!(!marks.add(impact));
        effects.pierced_sticker = Some(0);
        assert!(!marks.add(penetration(128, 0, effects)));

        for index in 0..=MAX_DAMAGE_STICKERS {
            let impact = penetration(128 + index as u8, 1, effects);
            assert!(marks.add(impact));
            assert!(!marks.add(impact));
        }
        assert_eq!(marks.len(), MAX_DAMAGE_STICKERS);
        let first = penetration(128, 1, effects);
        assert!(marks.add(first));
        assert_eq!(marks.len(), MAX_DAMAGE_STICKERS);
    }
}
