//! Training arena payloads encoded against the current resource bundle.

use super::{super::*, ArenaMap, Entrant};
use crate::platform::client_formats::util::io::WgWriteExt;
use crate::player_sessions::mercury::{
    app::{
        client::element::{SelectEntity, SetGameTime, arena::*},
        dispatch::{EntityDispatch, ScriptDispatch},
        entity::ScriptType,
    },
    net::codec::Codec,
    script::{PythonValue, StringValue, Ty, TyKind, Value as WireValue},
};
use glam::{Vec2, Vec3, Vec4};
use std::io;

mod controls;
mod roster;

pub(in crate::player_sessions::infrastructure::gateway) struct ArenaWire {
    avatar_type: u16,
    vehicle_type: u16,
    info_type: u16,
    roster_component: u16,
    roster_nrl_type: u16,
    own_vehicle_type: u16,
    own_vehicle_nrl_type: u16,
}

pub(super) struct ArenaEntry {
    pub(super) base_properties: WireValue,
    avatar_entity_id: u32,
    cell: CreateCellPlayer,
    own_vehicle: ArenaEntityProperties,
    own_component: NrlCreateNode,
    info: CreateArenaEntity,
    roster: NrlCreateNode,
}

impl ArenaWire {
    pub(in crate::player_sessions::infrastructure::gateway) fn bind(
        base: &mut base::App,
    ) -> Result<Self> {
        use crate::player_sessions::infrastructure::schema::Avatar;
        base.register::<Avatar>()
            .context("Bind training Avatar methods")?;
        let dispatch = base.dispatch();
        let type_id = |name: &str| {
            dispatch
                .entity_from_name(name)
                .map(|(id, _)| id)
                .with_context(|| format!("missing training entity or component {name}"))
        };
        let roster_component = type_id("AvatarVehiclesInfo")?;
        let own_vehicle_type = type_id("OwnVehicle")?;
        let schema = Self {
            avatar_type: type_id("Avatar")?,
            vehicle_type: type_id("Vehicle")?,
            info_type: type_id("AvatarInfo")?,
            roster_component,
            own_vehicle_type,
            own_vehicle_nrl_type: dispatch
                .component_nrl_id(own_vehicle_type)
                .context("OwnVehicle has no client replication type")?,
            roster_nrl_type: dispatch
                .component_nrl_id(roster_component)
                .context("AvatarVehiclesInfo has no client replication type")?,
        };
        let sample = Entrant::sample();
        schema.vehicle(dispatch, &sample)?;
        schema.validate_controls(dispatch)?;
        schema.entry(dispatch, 1, 1, &sample, &[&sample])?;
        Ok(schema)
    }

    pub(super) fn entry(
        &self,
        dispatch: &ScriptDispatch,
        space_id: u32,
        arena_type: i32,
        player: &Entrant,
        players: &[&Entrant],
    ) -> Result<ArenaEntry> {
        let avatar = dispatch
            .entity_from_id(self.avatar_type)
            .context("Missing Avatar")?;
        let base_properties = dict(
            &avatar.data_ty,
            [
                ("name", string(&player.name)),
                ("arenaUniqueID", WireValue::UInt64(u64::from(space_id))),
                ("arenaTypeID", WireValue::Int32(arena_type)),
                (
                    "arenaBonusType",
                    WireValue::UInt8(TRAINING_BONUS_TYPE as u8),
                ),
                ("arenaGuiType", WireValue::UInt8(TRAINING as u8)),
                (
                    "arenaExtraData",
                    python(crate::pickle!({"bonusCapsOverrides": {}, "battleLevel": 1})),
                ),
                (
                    "playLimits",
                    dict(
                        field(&avatar.data_ty, "playLimits")?,
                        [
                            ("curfew", WireValue::Int32(-1)),
                            ("weeklyPlayLimit", WireValue::Int32(-1)),
                            ("dailyPlayLimit", WireValue::Int32(-1)),
                            ("sessionLimit", WireValue::Int32(-1)),
                        ],
                    )?,
                ),
            ],
        )?;
        let mut properties = Vec::new();
        dict(
            &avatar.cell_data_ty,
            [
                ("team", WireValue::UInt8(player.team)),
                (
                    "playerVehicleID",
                    id(&avatar.cell_data_ty, "playerVehicleID", player.vehicle_id)?,
                ),
            ],
        )?
        .write(&mut properties, &avatar.cell_data_ty)?;
        properties.push(avatar.static_components_count);
        default_value(&avatar.cell_components_data_ty)
            .write(&mut properties, &avatar.cell_components_data_ty)?;
        let cell = CreateCellPlayer {
            space_id,
            vehicle_id: player.vehicle_id,
            position: player.position,
            direction: player.direction,
            properties,
        };
        let vehicle = dispatch
            .entity_from_id(self.vehicle_type)
            .context("Missing Vehicle")?;
        let own = properties_by_id(vehicle, [("isMyVehicle", WireValue::UInt8(1))])?;
        let info_schema = dispatch
            .entity_from_id(self.info_type)
            .context("Missing AvatarInfo")?;
        let avatar_id = info_schema
            .properties
            .iter()
            .find(|property| &*property.name == "avatarID")
            .context("AvatarInfo has no avatarID")?;
        let avatar_id = entity_id(&avatar_id.ty, player.avatar_id)?;
        let info = CreateArenaEntity {
            entity_id: player.info_id,
            type_id: self.info_type,
            position: Vec3::ZERO,
            direction: Vec3::ZERO,
            properties: properties_by_id(info_schema, [("avatarID", avatar_id)])?,
        };
        Ok(ArenaEntry {
            base_properties,
            avatar_entity_id: player.avatar_id,
            cell,
            own_vehicle: ArenaEntityProperties(own),
            own_component: self.own_component(dispatch, player)?,
            info,
            roster: self.roster(dispatch, player.info_id, players)?,
        })
    }

