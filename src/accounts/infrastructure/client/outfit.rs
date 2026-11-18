use crate::accounts::{
    Camouflage, Inscription, InscriptionSlot, OutfitSeason, VehicleOutfit, VehicleSurface,
};
use std::collections::BTreeMap;

const OUTFIT: u16 = 4;
const EFFECTS: u16 = 1;
const CAMOUFLAGES: u16 = 1 << 2;
const DECALS: u16 = 1 << 3;
const PERSONAL_NUMBERS: u16 = 1 << 7;
const MAX_NUMBER_DIGITS: usize = 8;
const EMPTY_ITEM: u16 = 22222;
const ALL_SEASONS: i64 = 15;
const MAX_OUTFIT_BYTES: usize = 160;

const SURFACES: [(u16, VehicleSurface); 3] = [
    (1 << 4, VehicleSurface::Hull),
    (1 << 8, VehicleSurface::Turret),
    (1 << 12, VehicleSurface::Gun),
];
const INSCRIPTION_SLOTS: [(u16, InscriptionSlot); 6] = [
    (1 << 6, InscriptionSlot::HullFirst),
    (1 << 7, InscriptionSlot::HullSecond),
    (1 << 10, InscriptionSlot::TurretFirst),
    (1 << 11, InscriptionSlot::TurretSecond),
    (1 << 14, InscriptionSlot::GunFirst),
    (1 << 15, InscriptionSlot::GunSecond),
];
const INSCRIPTION_AREAS: u16 = 0xCCC0;
const CAMOUFLAGE_AREAS: u16 = (1 << 4) | (1 << 8) | (1 << 12);
const EMBLEM_AREAS: u16 = CAMOUFLAGE_AREAS | (CAMOUFLAGE_AREAS << 1);

pub(super) fn season_code(season: OutfitSeason) -> i32 {
    match season {
        OutfitSeason::Winter => 1,
        OutfitSeason::Summer => 2,
        OutfitSeason::Desert => 4,
    }
}

pub fn decode_outfits<'a>(
    seasons: &[i64],
    outfits: impl ExactSizeIterator<Item = &'a [u8]>,
    default_emblem: Option<u16>,
) -> Option<BTreeMap<OutfitSeason, VehicleOutfit>> {
    if !(1..=4).contains(&seasons.len()) || seasons.len() != outfits.len() {
        return None;
    }
    let mut layouts = BTreeMap::new();
    let mut common_seen = false;
    for (&season, bytes) in seasons.iter().zip(outfits) {
        let layout = decode(bytes, default_emblem)?;
        let season = match season {
            1 => OutfitSeason::Winter,
            2 => OutfitSeason::Summer,
            4 => OutfitSeason::Desert,
            ALL_SEASONS if !common_seen && layout.is_empty() => {
                common_seen = true;
                continue;
            }
            _ => return None,
        };
        if layouts.insert(season, layout).is_some() {
            return None;
        }
    }
    Some(layouts)
}

