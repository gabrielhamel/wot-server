use super::*;
use crate::mercury::net::codec::WgSocketAddrV4;
use glam::Vec3;

crate::__struct_simple_codec! {
    #[derive(Debug, Clone)]
    pub struct ResetEntities {
        pub keep_player_on_base: bool,
    }
}

impl SimpleElement for ResetEntities {
    const ID: u8 = 0x04;
    const LEN: ElementLength = ElementLength::Fixed(1);
}

#[derive(Debug, Clone)]
pub struct CreateCellPlayer {
    pub space_id: u32,
    pub vehicle_id: u32,
    pub position: Vec3,
    pub direction: Vec3,
    pub properties: Vec<u8>,
}

impl SimpleCodec for CreateCellPlayer {
    fn write(&self, out: &mut dyn Write) -> io::Result<()> {
        out.write_u8(0)?;
        out.write_u32(self.space_id)?;
        out.write_u16(0)?;
        out.write_u32(self.vehicle_id)?;
        out.write_vec3(self.position)?;
        out.write_f32(0.007644442)?;
        out.write_vec3(self.direction)?;
        out.write_all(&self.properties)
    }

    fn read(input: &mut dyn Read) -> io::Result<Self> {
        let _ = input.read_u8()?;
        let space_id = input.read_u32()?;
        let _ = input.read_u16()?;
        let vehicle_id = input.read_u32()?;
        let position = input.read_vec3()?;
        let _ = input.read_f32()?;
        let direction = input.read_vec3()?;
        let mut properties = Vec::new();
        input.read_to_end(&mut properties)?;
        Ok(Self {
            space_id,
            vehicle_id,
            position,
            direction,
            properties,
        })
    }
}

impl SimpleElement for CreateCellPlayer {
    const ID: u8 = 0x06;
    const LEN: ElementLength = ElementLength::Variable16;
}

#[derive(Debug, Clone)]
pub struct GeometryMapping {
    pub space_id: u32,
    pub entry_id: WgSocketAddrV4,
    pub path: String,
}

impl SimpleCodec for GeometryMapping {
    fn write(&self, out: &mut dyn Write) -> io::Result<()> {
        out.write_u32(self.space_id)?;
        SimpleCodec::write(&self.entry_id, out)?;
        out.write_string_variable(&self.path)?;
        for value in glam::Mat4::IDENTITY.to_cols_array() {
            out.write_f32(value)?;
        }
        out.write_u8(0)
    }

    fn read(input: &mut dyn Read) -> io::Result<Self> {
        let space_id = input.read_u32()?;
        let entry_id = <WgSocketAddrV4 as SimpleCodec>::read(input)?;
        let path = input.read_string_variable()?;
        input.read_exact(&mut [0; 65])?;
        Ok(Self {
            space_id,
            entry_id,
            path,
        })
    }
}

impl SimpleElement for GeometryMapping {
    const ID: u8 = 0x09;
    const LEN: ElementLength = ElementLength::Variable16;
}

#[derive(Debug, Clone)]
pub struct CreateArenaEntity {
    pub entity_id: u32,
    pub type_id: u16,
    pub position: Vec3,
    pub direction: Vec3,
    pub properties: Vec<u8>,
}

impl SimpleCodec for CreateArenaEntity {
    fn write(&self, out: &mut dyn Write) -> io::Result<()> {
        out.write_u8(0)?;
        out.write_u32(self.entity_id)?;
        out.write_u16(self.type_id)?;
        out.write_u16(0)?;
        out.write_string_variable("bw.default")?;
        out.write_vec3(self.position)?;
        out.write_vec3(self.direction)?;
        out.write_all(&self.properties)
    }

    fn read(input: &mut dyn Read) -> io::Result<Self> {
        if input.read_u8()? != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "expected uncompressed entity",
            ));
        }
        let entity_id = input.read_u32()?;
        let type_id = input.read_u16()?;
        let _ = input.read_u16()?;
        let _ = input.read_string_variable()?;
        let position = input.read_vec3()?;
        let direction = input.read_vec3()?;
        let mut properties = Vec::new();
        input.read_to_end(&mut properties)?;
        Ok(Self {
            entity_id,
            type_id,
            position,
            direction,
            properties,
        })
    }
}

impl SimpleElement for CreateArenaEntity {
    const ID: u8 = 0x0c;
    const LEN: ElementLength = ElementLength::Variable16;
}

crate::__struct_simple_codec! {
    #[derive(Debug, Clone)]
    pub struct EnterArenaEntity {
        pub entity_id: u32,
        pub alias: u8,
    }
}

impl SimpleElement for EnterArenaEntity {
    const ID: u8 = 0x10;
    const LEN: ElementLength = ElementLength::Fixed(5);
}

crate::__struct_simple_codec! {
    #[derive(Debug, Clone)]
    pub struct LeaveArenaEntity {
        pub entity_id: u32,
    }
}

impl SimpleElement for LeaveArenaEntity {
    const ID: u8 = 0x12;
    const LEN: ElementLength = ElementLength::Variable16;
}

macro_rules! arena_blob {
    ($name:ident, $id:literal) => {
        #[derive(Debug, Clone)]
        pub struct $name(pub Vec<u8>);

        impl SimpleCodec for $name {
            fn write(&self, out: &mut dyn Write) -> io::Result<()> {
                out.write_all(&self.0)
            }

            fn read(input: &mut dyn Read) -> io::Result<Self> {
                let mut data = Vec::new();
                input.read_to_end(&mut data)?;
                Ok(Self(data))
            }
        }

        impl SimpleElement for $name {
            const ID: u8 = $id;
            const LEN: ElementLength = ElementLength::Variable16;
        }
    };
}

arena_blob!(SpaceProperty, 0x08);
arena_blob!(NrlCreateNode, 0x21);
arena_blob!(ArenaEntityProperties, 0x4b);

crate::__struct_simple_codec! {
    #[derive(Debug, Clone)]
    pub struct VehiclePosition {
        pub reference: u8,
        pub entity_id: u32,
        pub position: Vec3,
        pub direction: Vec3,
    }
}

impl SimpleElement for VehiclePosition {
    const ID: u8 = 0x1c;
    const LEN: ElementLength = ElementLength::Fixed(29);
}

arena_blob!(NrlData, 0x26);

crate::__struct_simple_codec! {
    #[derive(Debug, Clone)]
    pub struct ApplyNrlUnlink {}
}

impl SimpleElement for ApplyNrlUnlink {
    const ID: u8 = 0x24;
    const LEN: ElementLength = ElementLength::ZERO;
}

crate::__struct_simple_codec! {
    #[derive(Debug, Clone)]
    pub struct ApplyNrlUpdate {}
}

impl SimpleElement for ApplyNrlUpdate {
    const ID: u8 = 0x25;
    const LEN: ElementLength = ElementLength::ZERO;
}

crate::__struct_simple_codec! {
    #[derive(Debug, Clone)]
    pub struct AvatarPosition {
        pub reference: u8,
        pub position: Vec3,
        pub direction: Vec3,
    }
}

impl SimpleElement for AvatarPosition {
    const ID: u8 = 0x1e;
    const LEN: ElementLength = ElementLength::Fixed(25);
}

crate::__struct_simple_codec! {
    #[derive(Debug, Clone)]
    pub struct ControlEntity {
        pub entity_id: u32,
        pub is_locally_controlled: bool,
    }
}

impl SimpleElement for ControlEntity {
    const ID: u8 = 0x41;
    const LEN: ElementLength = ElementLength::Fixed(5);
}