    pub(super) fn vehicle(
        &self,
        dispatch: &ScriptDispatch,
        player: &Entrant,
    ) -> Result<CreateArenaEntity> {
        let vehicle = dispatch
            .entity_from_id(self.vehicle_type)
            .context("Missing Vehicle")?;
        let property_type = |name| {
            vehicle
                .properties
                .iter()
                .find(|property| &*property.name == name)
                .map(|property| &property.ty)
                .with_context(|| format!("Vehicle has no {name}"))
        };
        let public = dict(
            property_type("publicInfo")?,
            [
                ("name", string(&player.name)),
                (
                    "compDescr",
                    bytes(
                        field(property_type("publicInfo")?, "compDescr")?,
                        &player.vehicle.descriptor,
                    ),
                ),
                ("team", WireValue::UInt8(player.team)),
                (
                    "maxHealth",
                    WireValue::UInt16(u16::try_from(player.vehicle.health)?),
                ),
                (
                    "crewGroups",
                    WireValue::Seq(vec![WireValue::UInt16(0); player.vehicle.crew_size]),
                ),
            ],
        )?;
        Ok(CreateArenaEntity {
            entity_id: player.vehicle_id,
            type_id: self.vehicle_type,
            position: player.position,
            direction: player.direction,
            properties: properties_by_id(
                vehicle,
                [
                    ("publicInfo", public),
                    ("health", WireValue::Int16(player.vehicle.health)),
                    (
                        "avatarID",
                        entity_id(property_type("avatarID")?, player.avatar_id)?,
                    ),
                    ("isCrewActive", WireValue::UInt8(1)),
                    ("gunAnglesPacked", WireValue::UInt16(0x8020)),
                ],
            )?,
        })
    }

    fn roster(
        &self,
        dispatch: &ScriptDispatch,
        info_id: u32,
        players: &[&Entrant],
    ) -> Result<NrlCreateNode> {
        let component = dispatch
            .entity_from_id(self.roster_component)
            .context("Missing roster component")?;
        let property = component
            .properties
            .iter()
            .find(|property| &*property.name == "vehiclesInfo")
            .context("Missing vehiclesInfo property")?;
        let TyKind::Array(rows) = unalias(&property.ty) else {
            anyhow::bail!("vehiclesInfo is not an array");
        };
        let roster = players
            .iter()
            .map(|player| {
                dict(
                    &rows.ty,
                    [
                        ("vehicleID", id(&rows.ty, "vehicleID", player.vehicle_id)?),
                        ("isAlive", WireValue::UInt8(1)),
                        (
                            "compDescr",
                            bytes(field(&rows.ty, "compDescr")?, &player.vehicle.descriptor),
                        ),
                        ("name", string(&player.name)),
                        ("fakeName", string(&player.name)),
                        ("team", WireValue::Int8(i8::try_from(player.team)?)),
                        (
                            "accountDBID",
                            WireValue::UInt64(u64::try_from(player.account_id)?),
                        ),
                        ("avatarSessionID", string(&player.avatar_id.to_string())),
                        ("maxHealth", WireValue::Int16(player.vehicle.health)),
                        ("badges", python(crate::pickle!(([], [])))),
                        ("ranked", WireValue::Seq(vec![WireValue::UInt16(0); 2])),
                        (
                            "position",
                            WireValue::Seq(vec![WireValue::Int16(i16::MIN); 2]),
                        ),
                        (
                            "isPrebattleCreator",
                            WireValue::UInt8(u8::from(player.is_host)),
                        ),
                    ],
                )
            })
            .collect::<Result<Vec<_>>>()?;
        component_tree(
            component,
            self.roster_nrl_type,
            info_id,
            roster::ROSTER_ENTITY_NODE,
            BTreeMap::from([("vehiclesInfo", WireValue::Seq(roster))]),
        )
    }
}