fn decode(mut bytes: &[u8], default_emblem: Option<u16>) -> Option<VehicleOutfit> {
    let mut layout = VehicleOutfit::default();
    let mut occupied = 0;
    if bytes.is_empty() {
        return Some(layout);
    }
    if bytes.len() > MAX_OUTFIT_BYTES || read_varint(&mut bytes)? != OUTFIT {
        return None;
    }
    let mask = read_varint(&mut bytes)?;
    if mask & !(EFFECTS | CAMOUFLAGES | DECALS | PERSONAL_NUMBERS) != 0 {
        return None;
    }
    if mask & EFFECTS != 0 {
        match read_varint(&mut bytes)? {
            0 => {}
            1 => {
                let id = read_varint(&mut bytes)?;
                if id == 0 {
                    return None;
                }
                layout.effect = (id != EMPTY_ITEM).then_some(id);
            }
            _ => return None,
        }
    }
    if mask & CAMOUFLAGES != 0 {
        let count = read_varint(&mut bytes)?;
        if count > 3 {
            return None;
        }
        for _ in 0..count {
            let fields = read_varint(&mut bytes)?;
            if fields & !0xF != 0 || fields & 1 == 0 {
                return None;
            }
            let id = read_varint(&mut bytes)?;
            let pattern_size = u8::try_from(read_field(&mut bytes, fields, 2, 1)?).ok()?;
            let areas = read_field(&mut bytes, fields, 4, CAMOUFLAGE_AREAS)?;
            let palette = u8::try_from(read_field(&mut bytes, fields, 8, 0)?).ok()?;
            if areas == 0 || areas & !CAMOUFLAGE_AREAS != 0 || occupied & areas != 0 || id <= 1 {
                return None;
            }
            occupied |= areas;
            if id != EMPTY_ITEM {
                for (mask, surface) in SURFACES {
                    if areas & mask != 0 {
                        layout.camouflage.insert(
                            surface,
                            Camouflage {
                                id,
                                pattern_size,
                                palette,
                            },
                        );
                    }
                }
            }
        }
    }
    let mut occupied_inscriptions = 0;
    if mask & DECALS != 0 {
        let count = read_varint(&mut bytes)?;
        if count > (EMBLEM_AREAS | INSCRIPTION_AREAS).count_ones() as u16 {
            return None;
        }
        for _ in 0..count {
            let fields = read_varint(&mut bytes)?;
            let id = read_field(&mut bytes, fields, 1, 0)?;
            let areas = read_field(&mut bytes, fields, 2, 0)?;
            let level = read_field(&mut bytes, fields, 4, 0)?;
            if fields & !7 != 0 || areas == 0 || level != 0 {
                return None;
            }
            if areas & !EMBLEM_AREAS == 0 && (id == EMPTY_ITEM || Some(id) == default_emblem) {
                continue;
            }
            if id == 0
                || !insert_inscription(
                    &mut layout,
                    &mut occupied_inscriptions,
                    areas,
                    Inscription::Preset(id),
                )
            {
                return None;
            }
        }
    }
    if mask & PERSONAL_NUMBERS != 0 {
        let count = read_varint(&mut bytes)?;
        if count > INSCRIPTION_SLOTS.len() as u16 {
            return None;
        }
        for _ in 0..count {
            let fields = read_varint(&mut bytes)?;
            if fields & !7 != 0 || fields & 5 != 5 {
                return None;
            }
            let id = read_varint(&mut bytes)?;
            let digits = if fields & 2 != 0 {
                let length = usize::from(read_varint(&mut bytes)?);
                if !(1..=MAX_NUMBER_DIGITS).contains(&length) {
                    return None;
                }
                let (number, rest) = bytes.split_at_checked(length)?;
                bytes = rest;
                if !number.iter().all(u8::is_ascii_digit) {
                    return None;
                }
                std::str::from_utf8(number).ok()?.to_owned()
            } else {
                String::new()
            };
            let areas = read_varint(&mut bytes)?;
            if id == 0
                || (id != EMPTY_ITEM && digits.is_empty())
                || !insert_inscription(
                    &mut layout,
                    &mut occupied_inscriptions,
                    areas,
                    Inscription::Number { id, digits },
                )
            {
                return None;
            }
        }
    }
    bytes.is_empty().then_some(layout)
}

fn insert_inscription(
    layout: &mut VehicleOutfit,
    occupied: &mut u16,
    areas: u16,
    inscription: Inscription,
) -> bool {
    if areas == 0 || areas & !INSCRIPTION_AREAS != 0 || areas & *occupied != 0 {
        return false;
    }
    *occupied |= areas;
    if inscription.item().1 != EMPTY_ITEM {
        for (mask, slot) in INSCRIPTION_SLOTS {
            if areas & mask != 0 {
                layout.inscriptions.insert(slot, inscription.clone());
            }
        }
    }
    true
}

