//! Providing an bundle-oriented socket, backed by an UDP socket.

use std::collections::HashMap;
use std::io::{self, Cursor};
use std::net::{SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::Duration;

use blowfish::Blowfish;
use tracing::trace;

use super::bundle::Bundle;
use super::filter::{BlowfishReader, BlowfishWriter, blowfish::BLOCK_SIZE};
use super::packet::{self, Packet};

/// Encryption magic, 0xDEADBEEF in little endian.
const ENCRYPTION_MAGIC: [u8; 4] = 0xDEADBEEFu32.to_le_bytes();
/// Encryption footer length, 1 byte for wastage count + 4 bytes magic.
const ENCRYPTION_FOOTER_LEN: usize = ENCRYPTION_MAGIC.len() + 1;

/// UDP socket with per-session Blowfish keys.
#[derive(Debug)]
pub struct PacketSocket {
    socket: UdpSocket,
    encryption: HashMap<SocketAddr, Arc<Blowfish>>,
}

impl PacketSocket {
    pub fn bind(addr: SocketAddr) -> io::Result<Self> {
        Ok(Self {
            socket: UdpSocket::bind(addr)?,
            encryption: HashMap::new(),
        })
    }

    pub fn addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    pub fn set_recv_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.socket.set_read_timeout(timeout)
    }

    pub fn set_encryption(&mut self, addr: SocketAddr, key: Arc<Blowfish>) {
        self.encryption.insert(addr, key);
    }

    pub fn remove_encryption(&mut self, addr: SocketAddr) {
        self.encryption.remove(&addr);
    }

    pub fn recv(&self) -> io::Result<(Packet, SocketAddr)> {
        let mut datagram = [0; packet::PACKET_CAP + 1];
        let (len, addr) = self.socket.recv_from(&mut datagram)?;
        if !(packet::PACKET_HEADER_LEN..=packet::PACKET_CAP).contains(&len) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid UDP packet size",
            ));
        }

        let mut packet = Packet::new();
        packet.buf_mut()[..len].copy_from_slice(&datagram[..len]);
        packet.set_len(len);
        if let Some(key) = self.encryption.get(&addr) {
            packet = decrypt_packet(packet, key)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid encryption"))?;
        }
        Ok((packet, addr))
    }

    pub fn send(&self, packet: &Packet, addr: SocketAddr) -> io::Result<usize> {
        if let Some(key) = self.encryption.get(&addr) {
            let mut encrypted = encryption_packet::take();
            let result = encrypt_packet_raw(packet, key, &mut encrypted)
                .and_then(|()| self.socket.send_to(encrypted.slice(), addr));
            encryption_packet::put(encrypted);
            result
        } else {
            self.socket.send_to(packet.slice(), addr)
        }
    }

    pub fn send_bundle_without_encryption(
        &self,
        bundle: &Bundle,
        addr: SocketAddr,
    ) -> io::Result<usize> {
        bundle.iter().try_fold(0, |total, packet| {
            self.socket
                .send_to(packet.slice(), addr)
                .map(|sent| total + sent)
        })
    }

    pub fn send_bundle(&self, bundle: &Bundle, addr: SocketAddr) -> io::Result<usize> {
        bundle.iter().try_fold(0, |total, packet| {
            self.send(packet, addr).map(|sent| total + sent)
        })
    }
}

/// Decrypt a packet of a given length with a blowfish key. Note that the destination
/// packet will be completely erased, so the inner data is not relevant.
fn decrypt_packet_raw(src_packet: &Packet, bf: &Blowfish, dst_packet: &mut Packet) -> bool {
    let len = src_packet.len();

    dst_packet.set_len(len);

    // Decrypt the incoming packet into the new clear packet.
    // We don't need to set the length yet because this packet will be synchronized just
    // after. We don't encrypt the prefix.
    let src = &src_packet.slice()[packet::PACKET_PREFIX_LEN..];
    let dst = &mut dst_packet.slice_mut()[packet::PACKET_PREFIX_LEN..];

    // Note that src and dst have the same length, thanks to blowfish encryption.
    // Then we can already check the length and ensures that it is a multiple of
    // blowfish block size *and* can contain the wastage and encryption magic.
    if !src.len().is_multiple_of(BLOCK_SIZE) {
        trace!(
            "Invalid source body length: {}, block size: {BLOCK_SIZE}",
            src.len()
        );
        return false;
    } else if src.len() < ENCRYPTION_FOOTER_LEN {
        trace!(
            "Invalid source body length: {}, min len: {ENCRYPTION_FOOTER_LEN}",
            src.len()
        );
        return false;
    }

    if io::copy(
        &mut BlowfishReader::new(Cursor::new(src), bf),
        &mut Cursor::new(&mut *dst),
    )
    .ok()
        != Some(src.len() as u64)
    {
        return false;
    }

    let wastage_begin = src.len() - 1;
    let magic_begin = wastage_begin - 4;

    // Check invalid magic.
    if dst[magic_begin..wastage_begin] != ENCRYPTION_MAGIC {
        trace!(
            "Invalid destination packet magic: {:X}, expected: {:X}",
            crate::client_formats::util::BytesFmt(&dst[magic_begin..wastage_begin]),
            crate::client_formats::util::BytesFmt(&ENCRYPTION_MAGIC)
        );
        return false;
    }

    // Get the wastage count and compute the packet's length.
    // Note that wastage count also it self length.
    let wastage = dst[wastage_begin];
    if !(1..=BLOCK_SIZE as u8).contains(&wastage) {
        return false;
    }
    let Some(clear_len) = len.checked_sub(wastage as usize + ENCRYPTION_MAGIC.len()) else {
        return false;
    };
    if clear_len < packet::PACKET_HEADER_LEN {
        return false;
    }
    dst_packet.set_len(clear_len);
    // Copy the prefix directly because it is clear.
    dst_packet.write_prefix(src_packet.read_prefix());

    true
}