fn component_tree(
    component: &EntityDispatch,
    nrl_type: u16,
    entity_id: u32,
    entity_node: u16,
    values: BTreeMap<&str, WireValue>,
) -> Result<NrlCreateNode> {
    let component_node = entity_node
        .checked_add(1)
        .context("Invalid component node")?;
    let child_count = component.properties.len() + component.client_methods.len() + 1;
    anyhow::ensure!(
        child_count < usize::from(u16::MAX - component_node),
        "Too many component nodes"
    );
    let mut data = Vec::new();
    data.extend(entity_node.to_be_bytes());
    data.extend(1u16.to_be_bytes());
    data.push(1); // entity node
    data.write_u32(entity_id)?;
    data.push(0); // another create-node instruction
    data.extend(component_node.to_be_bytes());
    data.extend(entity_node.to_be_bytes());
    data.push(2); // dynamic component node
    data.write_u16(nrl_type)?;
    data.write_u8(0)?; // unnamed instance
    let mut mask = vec![0u8; child_count.div_ceil(8)];
    for index in 0..child_count {
        data.push(1);
        if component
            .properties
            .get(index)
            .is_none_or(|p| values.contains_key(&*p.name))
            && let Some(byte) = mask.get_mut(index / 8)
        {
            *byte |= 1 << (index % 8);
        }
    }
    data.push(0x82);
    data.extend(mask);
    for property in &component.properties {
        if let Some(value) = values.get(&*property.name) {
            anyhow::ensure!(
                property.ty.is_instance(value),
                "Invalid component property {}",
                property.name
            );
            value.write(&mut data, &property.ty)?;
        }
    }
    for index in 0..component.client_methods.len() {
        data.write_packed_u24(u32::try_from(index)?)?;
    }
    Ok(NrlCreateNode(data))
}

impl ArenaEntry {
    pub(super) fn publish(
        &self,
        base: &mut base::App,
        client: ClientKey,
        map: &ArenaMap,
        vehicles: impl Iterator<Item = CreateArenaEntity>,
        tick: u32,
    ) -> Result<()> {
        base.send_arena_element(client, SetGameTime { game_time: tick })?;
        base.send_arena_element(client, self.cell.clone())?;
        // Space metadata records observed alongside the geometry mapping.
        for record in [
            &[0, 0, 0, 0, 0, 1, 0, 0, 0][..],
            &[2, 0, 0, 0, 0, 0],
            &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        ] {
            let mut data = self.cell.space_id.to_le_bytes().to_vec();
            data.extend(record);
            base.send_arena_element(client, SpaceProperty(data))?;
        }
        let SocketAddr::V4(entry_id) = base.addr()? else {
            anyhow::bail!("training arena requires an IPv4 BaseApp");
        };
        base.send_arena_element(
            client,
            GeometryMapping {
                space_id: self.cell.space_id,
                entry_id: entry_id.into(),
                path: map.geometry.clone(),
            },
        )?;
        for vehicle in vehicles {
            let entity_id = vehicle.entity_id;
            publish_entity(base, client, vehicle)?;
            if entity_id == self.cell.vehicle_id {
                base.send_arena_element(client, SelectEntity { entity_id })?;
                base.send_arena_element(client, self.own_vehicle.clone())?;
            }
        }
        publish_entity(base, client, self.info.clone())?;
        base.send_arena_element(client, self.roster.clone())?;
        base.send_arena_element(client, self.own_component.clone())?;
        // CreateCellPlayer assumes local Avatar control and filters out server poses.
        base.send_arena_element(
            client,
            ControlEntity {
                entity_id: self.avatar_entity_id,
                is_locally_controlled: false,
            },
        )?;
        Ok(())
    }
}

fn publish_entity(
    base: &mut base::App,
    client: ClientKey,
    entity: CreateArenaEntity,
) -> io::Result<()> {
    base.send_arena_element(
        client,
        EnterArenaEntity {
            entity_id: entity.entity_id,
            alias: u8::MAX,
        },
    )?;
    base.send_arena_element(client, entity)
}

