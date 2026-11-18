use crate::accounts::{
    Attachment, Camouflage, Inscription, InscriptionSlot, MAX_ATTACHMENTS, MAX_PROJECTION_DECALS,
    OutfitSeason, Paint, PaintRegion, ProjectionDecal, StatTracker, VehicleOutfit, VehicleSurface,
};
use std::collections::{BTreeMap, BTreeSet};

const OUTFIT: u16 = 4;
const EFFECTS: u16 = 1;
const PAINTS: u16 = 1 << 1;
const CAMOUFLAGES: u16 = 1 << 2;
const DECALS: u16 = 1 << 3;
const STYLE: u16 = 1 << 4;
const PROJECTION_DECALS: u16 = 1 << 5;
const DEFAULT_DECAL_SCALE: u16 = 3;
const STAT_TRACKERS: u16 = 1 << 12;
const ATTACHMENTS: u16 = 1 << 9;
const PERSONAL_NUMBERS: u16 = 1 << 7;
const MAX_NUMBER_DIGITS: usize = 8;
const EMPTY_ITEM: u16 = 22222;
const MAX_OUTFIT_BYTES: usize = 224;

const SURFACES: [(u16, VehicleSurface); 3] = [
    (1 << 4, VehicleSurface::Hull),
    (1 << 8, VehicleSurface::Turret),
    (1 << 12, VehicleSurface::Gun),
];
const PAINT_REGIONS: [(u16, PaintRegion); 5] = [
    (1 << 0, PaintRegion::Chassis),
    (1 << 4, PaintRegion::Hull),
    (1 << 8, PaintRegion::Turret),
    (1 << 12, PaintRegion::Gun),
    (1 << 14, PaintRegion::Mantlet),
];
const PAINT_AREAS: u16 = 0x5111;

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
        OutfitSeason::Common => 15,
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
    for (&season, bytes) in seasons.iter().zip(outfits) {
        let layout = decode(bytes, default_emblem)?;
        let season = match season {
            1 => OutfitSeason::Winter,
            2 => OutfitSeason::Summer,
            4 => OutfitSeason::Desert,
            15 => OutfitSeason::Common,
            _ => return None,
        };
        if !layout.supports_season(season) || layouts.insert(season, layout).is_some() {
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
    if mask
        & !(EFFECTS
            | PAINTS
            | CAMOUFLAGES
            | DECALS
            | STYLE
            | PROJECTION_DECALS
            | PERSONAL_NUMBERS
            | ATTACHMENTS
            | STAT_TRACKERS)
        != 0
    {
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
    if mask & PAINTS != 0 {
        let count = usize::from(read_varint(&mut bytes)?);
        if count > PAINT_REGIONS.len() {
            return None;
        }
        let mut painted = 0;
        for _ in 0..count {
            let fields = read_varint(&mut bytes)?;
            if fields & !3 != 0 || fields & 1 == 0 {
                return None;
            }
            let id = read_varint(&mut bytes)?;
            let areas = read_field(&mut bytes, fields, 2, PAINT_AREAS)?;
            if id == 0 || areas == 0 || areas & !PAINT_AREAS != 0 || painted & areas != 0 {
                return None;
            }
            painted |= areas;
            if id != EMPTY_ITEM {
                for (mask, region) in PAINT_REGIONS {
                    if areas & mask != 0 {
                        layout.paints.insert(region, Paint { id, units: 1 });
                    }
                }
            }
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
    if mask & STYLE != 0 {
        let id = read_varint(&mut bytes)?;
        if id == EMPTY_ITEM {
            return None;
        }
        layout.style = (id != 0).then_some(id);
    }
    if mask & PROJECTION_DECALS != 0 {
        let count = usize::from(read_varint(&mut bytes)?);
        if count > MAX_PROJECTION_DECALS {
            return None;
        }
        for _ in 0..count {
            let fields = read_varint(&mut bytes)?;
            if fields & !0x1F != 0 || fields & 5 != 5 {
                return None;
            }
            let id = read_varint(&mut bytes)?;
            let options = u8::try_from(read_field(&mut bytes, fields, 2, 0)?).ok()?;
            let slot_id = read_varint(&mut bytes)?;
            let scale =
                u8::try_from(read_field(&mut bytes, fields, 8, DEFAULT_DECAL_SCALE)?).ok()?;
            let level = read_field(&mut bytes, fields, 16, 0)?;
            if id == 0
                || slot_id == 0
                || level != 0
                || layout
                    .projection_decals
                    .iter()
                    .any(|decal| decal.slot_id == slot_id)
            {
                return None;
            }
            if id != EMPTY_ITEM {
                layout.projection_decals.push(ProjectionDecal {
                    id,
                    slot_id,
                    options,
                    scale,
                });
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
    if mask & ATTACHMENTS != 0 {
        let count = usize::from(read_varint(&mut bytes)?);
        if count > MAX_ATTACHMENTS {
            return None;
        }
        let mut occupied = BTreeSet::new();
        for _ in 0..count {
            let fields = read_varint(&mut bytes)?;
            if fields & !0xF != 0 || fields & 3 != 3 {
                return None;
            }
            let id = read_varint(&mut bytes)?;
            let slot_id = read_varint(&mut bytes)?;
            let scale = u8::try_from(read_field(&mut bytes, fields, 4, 0)?).ok()?;
            let rotated = read_field(&mut bytes, fields, 8, 0)?;
            if id == 0 || slot_id == 0 || scale > 3 || rotated > 1 || !occupied.insert(slot_id) {
                return None;
            }
            if id != EMPTY_ITEM {
                layout.attachments.insert(
                    slot_id,
                    Attachment {
                        id,
                        scale,
                        rotated: rotated == 1,
                    },
                );
            }
        }
    }
    if mask & STAT_TRACKERS != 0 {
        match read_varint(&mut bytes)? {
            0 => {}
            1 => {
                if read_varint(&mut bytes)? != 3 {
                    return None;
                }
                let id = read_varint(&mut bytes)?;
                let slot_id = read_varint(&mut bytes)?;
                if id == 0 || slot_id == 0 {
                    return None;
                }
                if id != EMPTY_ITEM {
                    layout.stat_tracker = Some(StatTracker { id, slot_id });
                }
            }
            _ => return None,
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
    if !layout.paints.is_empty() {
        fields |= PAINTS;
    }
    if !layout.camouflage.is_empty() {
        fields |= CAMOUFLAGES;
    }
    if presets > 0 {
        fields |= DECALS;
    }
    if layout.style.is_some() {
        fields |= STYLE;
    }
    if !layout.projection_decals.is_empty() {
        fields |= PROJECTION_DECALS;
    }
    if numbers > 0 {
        fields |= PERSONAL_NUMBERS;
    }
    if !layout.attachments.is_empty() {
        fields |= ATTACHMENTS;
    }
    if layout.stat_tracker.is_some() {
        fields |= STAT_TRACKERS;
    }
    write_varint(&mut bytes, OUTFIT);
    write_varint(&mut bytes, fields);
    if let Some(effect) = layout.effect {
        write_varint(&mut bytes, 1);
        write_varint(&mut bytes, effect);
    }

    if fields & PAINTS != 0 {
        write_varint(&mut bytes, layout.paints.len() as u16);
        for (area, region) in PAINT_REGIONS {
            if let Some(paint) = layout.paints.get(&region) {
                write_varint(&mut bytes, 3);
                write_varint(&mut bytes, paint.id);
                write_varint(&mut bytes, area);
            }
        }
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
    if let Some(id) = layout.style {
        write_varint(&mut bytes, id);
    }
    if fields & PROJECTION_DECALS != 0 {
        write_varint(&mut bytes, layout.projection_decals.len() as u16);
        for decal in &layout.projection_decals {
            write_varint(&mut bytes, 0xF);
            write_varint(&mut bytes, decal.id);
            write_varint(&mut bytes, u16::from(decal.options));
            write_varint(&mut bytes, decal.slot_id);
            write_varint(&mut bytes, u16::from(decal.scale));
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
    if fields & ATTACHMENTS != 0 {
        write_varint(&mut bytes, layout.attachments.len() as u16);
        for (&slot_id, attachment) in &layout.attachments {
            write_varint(&mut bytes, 0xF);
            write_varint(&mut bytes, attachment.id);
            write_varint(&mut bytes, slot_id);
            write_varint(&mut bytes, u16::from(attachment.scale));
            write_varint(&mut bytes, u16::from(attachment.rotated));
        }
    }
    if let Some(tracker) = layout.stat_tracker {
        write_varint(&mut bytes, 1);
        write_varint(&mut bytes, 3);
        write_varint(&mut bytes, tracker.id);
        write_varint(&mut bytes, tracker.slot_id);
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
    fn projected_decals_decode_native_defaults_and_keep_slot_order() {
        let raw = [4, 32, 2, 5, 8, 157, 117, 15, 8, 1, 152, 117, 2];
        let outfit = decode(&raw, None).unwrap();
        assert_eq!(
            outfit.projection_decals,
            vec![
                ProjectionDecal {
                    id: 8,
                    slot_id: 15005,
                    options: 0,
                    scale: 3
                },
                ProjectionDecal {
                    id: 8,
                    slot_id: 15000,
                    options: 1,
                    scale: 2
                },
            ],
        );
        assert_eq!(decode(&encode(&outfit), None), Some(outfit));
        assert!(decode(&[4, 32, 4], None).is_none());
    }

    #[test]
    fn round_trips_a_style_in_the_common_outfit() {
        let raw = [4, 16, 0xa3, 2];
        let outfits = decode_outfits(&[15], [raw.as_slice()].into_iter(), None).unwrap();
        let outfit = &outfits[&OutfitSeason::Common];
        assert_eq!(outfit.style, Some(291));
        assert_eq!(encode(outfit), raw);
        assert!(decode_outfits(&[2], [raw.as_slice()].into_iter(), None).is_none());
    }

    #[test]
    fn rejects_invalid_seasons_and_unbounded_or_overlapping_components() {
        let overlap = [4, 4, 2, 1, 2, 5, 3, 16];
        let invalid = [
            overlap.as_slice(),
            &[4, 4, 4],                          // More components than surfaces.
            &[4, 4, 1, 5, 2, 1],                 // Chassis.
            &[4, 16, 0x83],                      // Truncated style ID.
            &[4, 4, 1, 1, 128, 128, 16],         // Oversized identifier.
            &[4, 4, 0, 0],                       // Extra byte.
            &[4, 128, 4, 7],                     // More attachments than permitted.
            &[4, 128, 4, 2, 3, 1, 16, 3, 1, 16], // Duplicate attachment anchor.
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
