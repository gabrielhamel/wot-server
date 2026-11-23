//! Elevation and driving surfaces from the client geometry.

use super::*;
use crate::domain::movement::GroundContact;
use byteorder::{LittleEndian, ReadBytesExt};
use flate2::read::ZlibDecoder;
use glam::Vec3Swizzles;
use std::io::{BufReader, Read};
use wot_platform::client_formats::pxml;

const CHUNK_LIMIT: usize = 1024;
const SAMPLE_MARGIN: usize = 2;
const MAX_TILE_WIDTH: usize = 261;
const DEFAULT_CHUNK_METRES: f32 = 100.0;
const TRACK_HALF_WIDTH: f32 = 1.25;
const TRACK_HALF_LENGTH: f32 = 2.5;

pub(super) struct Terrain {
    metres: f32,
    tiles: HashMap<(i16, i16), Tile>,
    surfaces: HashMap<(i16, i16), SurfaceTile>,
}

struct Tile {
    width: usize,
    depth: usize,
    millimetres: Vec<i32>,
}

impl Terrain {
    #[tracing::instrument(name = "battles.load_terrain", skip_all, fields(
        ddd.bounded_context.name = "battles", input.geometry = geometry,
        output.terrain.tiles = tracing::field::Empty, output.terrain.surface_tiles = tracing::field::Empty, otel.status_code = tracing::field::Empty,
    ), err)]
    pub(super) fn load(resources: &ResourceBundle, geometry: &str) -> Result<Self> {
        let settings = format!("{geometry}/space.settings");
        let metres = if resources.has_file(&settings) {
            pxml::from_reader(BufReader::new(resources.read(&settings)?))?
                .get_child_float("chunkSize")
                .unwrap_or(DEFAULT_CHUNK_METRES)
        } else {
            DEFAULT_CHUNK_METRES
        };
        anyhow::ensure!(
            metres.is_finite() && (1.0..=1000.0).contains(&metres),
            "Invalid terrain tile size"
        );
        let mut tiles = HashMap::new();
        for file in resources
            .read_dir(&format!("{geometry}/terrain"))
            .context("Arena terrain is missing. Re-extract the client resource bundle")?
        {
            let file = file?;
            let name = file.file_name();
            let Some((x, z)) = name.to_str().and_then(tile_coordinates) else {
                continue;
            };
            anyhow::ensure!(
                tiles.len() < CHUNK_LIMIT,
                "Arena terrain has too many tiles"
            );
            let key = format!("{geometry}/terrain/{}", name.to_string_lossy());
            let tile = Tile::decode(BufReader::new(resources.read(&key)?))
                .with_context(|| format!("Decode {key}"))?;
            tiles.insert((x, z), tile);
        }
        anyhow::ensure!(!tiles.is_empty(), "Arena terrain has no elevation tiles");
        let mut surfaces = HashMap::new();
        for &(x, z) in tiles.keys() {
            let key = format!("{geometry}/terrain/{:04x}{:04x}.mat", x as u16, z as u16);
            if resources.has_file(&key) {
                let tile = SurfaceTile::decode(BufReader::new(resources.read(&key)?))
                    .with_context(|| format!("Decode {key}"))?;
                surfaces.insert((x, z), tile);
            }
        }
        tracing::Span::current().record("output.terrain.tiles", tiles.len());
        tracing::Span::current().record("output.terrain.surface_tiles", surfaces.len());
        Ok(Self {
            metres,
            tiles,
            surfaces,
        })
    }

    fn tile_at(&self, point: Vec2) -> Option<((i16, i16), Vec2)> {
        let point = point / self.metres;
        let grid = point.floor();
        let x = i16::try_from(grid.x as i32).ok()?;
        let z = i16::try_from(grid.y as i32).ok()?;
        Some(((x, z), point - grid))
    }

    pub(super) fn height_at(&self, point: Vec2) -> Option<f32> {
        let (tile, local) = self.tile_at(point)?;
        self.tiles.get(&tile)?.sample(local)
    }

    pub(super) fn ground_contact(
        &self,
        point: Vec2,
        yaw: f32,
        half_width: f32,
        pitch: f32,
    ) -> GroundContact {
        let forward = Vec2::new(yaw.sin(), yaw.cos()) * TRACK_HALF_LENGTH;
        let right = Vec2::new(yaw.cos(), -yaw.sin()) * half_width;
        let mut surfaces = [0; 3];
        for offset in [
            forward + right,
            forward - right,
            -forward + right,
            -forward - right,
        ] {
            let surface = self
                .tile_at(point + offset)
                .and_then(|(tile, local)| self.surfaces.get(&tile)?.sample(local))
                .unwrap_or(1);
            surfaces[surface] += 1;
        }
        GroundContact { surfaces, pitch }
    }

    pub(super) fn shot_intersection(
        &self,
        origin: Vec3,
        direction: Vec3,
        range: f32,
    ) -> Option<f32> {
        let below = |distance: f32| {
            let point = origin + direction * distance;
            self.height_at(point.xz())
                .is_some_and(|height| point.y <= height)
        };
        let steps = range.ceil() as u32;
        let mut previous = 0.0;
        for step in 1..=steps {
            let distance = (step as f32).min(range);
            if below(distance) {
                let mut lo = previous;
                let mut hi = distance;
                for _ in 0..8 {
                    let mid = (lo + hi) * 0.5;
                    if below(mid) {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                return Some(hi);
            }
            previous = distance;
        }
        None
    }

    pub(super) fn vehicle_pose(&self, point: Vec2, yaw: f32) -> Option<(Vec3, Vec3)> {
        let center = self.height_at(point)?;
        let forward = Vec2::new(yaw.sin(), yaw.cos()) * TRACK_HALF_LENGTH;
        let right = Vec2::new(yaw.cos(), -yaw.sin()) * TRACK_HALF_WIDTH;
        let height = |offset| self.height_at(point + offset).unwrap_or(center);
        let front_left = height(forward - right);
        let front_right = height(forward + right);
        let rear_left = height(-forward - right);
        let rear_right = height(-forward + right);
        let rise_right = (front_right + rear_right - front_left - rear_left) * 0.25;
        let rise_forward = (front_left + front_right - rear_left - rear_right) * 0.25;
        let y = center
            .max(front_left + rise_right - rise_forward)
            .max(front_right - rise_right - rise_forward)
            .max(rear_left + rise_right + rise_forward)
            .max(rear_right - rise_right + rise_forward);
        Some((
            Vec3::new(point.x, y, point.y),
            Vec3::new(
                (rise_right / TRACK_HALF_WIDTH).atan(),
                (-rise_forward / TRACK_HALF_LENGTH).atan(),
                yaw,
            ),
        ))
    }
}

fn tile_coordinates(name: &str) -> Option<(i16, i16)> {
    let stem = name.strip_suffix(".hmp")?;
    let (x, z) = stem.split_at_checked(4)?;
    if z.len() != 4 {
        return None;
    }
    Some((
        u16::from_str_radix(x, 16).ok()? as i16,
        u16::from_str_radix(z, 16).ok()? as i16,
    ))
}

struct SurfaceTile {
    width: usize,
    depth: usize,
    cells: Vec<u8>,
}

impl SurfaceTile {
    fn decode(mut input: impl Read) -> Result<Self> {
        let number = |input: &mut dyn Read| input.read_u32::<LittleEndian>();
        anyhow::ensure!(
            number(&mut input)? == u32::from_le_bytes(*b"mat\0"),
            "Invalid terrain surface header"
        );
        anyhow::ensure!(
            number(&mut input)? == 1,
            "Unsupported terrain surface version"
        );
        let textures = u64::from(number(&mut input)?);
        let name_bytes = u64::from(number(&mut input)?);
        let width = number(&mut input)? as usize;
        let depth = number(&mut input)? as usize;
        input.read_exact(&mut [0; 8])?;
        anyhow::ensure!(
            textures <= 16 && name_bytes <= 1024,
            "Invalid terrain texture table"
        );
        let table_bytes = textures * name_bytes;
        anyhow::ensure!(
            std::io::copy(&mut (&mut input).take(table_bytes), &mut std::io::sink())?
                == table_bytes,
            "Truncated terrain texture table"
        );
        anyhow::ensure!(
            number(&mut input)? == 0x7a697000,
            "Invalid terrain surface compression header"
        );
        let _compressed_marker = number(&mut input)?;
        let size = number(&mut input)? as usize;
        anyhow::ensure!(
            (1..=MAX_TILE_WIDTH).contains(&width)
                && (1..=MAX_TILE_WIDTH).contains(&depth)
                && size == width * depth,
            "Invalid terrain surface dimensions"
        );
        let mut cells = Vec::with_capacity(size);
        ZlibDecoder::new(input.take(512 * 1024))
            .take(size as u64 + 1)
            .read_to_end(&mut cells)?;
        anyhow::ensure!(cells.len() == size, "Invalid terrain surface cell count");
        Ok(Self {
            width,
            depth,
            cells,
        })
    }

    fn sample(&self, point: Vec2) -> Option<usize> {
        let x = (point.x * self.width as f32) as usize;
        let z = (point.y * self.depth as f32) as usize;
        let cell = self
            .cells
            .get(z.min(self.depth - 1) * self.width + x.min(self.width - 1))?;
        match cell >> 4 {
            1 => Some(0),
            2 => Some(1),
            3 => Some(2),
            _ => None,
        }
    }
}

impl Tile {
    fn decode(mut input: impl Read) -> Result<Self> {
        let mut magic = [0; 4];
        input.read_exact(&mut magic)?;
        anyhow::ensure!(magic == *b"hmp\0", "Invalid terrain header");
        let width = input.read_u32::<LittleEndian>()? as usize;
        let depth = input.read_u32::<LittleEndian>()? as usize;
        let _compression = input.read_u32::<LittleEndian>()?;
        let version = input.read_u32::<LittleEndian>()?;
        anyhow::ensure!(
            version == 4,
            "Unsupported terrain height encoding {version}"
        );
        input.read_exact(&mut [0; 12])?;
        input.read_exact(&mut magic)?;
        anyhow::ensure!(magic == *b"gnpq", "Invalid quantized terrain PNG header");
        anyhow::ensure!(
            (6..=MAX_TILE_WIDTH).contains(&width) && (6..=MAX_TILE_WIDTH).contains(&depth),
            "Invalid terrain dimensions"
        );
        let mut decoder = png::Decoder::new(input.take(512 * 1024));
        decoder.set_limits(png::Limits { bytes: 1024 * 1024 });
        let mut reader = decoder.read_info()?;
        let info = reader.info();
        anyhow::ensure!(
            (
                info.width as usize,
                info.height as usize,
                info.color_type,
                info.bit_depth
            ) == (width, depth, png::ColorType::Rgba, png::BitDepth::Eight),
            "Unexpected terrain PNG dimensions or pixel layout"
        );
        let mut raw = vec![0; width * depth * 4];
        reader.next_frame(&mut raw)?;
        let (pixels, _) = raw.as_chunks::<4>();
        let millimetres = pixels.iter().copied().map(i32::from_le_bytes).collect();
        Ok(Self {
            width,
            depth,
            millimetres,
        })
    }

    fn sample(&self, point: Vec2) -> Option<f32> {
        let steps = Vec2::new(
            (self.width - 2 * SAMPLE_MARGIN - 1) as f32,
            (self.depth - 2 * SAMPLE_MARGIN - 1) as f32,
        );
        let p = point.clamp(Vec2::ZERO, Vec2::ONE) * steps + Vec2::splat(SAMPLE_MARGIN as f32);
        let x = p.x.floor() as usize;
        let z = p.y.floor() as usize;
        let u = p.x.fract();
        let v = p.y.fract();
        let at = |dx, dz| {
            self.millimetres
                .get((z + dz) * self.width + x + dx)
                .map(|&mm| mm as f32 * 0.001)
        };
        let a = at(0, 0)?;
        let b = at(1, 0)?;
        let c = at(0, 1)?;
        let d = at(1, 1)?;
        // BigWorld alternates triangle orientation across adjacent height samples.
        Some(if (x + z).is_multiple_of(2) {
            if u >= v {
                a * (1.0 - u) + b * (u - v) + d * v
            } else {
                a * (1.0 - v) + c * (v - u) + d * u
            }
        } else if u + v <= 1.0 {
            a * (1.0 - u - v) + b * u + c * v
        } else {
            d * (u + v - 1.0) + b * (1.0 - v) + c * (1.0 - u)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_tiles_use_the_ground_nibble_and_blend_across_signed_chunks() {
        use flate2::{Compression, write::ZlibEncoder};
        use std::io::Write;

        let mut tile = Vec::new();
        for word in [u32::from_le_bytes(*b"mat\0"), 1, 1, 4, 2, 2, 0, 0] {
            tile.extend(word.to_le_bytes());
        }
        tile.extend(b"dds\0");
        for word in [0x7a697000_u32, 0, 4] {
            tile.extend(word.to_le_bytes());
        }
        let mut compressed = ZlibEncoder::new(tile, Compression::default());
        compressed.write_all(&[0x1b, 0x11, 0x23, 0x3a]).unwrap();
        let tile = compressed.finish().unwrap();
        let surface = SurfaceTile::decode(tile.as_slice()).unwrap();
        assert_eq!(surface.sample(Vec2::new(0.1, 0.1)), Some(0));
        assert_eq!(surface.sample(Vec2::new(0.1, 0.9)), Some(1));
        assert_eq!(surface.sample(Vec2::new(0.9, 0.9)), Some(2));
        assert!(SurfaceTile::decode(&tile[..40]).is_err());

        let terrain = Terrain {
            metres: 100.0,
            tiles: HashMap::new(),
            surfaces: HashMap::from([((-1, 0), surface)]),
        };
        let contact = terrain.ground_contact(Vec2::new(0.0, 75.0), 0.0, 1.25, 0.0);
        assert_eq!(contact.surfaces, [0, 2, 2]);
    }

    #[test]
    fn terrain_interpolates_slopes_and_uses_signed_tile_coordinates() {
        let millimetres = (0..7)
            .flat_map(|z| (0..7).map(move |x| 20_000 + 2_000 * x + 4_000 * z))
            .collect();
        let ground = Terrain {
            metres: 100.0,
            surfaces: HashMap::new(),
            tiles: HashMap::from([(
                (-1, 0),
                Tile {
                    width: 7,
                    depth: 7,
                    millimetres,
                },
            )]),
        };
        assert_eq!(tile_coordinates("ffff0000.hmp"), Some((-1, 0)));
        assert_eq!(ground.height_at(Vec2::new(-100.0, 0.0)), Some(32.0));
        assert_eq!(ground.height_at(Vec2::new(-25.0, 50.0)), Some(39.0));
        assert_eq!(ground.height_at(Vec2::new(0.0, 50.0)), None);
        let (position, direction) = ground.vehicle_pose(Vec2::new(-25.0, 50.0), 0.0).unwrap();
        assert!((position.y - 39.0).abs() < 0.001);
        assert!(direction.x > 0.0 && direction.y < 0.0);

        let origin = position + 5.25 * Vec3::Y;
        assert_eq!(ground.shot_intersection(origin, -Vec3::Y, 4.0), None);
        assert_eq!(ground.shot_intersection(origin, Vec3::Y, 10.0), None);
        assert_eq!(ground.shot_intersection(origin, -Vec3::Y, 10.0), Some(5.25));
    }
}
