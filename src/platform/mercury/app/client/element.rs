//! BaseApp-to-client messages used during login and in the garage.

use std::{
    borrow::Cow,
    fmt,
    io::{self, Read, Write},
};

use tracing::warn;

use super::super::dispatch::{EntityDispatch, MethodCall, MethodDef, ScriptDispatch};
use crate::platform::client_formats::util::{
    AsciiFmt,
    io::{WgReadExt, WgWriteExt},
};
use crate::platform::mercury::net::codec::{Codec, SimpleCodec};
use crate::platform::mercury::net::element::{Element, ElementLength, SimpleElement};
use crate::platform::mercury::script::{Ty, Value};

pub mod arena;

pub mod id {
    use crate::platform::mercury::net::element::ElementIdRange;

    pub const UPDATE_FREQUENCY_NOTIFICATION: u8 = 0x02;
    pub const SET_GAME_TIME: u8 = 0x03;
    pub const CREATE_BASE_PLAYER: u8 = 0x05;
    pub const TICK_SYNC: u8 = 0x13;
    pub const SELECT_ENTITY: u8 = 0x19;
    pub const SELECT_PLAYER_ENTITY: u8 = 0x1A;
    pub const RESOURCE_HEADER: u8 = 0x45;
    pub const RESOURCE_FRAGMENT: u8 = 0x46;
    pub const ENTITY_METHOD: ElementIdRange = ElementIdRange::new(0x4E, 0xA6);
    pub const ENTITY_PROPERTY: ElementIdRange = ElementIdRange::new(0xA7, 0xFE);
}

/// A fixed-size property update addressed to the selected entity.
pub struct FixedEntityProperty<const N: usize> {
    pub element_id: u8,
    pub bytes: [u8; N],
}

impl<const N: usize> Element<()> for FixedEntityProperty<N> {
    fn write_length(&self, config: &()) -> io::Result<ElementLength> {
        Self::read_length(config, self.element_id)
    }

    fn write(&self, write: &mut dyn Write, _config: &()) -> io::Result<u8> {
        write.write_all(&self.bytes)?;
        Ok(self.element_id)
    }

    fn read_length(_config: &(), id: u8) -> io::Result<ElementLength> {
        if id::ENTITY_PROPERTY.contains(id) {
            Ok(ElementLength::Fixed(
                u32::try_from(N).map_err(io::Error::other)?,
            ))
        } else {
            Err(io::ErrorKind::InvalidData.into())
        }
    }

    fn read(read: &mut dyn Read, _config: &(), _len: usize, id: u8) -> io::Result<Self> {
        let mut bytes = [0; N];
        read.read_exact(&mut bytes)?;
        Ok(Self {
            element_id: id,
            bytes,
        })
    }
}

/// A variable-size property update addressed to the selected entity.
pub struct VariableEntityProperty {
    pub element_id: u8,
    pub bytes: Vec<u8>,
}

impl Element<()> for VariableEntityProperty {
    fn write_length(&self, config: &()) -> io::Result<ElementLength> {
        Self::read_length(config, self.element_id)
    }

    fn write(&self, write: &mut dyn Write, _config: &()) -> io::Result<u8> {
        write.write_all(&self.bytes)?;
        Ok(self.element_id)
    }

    fn read_length(_config: &(), id: u8) -> io::Result<ElementLength> {
        if id::ENTITY_PROPERTY.contains(id) {
            Ok(ElementLength::Variable8)
        } else {
            Err(io::ErrorKind::InvalidData.into())
        }
    }