/// Encrypt source packet with the given blowfish key and write it to the destination
/// raw packet. Everything except the packet prefix is encrypted, and the destination
/// packet will have a size that is a multiple of blowfish's block size (8). The clear
/// data is also padded to block size, but with additional data at the end: encryption
/// signature (0xDEADBEEF in little endian) and the wastage count + 1 on the last byte.
fn encrypt_packet_raw(
    src_packet: &Packet,
    bf: &Blowfish,
    dst_packet: &mut Packet,
) -> io::Result<()> {
    // Get the minimum, unpadded length of this packet with encryption footer appended to it.
    let mut len = src_packet.len() - packet::PACKET_PREFIX_LEN + ENCRYPTION_FOOTER_LEN;

    // The wastage amount is basically the padding + 1 for the wastage itself.
    let padding = (BLOCK_SIZE - (len % BLOCK_SIZE)) % BLOCK_SIZE;
    len += padding;
    let encrypted_len = len + packet::PACKET_PREFIX_LEN;
    if encrypted_len > packet::PACKET_CAP {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "packet exceeds encrypted capacity",
        ));
    }

    // Clone the packet data into a new vec and append the padding and the footer.
    let mut clear_data = src_packet.slice()[packet::PACKET_PREFIX_LEN..].to_vec();
    clear_data.reserve_exact(padding + ENCRYPTION_FOOTER_LEN);
    clear_data.extend_from_slice(&[0u8; BLOCK_SIZE - 1][..padding]); // Padding
    clear_data.extend_from_slice(&ENCRYPTION_MAGIC); // Magic
    clear_data.push(padding as u8 + 1); // Wastage count (+1 for it self size)

    debug_assert_eq!(clear_data.len(), len, "incoherent length");
    debug_assert_eq!(clear_data.len() % 8, 0, "data not padded as expected");

    // +4 for the prefix.
    dst_packet.set_len(encrypted_len);

    io::copy(
        &mut Cursor::new(&clear_data[..]),
        &mut BlowfishWriter::new(
            Cursor::new(&mut dst_packet.slice_mut()[packet::PACKET_PREFIX_LEN..]),
            bf,
        ),
    )?;

    // Copy the prefix directly because it is clear.
    dst_packet.write_prefix(src_packet.read_prefix());
    Ok(())
}

/// Decrypt a source packet given a blowfish key, return the clear packet if success,
/// if the decryption fails it return the source packet not touched.
pub fn decrypt_packet(src_packet: Packet, bf: &Blowfish) -> Result<Packet, Packet> {
    let mut dst_packet = encryption_packet::take();
    if decrypt_packet_raw(&src_packet, bf, &mut dst_packet) {
        encryption_packet::put(src_packet);
        Ok(dst_packet)
    } else {
        encryption_packet::put(dst_packet);
        Err(src_packet)
    }
}

/// Encrypt a source packet given a blowfish key, return the encrypted packet.
pub fn encrypt_packet(src_packet: &Packet, bf: &Blowfish) -> io::Result<Packet> {
    let mut dst_packet = encryption_packet::take();
    encrypt_packet_raw(src_packet, bf, &mut dst_packet)?;
    Ok(dst_packet)
}

/// Internal module to isolate implementation detail, the goal is just to avoid wasting
/// already allocated packets by keeping an encryption packet around.
mod encryption_packet {

    use crate::client_gateway::mercury::net::packet::Packet;
    use std::cell::Cell;

    thread_local! {
        static ENCRYPTION_PACKET: Cell<Option<Packet>> = const { Cell::new(None) };
    }

    pub fn take() -> Packet {
        ENCRYPTION_PACKET.take().unwrap_or_default()
    }

    pub fn put(mut packet: Packet) {
        packet.reset();
        ENCRYPTION_PACKET.set(Some(packet));
    }
}
