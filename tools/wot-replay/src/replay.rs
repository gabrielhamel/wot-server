use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;

use anyhow::{Context, Result, ensure};
use blowfish::Blowfish;
use blowfish::cipher::{Block, BlockDecrypt, KeyInit};
use flate2::{Decompress, FlushDecompress, Status};
use serde::{Deserialize, Serialize};

const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;
const MAX_STREAM_BYTES: usize = 256 * 1024 * 1024;
const MAGIC: u32 = 0x1134_3212;
const REPLAY_KEY: [u8; 16] = [
    0xde, 0x72, 0xbe, 0xa0, 0xde, 0x04, 0xbe, 0xb1, 0xde, 0xfe, 0xbe, 0xef, 0xde, 0xad, 0xbe, 0xef,
];

pub const POSITION: u32 = 0x0a;

pub struct Replay {
    pub metadata: Metadata,
    stream: Vec<u8>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    client_version_from_xml: Option<String>,
    client_version_from_exe: Option<String>,
    region_code: Option<String>,
    server_name: Option<String>,
    date_time: Option<String>,
    map_name: Option<String>,
    player_vehicle: Option<String>,
}

impl Replay {
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = Vec::new();
        File::open(path)?
            .take(MAX_FILE_BYTES as u64 + 1)
            .read_to_end(&mut file)?;
        ensure!(
            file.len() <= MAX_FILE_BYTES,
            "replay exceeds input size limit"
        );

        let mut input = Cursor::new(file.as_slice());
        ensure!(read_u32(&mut input)? == MAGIC, "invalid replay magic");
        let block_count = read_u32(&mut input)?;
        ensure!(block_count > 0, "missing replay metadata");
        let metadata_size = read_u32(&mut input)? as usize;
        let metadata = serde_json::from_slice(take(&mut input, metadata_size)?)
            .context("invalid replay metadata")?;
        for _ in 1..block_count {
            let length = read_u32(&mut input)? as usize;
            take(&mut input, length)?;
        }

        let stream_size = read_u32(&mut input)? as usize;
        let compressed_size = read_u32(&mut input)? as usize;
        ensure!(
            stream_size <= MAX_STREAM_BYTES,
            "replay exceeds decompressed size limit"
        );
        ensure!(compressed_size <= MAX_FILE_BYTES, "invalid compressed size");
        let encrypted = take(&mut input, compressed_size.next_multiple_of(8))?;
        ensure!(
            input.position() == file.len() as u64,
            "trailing replay data"
        );

        let cipher: Blowfish = Blowfish::new_from_slice(&REPLAY_KEY)
            .map_err(|_| anyhow::anyhow!("invalid replay key"))?;
        let mut previous = [0; 8];
        let mut compressed = Vec::with_capacity(encrypted.len());
        let (encrypted_blocks, remainder) = encrypted.as_chunks::<8>();
        ensure!(remainder.is_empty(), "unaligned replay ciphertext");
        for bytes in encrypted_blocks {
            let mut block = Block::<Blowfish>::clone_from_slice(bytes);
            cipher.decrypt_block(&mut block);
            for (byte, previous_byte) in block.iter().zip(&mut previous) {
                *previous_byte ^= byte;
            }
            compressed.extend_from_slice(&previous);
        }
        compressed.truncate(compressed_size);

        let mut stream = vec![0; stream_size + 1];
        let mut decoder = Decompress::new(true);
        let status = decoder.decompress(&compressed, &mut stream, FlushDecompress::Finish)?;
        ensure!(
            status == Status::StreamEnd
                && decoder.total_in() == compressed_size as u64
                && decoder.total_out() == stream_size as u64,
            "replay stream sizes do not match the header"
        );
        stream.truncate(stream_size);
        Ok(Self { metadata, stream })
    }

    pub fn records(&self) -> impl Iterator<Item = Result<Record<'_>>> {
        let mut stream = Cursor::new(self.stream.as_slice());
        std::iter::from_fn(move || {
            let offset = stream.position();
            if offset >= self.stream.len() as u64 {
                return None;
            }
            let result = Record::read(&mut stream, offset)
                .with_context(|| format!("invalid record at stream byte {offset}"));
            if result.is_err() {
                stream.set_position(self.stream.len() as u64);
            }
            Some(result)
        })
    }
}

pub struct Record<'a> {
    pub kind: u32,
    pub clock_seconds: f32,
    pub payload: &'a [u8],
    pub offset: u64,
}

