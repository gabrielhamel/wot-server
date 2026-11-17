//! Client-to-BaseApp messages used during login and in the garage.

use std::io::{self, Read, Write};

use super::super::dispatch::{MethodCall, MethodDef};
use crate::client_formats::util::io::WgReadExt;
use crate::client_gateway::mercury::net::codec::SimpleCodec;
use crate::client_gateway::mercury::net::element::{
    DebugElementFixed, DebugElementVariable16, Element, ElementLength, SimpleElement,
};

pub mod id {

    use crate::client_gateway::mercury::net::element::ElementIdRange;

    // The ids below follow the registration order of `BaseAppExtInterface`'s
    // `Mercury::InterfaceMinder` (`baseapp_ext_interface.hpp`). Unlike
    // `client::element`'s `ClientInterface`, nothing in this interface uses
    // `CALLBACK`: every id here is either a fixed byte count or a
    // `Variable16`-style dynamic size.

    // --- Login/session handshake & datacenter ping ---
    pub const LOGIN_KEY: u8 = 0x00; // baseAppLogin, FIXED 7
    pub const PING_DATACENTER: u8 = 0x01; // pingDatacenter, FIXED 10
    pub const SESSION_KEY: u8 = 0x02; // authenticate, FIXED 4

    // --- Avatar updates & physics corrections (raw placeholders) ---
    pub const AVATAR_UPDATE_IMPLICIT: u8 = 0x03; // FIXED 17
    pub const AVATAR_UPDATE_EXPLICIT: u8 = 0x04; // FIXED 22
    pub const ACK_PHYSICS_CORRECTION: u8 = 0x05; // FIXED 0
    pub const REQUEST_ENTITY_UPDATE: u8 = 0x06; // VAR 2

    // --- Network Replication Layer ("NRL") ---
    pub const NRL_MSG_TO_CELL: u8 = 0x07; // VAR 2

    // --- Ward avatar updates & physics corrections ---
    pub const AVATAR_UPDATE_WARD_IMPLICIT: u8 = 0x08; // FIXED 20
    pub const AVATAR_UPDATE_WARD_EXPLICIT: u8 = 0x09; // FIXED 29
    pub const ACK_WARD_PHYSICS_CORRECTION: u8 = 0x0A; // FIXED 4

    // --- Session control & acks ---
    pub const ENABLE_ENTITIES: u8 = 0x0B; // FIXED 0
    pub const RESTORE_CLIENT_ACK: u8 = 0x0C; // FIXED 4
    pub const DISCONNECT_CLIENT: u8 = 0x0D; // FIXED 1
    pub const CLIENT_TO_SERVER_HEARTBEAT: u8 = 0x0E; // FIXED 0
    pub const SEND_TO_CELL: u8 = 0x0F; // FIXED 0

    // --- Dynamic entity method dispatch (base & cell) ---
    pub const CELL_ENTITY_METHOD: ElementIdRange = ElementIdRange::new(0x10, 0x87); // VAR 2 (all entries, not CALLBACK)
    pub const BASE_ENTITY_METHOD: ElementIdRange = ElementIdRange::new(0x88, 0xFE); // VAR 2 (all entries, not CALLBACK)
}

crate::__struct_simple_codec! {
    /// Sent by the client to the server without encryption in order to authenticate,
    /// the server then compares with its internal login keys from past successful
    /// logins on the login app.
    ///
    /// This element is usually a request, in such case a [`SessionKey`] must be sent as
    /// a reply, which is the server session key (not the same as login key).
    #[derive(Debug, Clone)]
    pub struct LoginKey {
        /// The login key that was sent by the login application, part of the  element
        /// [`super::super::login::element::LoginSuccess`].
        pub login_key: u32,
        /// The current number of attempts.
        pub attempt_num: u8,
        /// Unknown 16-bits value at the end.
        pub unk: u16,
    }
}

impl SimpleElement for LoginKey {
    const ID: u8 = id::LOGIN_KEY;
    const LEN: ElementLength = ElementLength::Fixed(7);
}

crate::__struct_simple_codec! {
    /// This element can be used in two cases:
    /// - As a reply to [`LoginKey`] from the server to the client in order to give it
    ///   the initial session key.
    /// - Sent by the client on login (and apparently randomly after login) to return
    ///   the session key that was sent by the server in the initial reply (first case).
    #[derive(Debug, Clone)]
    pub struct SessionKey {
        /// The server session key.
        pub session_key: u32,
    }
}

impl SimpleElement for SessionKey {
    const ID: u8 = id::SESSION_KEY;
    const LEN: ElementLength = ElementLength::Fixed(4);
}

crate::__struct_simple_codec! {
    /// This is sent by the client to the base application as an acknowledgment of a
    /// reset entity request sent to the client.
    #[derive(Debug, Clone)]
    pub struct EnableEntities {}
}