    fn read(read: &mut dyn Read, _config: &(), len: usize, id: u8) -> io::Result<Self> {
        if len > 256 {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let mut bytes = vec![0; len];
        read.read_exact(&mut bytes)?;
        Ok(Self {
            element_id: id,
            bytes,
        })
    }
}

crate::__struct_simple_codec! {
    /// The server informs us how frequently it is going to send update
    /// the the client, and also give the server game time (exactly the
    /// same as [`SetGameTime`] element, but inlined here).
    #[derive(Debug, Clone)]
    pub struct UpdateFrequencyNotification {
        /// The frequency in hertz.
        pub frequency: u8,
        /// Unknown value!
        pub unknown: u16,
        /// The server game time.
        pub game_time: u32,
    }
}

impl SimpleElement for UpdateFrequencyNotification {
    const ID: u8 = id::UPDATE_FREQUENCY_NOTIFICATION;
    const LEN: ElementLength = ElementLength::Fixed(7);
}

crate::__struct_simple_codec! {
    /// The server informs us of the current (server) game time.
    #[derive(Debug, Clone)]
    pub struct SetGameTime {
        /// The server game time.
        pub game_time: u32,
    }
}

impl SimpleElement for SetGameTime {
    const ID: u8 = id::SET_GAME_TIME;
    const LEN: ElementLength = ElementLength::Fixed(4);
}

#[derive(Debug, Clone)]
pub struct CreateBasePlayer<'a> {
    /// The unique identifier of the entity being created.
    pub entity_id: u32,
    /// The entity type id.
    pub entity_type_id: u16,
    /// The actual data sent for creating the player's entity -- borrowed when writing
    /// (the caller already owns it), owned when read off the wire.
    pub entity_data: Cow<'a, Value>,
    /// The number of the entity type's static components, those whose `<ofEntity>`
    /// names it: 19 for an `Account`, 2 for an `Avatar`, as captured. No component data
    /// follows the count: the element ends right after it.
    pub entity_components_count: u8,
}

impl CreateBasePlayer<'_> {
    /// Read everything but `entity_data` (which needs a resolved [`Ty`] the two
    /// [`Element`] impls below each get differently), returning it alongside the
    /// remaining trailer so each impl only has to plug in its own `data_ty` resolution.
    fn read_prefix(read: &mut dyn Read) -> io::Result<(u32, u16)> {
        let entity_id = read.read_u32()?;
        let entity_type_id = read.read_u16()?;
        let unk = read.read_blob_variable()?;
        if !unk.is_empty() {
            warn!("non-empty unknown blob when decoding CreateBasePlayer: {unk:?}");
        }
        Ok((entity_id, entity_type_id))
    }

    fn read_suffix(
        read: &mut dyn Read,
        entity_id: u32,
        entity_type_id: u16,
        data_ty: &Ty,
    ) -> io::Result<Self> {
        let entity_data = Value::read(read, data_ty)?;
        let entity_components_count = read.read_u8()?;
        // Nothing follows a nonzero count (see `entity_components_count`), so leftover
        // bytes would mean the layout above does not fit this payload.
        if entity_components_count != 0 {
            let mut trailer = Vec::new();
            read.read_to_end(&mut trailer)?;
            if !trailer.is_empty() {
                warn!(
                    "CreateBasePlayer: entity_id={entity_id} entity_components_count={entity_components_count}, \
                    {} unexpected trailing bytes: {trailer:?}",
                    trailer.len()
                );
            }
            return Ok(Self {
                entity_id,
                entity_type_id,
                entity_data: Cow::Owned(entity_data),
                entity_components_count,
            });
        }
        Ok(Self {
            entity_id,
            entity_type_id,
            entity_data: Cow::Owned(entity_data),
            entity_components_count,
        })
    }

    fn write_to(&self, write: &mut dyn Write, data_ty: &Ty) -> io::Result<u8> {
        write.write_u32(self.entity_id)?;
        write.write_u16(self.entity_type_id)?;
        write.write_blob_variable(&[])?; // Unknown blob or string?
        Codec::write(&*self.entity_data, write, data_ty)?;
        write.write_u8(self.entity_components_count)?;
        Ok(id::CREATE_BASE_PLAYER)
    }
}

impl Element<ScriptDispatch> for CreateBasePlayer<'_> {
    fn write_length(&self, _config: &ScriptDispatch) -> io::Result<ElementLength> {
        Ok(ElementLength::Variable16)
    }

    fn write(&self, write: &mut dyn Write, config: &ScriptDispatch) -> io::Result<u8> {
        let dispatch = config.entity_from_id(self.entity_type_id).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown entity type id: 0x{:02X}", self.entity_type_id),
            )
        })?;
        self.write_to(write, &dispatch.data_ty)
    }

    fn read_length(_config: &ScriptDispatch, _id: u8) -> io::Result<ElementLength> {
        Ok(ElementLength::Variable16)
    }

    fn read(
        read: &mut dyn Read,
        config: &ScriptDispatch,
        _len: usize,
        _id: u8,
    ) -> io::Result<Self> {
        let (entity_id, entity_type_id) = Self::read_prefix(read)?;
        let dispatch = config.entity_from_id(entity_type_id).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown entity type id: 0x{entity_type_id:02X}"),
            )
        })?;
        Self::read_suffix(read, entity_id, entity_type_id, &dispatch.data_ty)
    }
}