impl<'a> Record<'a> {
    fn read(stream: &mut Cursor<&'a [u8]>, offset: u64) -> Result<Self> {
        let payload_size = read_u32(stream)? as usize;
        let kind = read_u32(stream)?;
        let clock_seconds = read_f32(stream)?;
        ensure!(clock_seconds.is_finite(), "non-finite replay clock");
        let payload = take(stream, payload_size)?;
        Ok(Self {
            kind,
            clock_seconds,
            payload,
            offset,
        })
    }

    pub fn position(&self) -> Result<Position> {
        ensure!(self.kind == POSITION, "expected position record");
        ensure!(
            self.payload.len() == 45,
            "unsupported position payload size {} at stream byte {}",
            self.payload.len(),
            self.offset
        );
        let mut payload = Cursor::new(self.payload);
        Ok(Position {
            clock_seconds: self.clock_seconds,
            stream_offset: self.offset,
            entity_id: read_u32(&mut payload)?,
            secondary_id: read_u32(&mut payload)?,
            position_xyz: read_vector(&mut payload)?,
            position_error_xyz: read_vector(&mut payload)?,
            yaw_pitch_roll: read_vector(&mut payload)?,
            flags: read_u8(&mut payload)?,
        })
    }
}

#[derive(Serialize)]
pub struct Position {
    clock_seconds: f32,
    stream_offset: u64,
    pub entity_id: u32,
    secondary_id: u32,
    position_xyz: [f32; 3],
    position_error_xyz: [f32; 3],
    yaw_pitch_roll: [f32; 3],
    flags: u8,
}

fn read_vector(input: &mut Cursor<&[u8]>) -> Result<[f32; 3]> {
    let vector = [read_f32(input)?, read_f32(input)?, read_f32(input)?];
    ensure!(
        vector.iter().all(|value| value.is_finite()),
        "non-finite vector"
    );
    Ok(vector)
}

fn read_f32(input: &mut Cursor<&[u8]>) -> Result<f32> {
    Ok(f32::from_bits(read_u32(input)?))
}

fn read_u8(input: &mut Cursor<&[u8]>) -> Result<u8> {
    let mut byte = [0];
    input.read_exact(&mut byte)?;
    Ok(u8::from_le_bytes(byte))
}

fn read_u32(input: &mut Cursor<&[u8]>) -> Result<u32> {
    let mut bytes = [0; 4];
    input.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn take<'a>(input: &mut Cursor<&'a [u8]>, length: usize) -> Result<&'a [u8]> {
    let offset = input.position() as usize;
    let remaining = input.get_ref().get(offset..).context("truncated replay")?;
    let bytes = remaining.get(..length).context("truncated replay")?;
    input.set_position((offset + bytes.len()) as u64);
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_keeps_coordinates_distinct_from_position_error() -> Result<()> {
        let mut payload = Vec::new();
        payload.extend(17_u32.to_le_bytes());
        payload.extend(42_u32.to_le_bytes());
        for value in [12.0_f32, 3.0, -27.0, 0.01, 0.02, 0.03, 1.0, 0.2, -0.1] {
            payload.extend(value.to_le_bytes());
        }
        payload.push(1);
        let record = Record {
            kind: POSITION,
            clock_seconds: 2.5,
            offset: 48,
            payload: &payload,
        };

        let position = record.position()?;
        assert_eq!(position.entity_id, 17);
        assert_eq!(position.secondary_id, 42);
        assert_eq!(position.position_xyz, [12.0, 3.0, -27.0]);
        assert_eq!(position.position_error_xyz, [0.01, 0.02, 0.03]);
        assert_eq!(position.yaw_pitch_roll, [1.0, 0.2, -0.1]);
        assert_eq!(position.flags, 1);
        assert_eq!(position.clock_seconds, 2.5);
        assert_eq!(position.stream_offset, 48);
        Ok(())
    }

    #[test]
    fn truncated_record_is_reported_at_its_stream_offset() -> Result<()> {
        let mut stream = vec![0; 12];
        stream.extend(45_u32.to_le_bytes());
        stream.extend(POSITION.to_le_bytes());
        stream.extend(2.5_f32.to_le_bytes());
        let replay = Replay {
            metadata: serde_json::from_str("{}")?,
            stream,
        };
        let mut records = replay.records();
        assert!(matches!(records.next(), Some(Ok(record)) if record.offset == 0));
        assert!(
            matches!(records.next(), Some(Err(error)) if error.to_string().contains("byte 12"))
        );
        assert!(records.next().is_none());
        Ok(())
    }
}