impl SimpleElement for EnableEntities {
    const ID: u8 = id::ENABLE_ENTITIES;
    const LEN: ElementLength = ElementLength::ZERO;
}

crate::__struct_simple_codec! {
    /// This is sent by the client to the base application as an acknowledgment of a
    /// reset entity request sent to the client.
    #[derive(Debug, Clone)]
    pub struct DisconnectClient {
        pub reason: u8,
    }
}

impl SimpleElement for DisconnectClient {
    const ID: u8 = id::DISCONNECT_CLIENT;
    const LEN: ElementLength = ElementLength::Fixed(1);
}

/// Sent by the client to report latency to various datacenters, used for
/// server/periphery selection.
pub type PingDatacenter = DebugElementFixed<{ id::PING_DATACENTER }, 10>;

pub type AvatarUpdateWardImplicit = DebugElementFixed<{ id::AVATAR_UPDATE_WARD_IMPLICIT }, 20>;
/// The [`AvatarUpdateExplicit`] form for a ward, stating the vehicle it is mounted on.
/// Relates to [`AvatarUpdateWardImplicit`] exactly as [`AvatarUpdateExplicit`] does to
/// [`AvatarUpdateImplicit`]. Body not decoded, and never seen on the wire.
pub type AvatarUpdateWardExplicit = DebugElementFixed<{ id::AVATAR_UPDATE_WARD_EXPLICIT }, 29>;
/// Acknowledges a server physics correction applied to a ward, the ward counterpart of
/// [`AckPhysicsCorrection`].
///
/// Four bytes, presumably the ward's entity id. Body not decoded, and never seen on the
/// wire.
pub type AckWardPhysicsCorrection = DebugElementFixed<{ id::ACK_WARD_PHYSICS_CORRECTION }, 4>;
/// Acknowledges a server restore request.
pub type RestoreClientAck = DebugElementFixed<{ id::RESTORE_CLIENT_ACK }, 4>;

/// Empty heartbeat sent by the client.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClientToServerHeartbeat;

impl SimpleCodec for ClientToServerHeartbeat {
    fn write(&self, _write: &mut dyn Write) -> io::Result<()> {
        Ok(())
    }
    fn read(_read: &mut dyn Read) -> io::Result<Self> {
        Ok(Self)
    }
}

impl SimpleElement for ClientToServerHeartbeat {
    const ID: u8 = id::CLIENT_TO_SERVER_HEARTBEAT;
    const LEN: ElementLength = ElementLength::ZERO;
}

/// Marks the bundle as destined for the cell rather than the base. Payload-less, exactly
/// as [`ClientToServerHeartbeat`].
#[derive(Debug, Clone, Copy, Default)]
pub struct SendToCell;

impl SimpleCodec for SendToCell {
    fn write(&self, _write: &mut dyn Write) -> io::Result<()> {
        Ok(())
    }
    fn read(_read: &mut dyn Read) -> io::Result<Self> {
        Ok(Self)
    }
}

impl SimpleElement for SendToCell {
    const ID: u8 = id::SEND_TO_CELL;
    const LEN: ElementLength = ElementLength::ZERO;
}

#[derive(Debug, Clone)]
pub struct BaseEntityMethod {
    pub call: MethodCall,
}

impl Element<[MethodDef]> for BaseEntityMethod {
    fn write_length(&self, _config: &[MethodDef]) -> io::Result<ElementLength> {
        unreachable!("BaseEntityMethod is read-only")
    }

    fn write(&self, _write: &mut dyn Write, _config: &[MethodDef]) -> io::Result<u8> {
        unreachable!("BaseEntityMethod is read-only")
    }

    fn read_length(_config: &[MethodDef], _id: u8) -> io::Result<ElementLength> {
        Ok(ElementLength::Variable16)
    }

    fn read(read: &mut dyn Read, config: &[MethodDef], len: usize, id: u8) -> io::Result<Self> {
        if !id::BASE_ENTITY_METHOD.contains(id) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unexpected base entity method element id: {id:02X}"),
            ));
        }

        let mut len = len;
        let mut sub_id_err = None;
        let exposed_id = id::BASE_ENTITY_METHOD.to_exposed_id(config.len() as u16, id, || {
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

pub type AvatarUpdateImplicit = DebugElementFixed<{ id::AVATAR_UPDATE_IMPLICIT }, 17>;
pub type AvatarUpdateExplicit = DebugElementFixed<{ id::AVATAR_UPDATE_EXPLICIT }, 22>;
pub type AckPhysicsCorrection = DebugElementFixed<{ id::ACK_PHYSICS_CORRECTION }, 0>;
pub type RequestEntityUpdate = DebugElementVariable16<{ id::REQUEST_ENTITY_UPDATE }>;
pub type NrlMsgToCell = DebugElementVariable16<{ id::NRL_MSG_TO_CELL }>;