impl Element<EntityDispatch> for CreateBasePlayer<'_> {
    fn write_length(&self, _config: &EntityDispatch) -> io::Result<ElementLength> {
        Ok(ElementLength::Variable16)
    }

    fn write(&self, write: &mut dyn Write, config: &EntityDispatch) -> io::Result<u8> {
        self.write_to(write, &config.data_ty)
    }

    fn read_length(_config: &EntityDispatch, _id: u8) -> io::Result<ElementLength> {
        unreachable!("CreateBasePlayer<EntityDispatch> is write-only")
    }

    fn read(
        _read: &mut dyn Read,
        _config: &EntityDispatch,
        _len: usize,
        _id: u8,
    ) -> io::Result<Self> {
        unreachable!("CreateBasePlayer<EntityDispatch> is write-only")
    }
}

crate::__struct_simple_codec! {
    /// It is used as a timestamp for the elements in a bundle.
    #[derive(Debug, Clone)]
    pub struct TickSync {
        pub tick: u8,
    }
}

impl SimpleElement for TickSync {
    const ID: u8 = id::TICK_SYNC;
    const LEN: ElementLength = ElementLength::Fixed(1);
}

crate::__struct_simple_codec! {
    /// Select a client entity by its full identifier.
    #[derive(Debug, Clone, Copy)]
    pub struct SelectEntity {
        pub entity_id: u32,
    }
}

impl SimpleElement for SelectEntity {
    const ID: u8 = id::SELECT_ENTITY;
    const LEN: ElementLength = ElementLength::Fixed(4);
}

crate::__struct_simple_codec! {
    /// Sent by the server to inform that subsequent elements will target
    /// the player entity.
    #[derive(Debug, Default, Clone, Copy)]
    pub struct SelectPlayerEntity {}
}

impl SimpleElement for SelectPlayerEntity {
    const ID: u8 = id::SELECT_PLAYER_ENTITY;
    const LEN: ElementLength = ElementLength::Fixed(0);
}

/// Header describing a resource that will be downloaded in possibly many fragments.
#[derive(Clone)]
pub struct ResourceHeader {
    pub id: u16,
    pub description: Vec<u8>,
}

impl SimpleCodec for ResourceHeader {
    fn write(&self, write: &mut dyn Write) -> io::Result<()> {
        write.write_u16(self.id)?;
        write.write_blob_variable(&self.description)?;
        Ok(())
    }

    fn read(read: &mut dyn Read) -> io::Result<Self> {
        Ok(Self {
            id: read.read_u16()?,
            description: read.read_blob_variable()?,
        })
    }
}

impl SimpleElement for ResourceHeader {
    const ID: u8 = id::RESOURCE_HEADER;
    const LEN: ElementLength = ElementLength::Variable16;
}

impl fmt::Debug for ResourceHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResourceHeader")
            .field("id", &self.id)
            .field("description", &AsciiFmt(&self.description))
            .finish()
    }
}

/// Header describing a resource that will be downloaded in possibly many fragments.
#[derive(Clone)]
pub struct ResourceFragment {
    pub id: u16,
    pub sequence_num: u8,
    pub last: bool,
    pub data: Vec<u8>,
}

impl SimpleCodec for ResourceFragment {
    fn write(&self, write: &mut dyn Write) -> io::Result<()> {
        write.write_u16(self.id)?;
        write.write_u8(self.sequence_num)?;
        write.write_bool(self.last)?;
        write.write_blob(&self.data)?;
        Ok(())
    }

    fn read(read: &mut dyn Read) -> io::Result<Self> {
        Ok(Self {
            id: read.read_u16()?,
            sequence_num: read.read_u8()?,
            last: read.read_bool()?,
            data: read.read_blob_to_end()?,
        })
    }
}

impl SimpleElement for ResourceFragment {
    const ID: u8 = id::RESOURCE_FRAGMENT;
    const LEN: ElementLength = ElementLength::Variable16;
}