pub(super) fn encode(layout: &VehicleOutfit) -> Vec<u8> {
    let mut bytes = Vec::new();
    let presets = layout
        .inscriptions
        .values()
        .filter(|item| matches!(item, Inscription::Preset(_)))
        .count() as u16;
    let numbers = layout.inscriptions.len() as u16 - presets;
    let mut fields = if layout.effect.is_some() { EFFECTS } else { 0 };
    if !layout.camouflage.is_empty() {
        fields |= CAMOUFLAGES;
    }
    if presets > 0 {
        fields |= DECALS;
    }
    if numbers > 0 {
        fields |= PERSONAL_NUMBERS;
    }
    write_varint(&mut bytes, OUTFIT);
    write_varint(&mut bytes, fields);
    if let Some(effect) = layout.effect {
        write_varint(&mut bytes, 1);
        write_varint(&mut bytes, effect);
    }

    if fields & CAMOUFLAGES != 0 {
        write_varint(&mut bytes, layout.camouflage.len() as u16);
        for (area, surface) in SURFACES {
            if let Some(item) = layout.camouflage.get(&surface) {
                write_varint(&mut bytes, 0xF);
                write_varint(&mut bytes, item.id);
                write_varint(&mut bytes, u16::from(item.pattern_size));
                write_varint(&mut bytes, area);
                write_varint(&mut bytes, u16::from(item.palette));
            }
        }
    }
    if presets > 0 {
        write_varint(&mut bytes, presets);
        for (area, slot) in INSCRIPTION_SLOTS {
            if let Some(Inscription::Preset(id)) = layout.inscriptions.get(&slot) {
                write_varint(&mut bytes, 3);
                write_varint(&mut bytes, *id);
                write_varint(&mut bytes, area);
            }
        }
    }
    if numbers > 0 {
        write_varint(&mut bytes, numbers);
        for (area, slot) in INSCRIPTION_SLOTS {
            if let Some(Inscription::Number { id, digits }) = layout.inscriptions.get(&slot) {
                write_varint(&mut bytes, 7);
                write_varint(&mut bytes, *id);
                write_varint(&mut bytes, digits.len() as u16);
                bytes.extend_from_slice(digits.as_bytes());
                write_varint(&mut bytes, area);
            }
        }
    }
    bytes
}

fn read_field(bytes: &mut &[u8], fields: u16, mask: u16, default: u16) -> Option<u16> {
    if fields & mask == 0 {
        Some(default)
    } else {
        read_varint(bytes)
    }
}

fn read_varint(bytes: &mut &[u8]) -> Option<u16> {
    let mut value = 0;
    for shift in [0, 7, 14] {
        let (&byte, rest) = bytes.split_first()?;
        *bytes = rest;
        if shift == 14 && byte > 3 {
            return None;
        }
        value |= (u16::from(byte) & 0x7F) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

fn write_varint(bytes: &mut Vec<u8>, mut value: u16) {
    while value >= 128 {
        bytes.push((value & 0x7F) as u8 | 0x80);
        value >>= 7;
    }
    bytes.push(value as u8);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_grouped_surfaces_and_ignores_the_vehicle_default_emblem() {
        // OUTFIT, camouflage and decal fields, then one entry in each array.
        let raw = [4, 12, 1, 1, 228, 118, 1, 3, 72, 16];
        let decoded = decode(&raw, Some(72)).unwrap();
        let expected = Camouflage {
            id: 15_204,
            palette: 0,
            pattern_size: 1,
        };
        assert_eq!(
            decoded.camouflage,
            SURFACES
                .into_iter()
                .map(|(_, surface)| (surface, expected))
                .collect()
        );
        assert_eq!(decode(&encode(&decoded), None), Some(decoded));
        assert_eq!(decode(&raw, Some(73)), None);
    }

    #[test]
    fn rejects_invalid_seasons_and_unbounded_or_overlapping_components() {
        let overlap = [4, 4, 2, 1, 2, 5, 3, 16];
        let invalid = [
            overlap.as_slice(),
            &[4, 4, 4],                  // More components than surfaces.
            &[4, 4, 1, 5, 2, 1],         // Chassis.
            &[4, 16, 3],                 // Style.
            &[4, 4, 1, 1, 128, 128, 16], // Oversized identifier.
            &[4, 4, 0, 0],               // Extra byte.
        ];
        for payload in invalid {
            assert_eq!(decode(payload, None), None);
        }
        for seasons in [[2, 2], [15, 15], [8, 2]] {
            assert_eq!(
                decode_outfits(&seasons, [&[][..], &[][..]].into_iter(), None),
                None
            );
        }
    }
}