fn properties_by_id<const N: usize>(
    dispatch: &EntityDispatch,
    fields: [(&str, WireValue); N],
) -> Result<Vec<u8>> {
    let mut data = vec![u8::try_from(N)?];
    for (name, value) in fields {
        let (index, property) = dispatch
            .properties
            .iter()
            .enumerate()
            .find(|(_, property)| &*property.name == name)
            .with_context(|| format!("Missing arena property {name}"))?;
        anyhow::ensure!(
            property.ty.is_instance(&value),
            "Invalid type for arena property {name}"
        );
        data.push(u8::try_from(index)?);
        value
            .write(&mut data, &property.ty)
            .with_context(|| format!("Encode {name}"))?;
    }
    Ok(data)
}

fn dict<const N: usize>(ty: &Ty, overrides: [(&str, WireValue); N]) -> Result<WireValue> {
    let TyKind::Dict(schema) = unalias(ty) else {
        anyhow::bail!("{} must be a fixed dictionary", ty.name());
    };
    let mut fields = schema
        .properties
        .iter()
        .map(|property| {
            (
                property.name.clone(),
                property
                    .default
                    .clone()
                    .unwrap_or_else(|| default_value(&property.ty)),
            )
        })
        .collect::<BTreeMap<_, _>>();
    for (name, value) in overrides {
        let property = field(ty, name)?;
        anyhow::ensure!(
            property.is_instance(&value),
            "Invalid type for arena field {name}"
        );
        fields.insert(name.into(), value);
    }
    Ok(WireValue::Dict(fields))
}

fn field<'a>(ty: &'a Ty, name: &str) -> Result<&'a Ty> {
    let TyKind::Dict(schema) = unalias(ty) else {
        anyhow::bail!("{} must be a fixed dictionary", ty.name());
    };
    schema
        .properties
        .iter()
        .find(|p| &*p.name == name)
        .map(|p| &p.ty)
        .with_context(|| format!("Missing arena field {name}"))
}

fn id(ty: &Ty, name: &str, value: u32) -> Result<WireValue> {
    entity_id(field(ty, name)?, value)
}

fn entity_id(ty: &Ty, value: u32) -> Result<WireValue> {
    anyhow::ensure!(EntityId::check(ty), "Unsupported arena entity ID type");
    Ok(EntityId(value).into_value(ty))
}

fn bytes(ty: &Ty, value: &[u8]) -> WireValue {
    crate::player_sessions::mercury::app::entity::Bytes(value.to_vec()).into_value(ty)
}

fn string(value: &str) -> WireValue {
    WireValue::String(StringValue::String(value.to_owned()))
}

fn python(value: Value) -> WireValue {
    WireValue::Python(PythonValue::Decoded(value))
}

fn unalias(ty: &Ty) -> &TyKind {
    match ty.kind() {
        TyKind::Alias(inner) => unalias(inner),
        kind => kind,
    }
}

fn default_value(ty: &Ty) -> WireValue {
    match unalias(ty) {
        TyKind::Int8 => WireValue::Int8(0),
        TyKind::Int16 => WireValue::Int16(0),
        TyKind::Int32 => WireValue::Int32(0),
        TyKind::Int64 => WireValue::Int64(0),
        TyKind::UInt8 => WireValue::UInt8(0),
        TyKind::UInt16 => WireValue::UInt16(0),
        TyKind::UInt32 => WireValue::UInt32(0),
        TyKind::UInt64 => WireValue::UInt64(0),
        TyKind::Float32 => WireValue::Float32(0.0),
        TyKind::Float64 => WireValue::Float64(0.0),
        TyKind::Vector2 => WireValue::Vector2(Vec2::ZERO),
        TyKind::Vector3 => WireValue::Vector3(Vec3::ZERO),
        TyKind::Vector4 => WireValue::Vector4(Vec4::ZERO),
        TyKind::String => string(""),
        TyKind::Python => python(crate::pickle!({})),
        TyKind::Mailbox => WireValue::Mailbox,
        TyKind::Dict(schema) if schema.allow_none => WireValue::None,
        TyKind::Dict(schema) => WireValue::Dict(
            schema
                .properties
                .iter()
                .map(|property| {
                    (
                        property.name.clone(),
                        property
                            .default
                            .clone()
                            .unwrap_or_else(|| default_value(&property.ty)),
                    )
                })
                .collect(),
        ),
        TyKind::Array(sequence) | TyKind::Tuple(sequence) => WireValue::Seq(
            (0..sequence.size.unwrap_or_default())
                .map(|_| default_value(&sequence.ty))
                .collect(),
        ),
        TyKind::Alias(_) => unreachable!("alias resolved"),
    }
}