impl fmt::Debug for ResourceFragment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResourceFragment")
            .field("id", &self.id)
            .field("sequence_num", &self.sequence_num)
            .field("last", &self.last)
            .field("data", &AsciiFmt(&self.data))
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct EntityMethod {
    pub call: MethodCall,
}

/// The method a known call's exposed id names in the table it is written against.
fn known_method(config: &[MethodDef], exposed_id: u16) -> io::Result<&MethodDef> {
    config.get(exposed_id as usize).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("no method of exposed id {exposed_id}"),
        )
    })
}

impl Element<[MethodDef]> for EntityMethod {
    fn write_length(&self, config: &[MethodDef]) -> io::Result<ElementLength> {
        let (exposed_id, preferred_len) = match &self.call {
            MethodCall::Known { exposed_id, .. } => {
                (*exposed_id, known_method(config, *exposed_id)?.length)
            }
            MethodCall::Unknown { exposed_id, .. } => (*exposed_id, ElementLength::Variable8),
        };

        // A sub-id is written as an extra byte ahead of the method's own payload (see
        // `write` below), so the preferred length only applies to full-slot ids; ids
        // requiring a sub-id always frame as Variable16, matching `read_length` below.
        let (_, sub_id) = id::ENTITY_METHOD.from_exposed_id(config.len() as u16, exposed_id);
        Ok(if sub_id.is_some() {
            ElementLength::Variable16
        } else {
            preferred_len
        })
    }

    fn write(&self, write: &mut dyn Write, config: &[MethodDef]) -> io::Result<u8> {
        let exposed_id = match &self.call {
            MethodCall::Known { exposed_id, .. } | MethodCall::Unknown { exposed_id, .. } => {
                *exposed_id
            }
        };

        let (element_id, sub_id) =
            id::ENTITY_METHOD.from_exposed_id(config.len() as u16, exposed_id);
        if let Some(sub_id) = sub_id {
            write.write_u8(sub_id)?;
        }

        match &self.call {
            MethodCall::Known { args, .. } => {
                known_method(config, exposed_id)?.write_args(write, args)?
            }
            MethodCall::Unknown { data, .. } => write.write_all(data)?,
        }

        Ok(element_id)
    }

    fn read_length(config: &[MethodDef], id: u8) -> io::Result<ElementLength> {
        if !id::ENTITY_METHOD.contains(id) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unexpected entity method element id: {id:02X}"),
            ));
        }

        Ok(
            match id::ENTITY_METHOD.to_exposed_id_checked(config.len() as u16, id) {
                // An unrecognized exposed id falls back to Variable8 here instead of
                // erroring -- matching `getEntityMethodStreamSize` on a
                // live client instance, which returns Mercury's
                // `DEFAULT_VARIABLE_LENGTH_HEADER_SIZE` sentinel (-1, i.e. "read 1 more
                // header byte") for an id it doesn't recognize either.
                Some(exposed_id) => config
                    .get(exposed_id as usize)
                    .map(|def| def.length)
                    .unwrap_or(ElementLength::Variable8),
                // A sub-id slot: the actual exposed id (and so its preferred length) can only
                // be known once the sub-id byte prefixing the payload has been read, so the
                // whole payload is always Variable16-framed instead.
                None => ElementLength::Variable16,
            },
        )
    }

    fn read(read: &mut dyn Read, config: &[MethodDef], len: usize, id: u8) -> io::Result<Self> {
        if !id::ENTITY_METHOD.contains(id) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unexpected entity method element id: {id:02X}"),
            ));
        }

        let mut len = len;
        let mut sub_id_err = None;
        let exposed_id = id::ENTITY_METHOD.to_exposed_id(config.len() as u16, id, || {
            len = len.saturating_sub(1);
            match read.read_u8() {
                Ok(n) => n,
                Err(e) => {
                    sub_id_err = Some(e);
                    0 // Unused, we bail out right after via sub_id_err.
                }
            }
        });
        if let Some(e) = sub_id_err {
            return Err(e);
        }

        let call = match config.get(exposed_id as usize) {
            Some(def) => MethodCall::Known {
                exposed_id,
                name: def.name.clone(),
                args: def.read_args(read)?,
            },
            None => {
                let mut data = vec![0; len];
                read.read_exact(&mut data)?;
                MethodCall::Unknown { exposed_id, data }
            }
        };

        Ok(Self { call })
    }
}
