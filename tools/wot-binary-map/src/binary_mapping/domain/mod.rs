use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Investigation {
    pub format_version: u32,
    pub executable: String,
    pub sha256: String,
    pub image_base: u64,
    pub registration_function: u64,
    pub interfaces: Vec<Interface>,
    pub native_types: Vec<NativeType>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Interface {
    pub name: String,
    pub address: u64,
    pub messages: Vec<Message>,
}

#[derive(Debug, Serialize)]
pub struct Message {
    pub name: String,
    pub first_id: u8,
    pub last_id: u8,
    pub framing: Framing,
    pub handler: u64,
    pub registration: u64,
    pub initializer_slot: u64,
    pub range_registration: Option<u64>,
    pub callback_length: Option<CallbackLength>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Framing {
    Fixed { bytes: u32 },
    Variable { prefix_bytes: u32 },
    Callback,
}

#[derive(Debug, Serialize)]
pub struct NativeType {
    pub decorated_name: String,
    pub name: String,
    pub descriptor: u64,
    pub bases: Vec<BaseClass>,
    pub vtables: Vec<Vtable>,
}

#[derive(Debug, Serialize)]
pub struct BaseClass {
    pub descriptor: u64,
    pub name: String,
    pub contained_bases: u32,
    pub member_displacement: i32,
    pub vbtable_displacement: i32,
    pub displacement_inside_vbtable: i32,
    pub attributes: u32,
}

#[derive(Debug, Serialize)]
pub struct Vtable {
    pub address: u64,
    pub locator: u64,
    pub object_offset: u32,
    pub constructor_displacement: u32,
    pub functions: Vec<u64>,
}

#[derive(Debug, Serialize)]
pub struct CallbackLength {
    pub bytes: u32,
    pub function: u64,
}
