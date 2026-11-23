use super::{AccountSnapshot, ProgressionError, VehicleCost};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_COSMETIC_PURCHASE_LINES: usize = 12;
pub const MAX_ATTACHMENTS: usize = 6;
pub const MAX_PROJECTION_DECALS: usize = 3;
const MAX_UNITS_PER_PURCHASE: u32 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CosmeticKind {
    Camouflage,
    Style,
    Inscription,
    PersonalNumber,
    Effect,
    ProjectionDecal,
    Paint,
    Attachment,
    StatTracker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum OutfitSeason {
    Winter,
    Summer,
    Desert,
    Common,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum VehicleSurface {
    Hull,
    Turret,
    Gun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Camouflage {
    pub id: u16,
    pub pattern_size: u8,
    pub palette: u8,
}

pub type CosmeticItem = (CosmeticKind, u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum InscriptionSlot {
    HullFirst,
    HullSecond,
    TurretFirst,
    TurretSecond,
    GunFirst,
    GunSecond,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Inscription {
    Preset(u16),
    Number { id: u16, digits: String },
}

impl Inscription {
    pub fn item(&self) -> CosmeticItem {
        match self {
            Self::Preset(id) => (CosmeticKind::Inscription, *id),
            Self::Number { id, .. } => (CosmeticKind::PersonalNumber, *id),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionDecal {
    pub id: u16,
    pub slot_id: u16,
    pub options: u8,
    pub scale: u8,
}

#[derive(Clone)]
pub struct ProjectionDecalSlot {
    pub forms: BTreeSet<String>,
    pub vertical_mirror: bool,
}

impl ProjectionDecalSlot {
    pub fn supports_form(&self, form: Option<&str>) -> bool {
        self.forms.is_empty() || form.is_some_and(|form| self.forms.contains(form))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    pub id: u16,
    pub scale: u8,
    pub rotated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatTracker {
    pub id: u16,
    pub slot_id: u16,
}

#[derive(Clone)]
pub struct AttachmentSlot {
    pub apply_type: String,
    pub scale: u8,
}

#[derive(Clone)]
pub struct AttachmentRule {
    pub apply_type: String,
    pub scale: u8,
    pub scalable: bool,
    pub rotatable: bool,
}

impl AttachmentRule {
    pub fn fits(&self, slot: &AttachmentSlot, attachment: &Attachment) -> bool {
        self.apply_type == slot.apply_type
            && (!attachment.rotated || self.rotatable)
            && if self.scalable {
                (1..=slot.scale).contains(&attachment.scale)
            } else {
                attachment.scale == self.scale.min(slot.scale)
            }
    }
}

#[derive(Clone, Default)]
pub struct VehicleCosmeticSlots {
    pub camouflage: BTreeSet<VehicleSurface>,
    pub paints: BTreeSet<PaintRegion>,
    pub inscriptions: BTreeSet<InscriptionSlot>,
    pub effect_slot: bool,
    pub projection_slots: BTreeMap<u16, ProjectionDecalSlot>,
    pub attachment_slots: BTreeMap<u16, AttachmentSlot>,
    pub stat_tracker_slots: BTreeSet<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PaintRegion {
    Chassis,
    Hull,
    Turret,
    Gun,
    Mantlet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Paint {
    pub id: u16,
    pub units: u32,
}

pub type CamouflageLayout = BTreeMap<VehicleSurface, Camouflage>;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VehicleOutfit {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<u16>,
    pub camouflage: CamouflageLayout,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub paints: BTreeMap<PaintRegion, Paint>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inscriptions: BTreeMap<InscriptionSlot, Inscription>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projection_decals: Vec<ProjectionDecal>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attachments: BTreeMap<u16, Attachment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stat_tracker: Option<StatTracker>,
}

impl VehicleOutfit {
    pub fn is_empty(&self) -> bool {
        self.style.is_none()
            && self.seasonal_placements() == 0
            && self.attachments.is_empty()
            && self.stat_tracker.is_none()
    }

    pub fn supports_season(&self, season: OutfitSeason) -> bool {
        match season {
            OutfitSeason::Common => self.seasonal_placements() == 0,
            _ => self.style.is_none() && self.attachments.is_empty() && self.stat_tracker.is_none(),
        }
    }

    fn seasonal_placements(&self) -> usize {
        self.camouflage.len()
            + self.paints.len()
            + self.inscriptions.len()
            + usize::from(self.effect.is_some())
            + self.projection_decals.len()
    }

    fn items(&self) -> impl Iterator<Item = (CosmeticItem, u32)> + '_ {
        self.camouflage
            .values()
            .map(|item| (CosmeticKind::Camouflage, item.id))
            .chain(self.inscriptions.values().map(Inscription::item))
            .chain(self.effect.map(|id| (CosmeticKind::Effect, id)))
            .chain(self.style.map(|id| (CosmeticKind::Style, id)))
            .chain(
                self.projection_decals
                    .iter()
                    .map(|decal| (CosmeticKind::ProjectionDecal, decal.id)),
            )
            .chain(
                self.attachments
                    .values()
                    .map(|attachment| (CosmeticKind::Attachment, attachment.id)),
            )
            .chain(
                self.stat_tracker
                    .map(|tracker| (CosmeticKind::StatTracker, tracker.id)),
            )
            .map(|item| (item, 1))
            .chain(
                self.paints
                    .values()
                    .map(|paint| ((CosmeticKind::Paint, paint.id), paint.units)),
            )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CosmeticState {
    // Ownership includes mounted copies. Free stock is derived from the outfits.
    owned: BTreeMap<CosmeticKind, BTreeMap<u16, u32>>,
    pub outfits: BTreeMap<i64, BTreeMap<OutfitSeason, VehicleOutfit>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    stat_trackers: BTreeMap<i32, BTreeMap<u16, u32>>,
}

impl CosmeticState {
    pub(crate) fn retain_supported_slots(
        &mut self,
        vehicle: i64,
        slots: &VehicleCosmeticSlots,
        attachment_fits: impl Fn(u16, &Attachment) -> bool,
    ) {
        if let Some(seasons) = self.outfits.get_mut(&vehicle) {
            seasons.retain(|_, outfit| {
                outfit
                    .camouflage
                    .retain(|surface, _| slots.camouflage.contains(surface));
                outfit
                    .paints
                    .retain(|region, _| slots.paints.contains(region));
                outfit
                    .inscriptions
                    .retain(|slot, _| slots.inscriptions.contains(slot));
                if !slots.effect_slot {
                    outfit.effect = None;
                }
                outfit
                    .projection_decals
                    .retain(|decal| slots.projection_slots.contains_key(&decal.slot_id));
                outfit
                    .attachments
                    .retain(|&id, attachment| attachment_fits(id, attachment));
                if outfit
                    .stat_tracker
                    .is_some_and(|tracker| !slots.stat_tracker_slots.contains(&tracker.slot_id))
                {
                    outfit.stat_tracker = None;
                }
                !outfit.is_empty()
            });
            if seasons.is_empty() {
                self.outfits.remove(&vehicle);
            }
        }
    }

    #[tracing::instrument(
        name = "accounts.cosmetics.grant_attachment",
        skip_all,
        fields(ddd.bounded_context.name = "accounts", input.attachment_id = id, input.amount = count, output.owned.count = tracing::field::Empty),
        err
    )]
    pub(crate) fn grant_attachment(
        &mut self,
        id: u16,
        count: u32,
        max_owned: Option<u32>,
    ) -> Result<(), super::GrantError> {
        use super::GrantError;
        if !(1..=MAX_UNITS_PER_PURCHASE).contains(&count) {
            return Err(GrantError::InvalidAmount);
        }
        let owned = self.owned.entry(CosmeticKind::Attachment).or_default();
        let total = owned
            .get(&id)
            .copied()
            .unwrap_or_default()
            .checked_add(count)
            .filter(|&total| max_owned.is_none_or(|limit| total <= limit))
            .ok_or(GrantError::AttachmentLimit)?;
        owned.insert(id, total);
        tracing::Span::current().record("output.owned.count", total);
        Ok(())
    }

    #[tracing::instrument(
        name = "accounts.cosmetics.grant_stat_tracker",
        skip_all,
        fields(ddd.bounded_context.name = "accounts", input.vehicle_type = vehicle_type, input.stat_tracker_id = id, input.amount = count, output.owned.count = tracing::field::Empty),
        err
    )]
    pub(crate) fn grant_stat_tracker(
        &mut self,
        vehicle_type: i32,
        id: u16,
        count: u32,
        max_owned: Option<u32>,
    ) -> Result<(), super::GrantError> {
        use super::GrantError;
        if !(1..=MAX_UNITS_PER_PURCHASE).contains(&count) {
            return Err(GrantError::InvalidAmount);
        }
        let owned = self.stat_trackers.entry(vehicle_type).or_default();
        let total = owned
            .get(&id)
            .copied()
            .unwrap_or_default()
            .checked_add(count)
            .filter(|&total| max_owned.is_none_or(|limit| total <= limit))
            .ok_or(GrantError::StatTrackerLimit)?;
        owned.insert(id, total);
        tracing::Span::current().record("output.owned.count", total);
        Ok(())
    }

    pub fn stat_trackers(&self) -> &BTreeMap<i32, BTreeMap<u16, u32>> {
        &self.stat_trackers
    }

    pub(crate) fn style_owned(&self, id: u16) -> u32 {
        self.owned
            .get(&CosmeticKind::Style)
            .and_then(|styles| styles.get(&id))
            .copied()
            .unwrap_or_default()
    }

    pub(crate) fn style_in_depot(&self, id: u16) -> u32 {
        let mounted = self
            .outfits
            .values()
            .filter(|outfits| {
                outfits
                    .get(&OutfitSeason::Common)
                    .is_some_and(|outfit| outfit.style == Some(id))
            })
            .count();
        self.style_owned(id)
            .saturating_sub(u32::try_from(mounted).unwrap_or(u32::MAX))
    }

    #[tracing::instrument(name = "accounts.cosmetics.add_style", skip_all, fields(ddd.bounded_context.name = "accounts", input.style_id = id, input.count = count, output.owned.count = tracing::field::Empty), err)]
    pub(crate) fn add_style(
        &mut self,
        id: u16,
        count: u32,
        limit: Option<u32>,
    ) -> Result<(), ProgressionError> {
        let total = self
            .style_owned(id)
            .checked_add(count)
            .filter(|&total| limit.is_none_or(|limit| total <= limit))
            .ok_or(ProgressionError::InvalidCosmetic)?;
        self.owned
            .entry(CosmeticKind::Style)
            .or_default()
            .insert(id, total);
        tracing::Span::current().record("output.owned.count", total);
        Ok(())
    }

    pub fn owned(&self) -> &BTreeMap<CosmeticKind, BTreeMap<u16, u32>> {
        &self.owned
    }

    pub fn mounted(&self) -> impl Iterator<Item = (i64, CosmeticItem, u32)> + '_ {
        self.outfits.iter().flat_map(|(&vehicle, seasons)| {
            seasons.values().flat_map(move |outfit| {
                outfit
                    .items()
                    .map(move |(item, units)| (vehicle, item, units))
            })
        })
    }
}

#[derive(Debug, Clone)]
pub struct CosmeticChange(CosmeticAction);

#[derive(Debug, Clone)]
enum CosmeticAction {
    Buy(Vec<(CosmeticItem, u32)>),
    Apply(BTreeMap<OutfitSeason, VehicleOutfit>),
}

impl CosmeticChange {
    pub(crate) fn buy(items: Vec<(CosmeticItem, u32)>) -> Self {
        Self(CosmeticAction::Buy(items))
    }

    pub(crate) fn apply(outfits: BTreeMap<OutfitSeason, VehicleOutfit>) -> Self {
        Self(CosmeticAction::Apply(outfits))
    }

    pub(crate) fn is_purchase(&self) -> bool {
        matches!(self.0, CosmeticAction::Buy(_))
    }

    fn line_count(&self) -> usize {
        match &self.0 {
            CosmeticAction::Buy(items) => items.len(),
            CosmeticAction::Apply(seasons) => seasons
                .values()
                .map(|outfit| {
                    outfit.seasonal_placements()
                        + outfit.attachments.len()
                        + usize::from(outfit.stat_tracker.is_some())
                        + usize::from(outfit.style.is_some())
                })
                .sum(),
        }
    }

    fn operation(&self) -> &'static str {
        match self.0 {
            CosmeticAction::Buy(_) => "buy",
            CosmeticAction::Apply(_) => "apply",
        }
    }
}

#[derive(Clone)]
pub struct CosmeticRule {
    pub price: Option<VehicleCost>,
    pub seasons: BTreeSet<OutfitSeason>,
    pub placement: CosmeticPlacement,
    pub max_owned: Option<u32>,
}

#[derive(Clone)]
pub enum CosmeticPlacement {
    Style,
    Paint {
        units_by_region: BTreeMap<PaintRegion, u32>,
    },
    Camouflage {
        surfaces: BTreeSet<VehicleSurface>,
        palettes: usize,
        pattern_sizes: usize,
    },
    Inscription {
        slots: BTreeSet<InscriptionSlot>,
        digits: Option<usize>,
    },
    Effect,
    Attachment(AttachmentRule),
    StatTracker,
    ProjectionDecal {
        form: Option<String>,
        horizontal_mirror: bool,
        vertical_mirror: bool,
        only_vertical_mirror: bool,
    },
}

#[tracing::instrument(
    name = "accounts.cosmetics.change",
    skip_all,
    fields(
        ddd.bounded_context.name = "accounts",
        user.id = snapshot.id,
        input.inventory_id = vehicle,
        input.vehicle_type = vehicle_type,
        input.revision = snapshot.revision,
        input.operation = change.operation(),
        input.items.count = change.line_count(),
        input.style_id = tracing::field::Empty,
        output.purchases.count = tracing::field::Empty,
        output.outcome = tracing::field::Empty,
        otel.status_code = tracing::field::Empty,
    )
)]
pub fn change_cosmetics(
    snapshot: &mut AccountSnapshot,
    vehicle: i64,
    vehicle_type: Option<i32>,
    change: CosmeticChange,
    rules: &BTreeMap<CosmeticItem, CosmeticRule>,
    prohibited_numbers: &BTreeSet<String>,
    slots: &VehicleCosmeticSlots,
) -> Result<bool, ProgressionError> {
    let result = plan_cosmetics(
        snapshot,
        vehicle,
        vehicle_type,
        change,
        rules,
        prohibited_numbers,
        slots,
    );
    let span = tracing::Span::current();
    match &result {
        Ok(changed) => span.record(
            "output.outcome",
            if *changed { "changed" } else { "unchanged" },
        ),
        Err(error) => {
            span.record("otel.status_code", "ERROR");
            tracing::info!(%error, "Exterior change rejected");
            span.record("output.outcome", "rejected")
        }
    };
    result
}

fn plan_cosmetics(
    snapshot: &mut AccountSnapshot,
    vehicle: i64,
    vehicle_type: Option<i32>,
    change: CosmeticChange,
    rules: &BTreeMap<CosmeticItem, CosmeticRule>,
    prohibited_numbers: &BTreeSet<String>,
    slots: &VehicleCosmeticSlots,
) -> Result<bool, ProgressionError> {
    use ProgressionError::InvalidCosmetic;

    let unbound_purchase = vehicle == 0 && change.is_purchase();
    if !unbound_purchase
        && !snapshot
            .vehicles
            .iter()
            .any(|owned| owned.inventory_id == vehicle)
    {
        return Err(ProgressionError::VehicleNotFound);
    }
    let state = &snapshot.progression.cosmetics;
    let mut next_outfits = None;
    let mut purchases = BTreeMap::new();
    match change.0 {
        CosmeticAction::Buy(lines) => {
            if !(1..=MAX_COSMETIC_PURCHASE_LINES).contains(&lines.len()) {
                return Err(InvalidCosmetic);
            }
            for (id, count) in lines {
                if !(1..=MAX_UNITS_PER_PURCHASE).contains(&count)
                    || purchases.insert(id, count).is_some()
                {
                    return Err(InvalidCosmetic);
                }
            }
        }
        CosmeticAction::Apply(seasons) => {
            if let Some(style) = seasons
                .get(&OutfitSeason::Common)
                .and_then(|outfit| outfit.style)
            {
                tracing::Span::current().record("input.style_id", style);
            }
            let current = state.outfits.get(&vehicle);
            let mut outfits = current.cloned().unwrap_or_default();
            for (season, mut outfit) in seasons {
                if !outfit.supports_season(season) || outfit.attachments.len() > MAX_ATTACHMENTS {
                    return Err(InvalidCosmetic);
                }
                if let Some(style) = outfit.style
                    && !rules
                        .get(&(CosmeticKind::Style, style))
                        .is_some_and(|rule| {
                            matches!(rule.placement, CosmeticPlacement::Style)
                                && rule.seasons.contains(&season)
                        })
                {
                    return Err(InvalidCosmetic);
                }
                if let Some(tracker) = outfit.stat_tracker {
                    let rule = rules
                        .get(&(CosmeticKind::StatTracker, tracker.id))
                        .ok_or(InvalidCosmetic)?;
                    if !matches!(rule.placement, CosmeticPlacement::StatTracker)
                        || !rule.seasons.contains(&season)
                        || !slots.stat_tracker_slots.contains(&tracker.slot_id)
                        || vehicle_type
                            .and_then(|cd| state.stat_trackers.get(&cd))
                            .and_then(|owned| owned.get(&tracker.id))
                            .copied()
                            .unwrap_or_default()
                            == 0
                    {
                        return Err(InvalidCosmetic);
                    }
                }
                for (&slot_id, attachment) in &outfit.attachments {
                    let rule = rules
                        .get(&(CosmeticKind::Attachment, attachment.id))
                        .ok_or(InvalidCosmetic)?;
                    let CosmeticPlacement::Attachment(attachment_rule) = &rule.placement else {
                        return Err(InvalidCosmetic);
                    };
                    let slot = slots
                        .attachment_slots
                        .get(&slot_id)
                        .ok_or(InvalidCosmetic)?;
                    if !rule.seasons.contains(&season) || !attachment_rule.fits(slot, attachment) {
                        return Err(InvalidCosmetic);
                    }
                }
                for (region, paint) in &mut outfit.paints {
                    let rule = rules
                        .get(&(CosmeticKind::Paint, paint.id))
                        .ok_or(InvalidCosmetic)?;
                    let CosmeticPlacement::Paint { units_by_region } = &rule.placement else {
                        return Err(InvalidCosmetic);
                    };
                    if !rule.seasons.contains(&season) {
                        return Err(InvalidCosmetic);
                    }
                    paint.units = *units_by_region.get(region).ok_or(InvalidCosmetic)?;
                }
                for (&surface, item) in &outfit.camouflage {
                    let rule = rules
                        .get(&(CosmeticKind::Camouflage, item.id))
                        .ok_or(InvalidCosmetic)?;
                    let CosmeticPlacement::Camouflage {
                        surfaces,
                        palettes,
                        pattern_sizes,
                    } = &rule.placement
                    else {
                        return Err(InvalidCosmetic);
                    };
                    if !rule.seasons.contains(&season)
                        || !surfaces.contains(&surface)
                        || usize::from(item.palette) >= *palettes
                        || usize::from(item.pattern_size) >= *pattern_sizes
                    {
                        return Err(InvalidCosmetic);
                    }
                }
                for (slot, item) in &outfit.inscriptions {
                    let rule = rules.get(&item.item()).ok_or(InvalidCosmetic)?;
                    let CosmeticPlacement::Inscription { slots, digits } = &rule.placement else {
                        return Err(InvalidCosmetic);
                    };
                    let number_valid = match (item, digits) {
                        (Inscription::Preset(_), None) => true,
                        (Inscription::Number { digits: number, .. }, Some(length)) => {
                            number.len() == *length
                                && number.bytes().all(|digit| digit.is_ascii_digit())
                                && !prohibited_numbers.contains(number)
                        }
                        _ => false,
                    };
                    if !rule.seasons.contains(&season) || !slots.contains(slot) || !number_valid {
                        return Err(InvalidCosmetic);
                    }
                }
                if let Some(effect) = outfit.effect {
                    let rule = rules
                        .get(&(CosmeticKind::Effect, effect))
                        .ok_or(InvalidCosmetic)?;
                    if !matches!(rule.placement, CosmeticPlacement::Effect)
                        || !rule.seasons.contains(&season)
                    {
                        return Err(InvalidCosmetic);
                    }
                }
                if outfit.projection_decals.len() > MAX_PROJECTION_DECALS {
                    return Err(InvalidCosmetic);
                }
                let mut used_slots = BTreeSet::new();
                for decal in &outfit.projection_decals {
                    let rule = rules
                        .get(&(CosmeticKind::ProjectionDecal, decal.id))
                        .ok_or(InvalidCosmetic)?;
                    let CosmeticPlacement::ProjectionDecal {
                        form,
                        horizontal_mirror,
                        vertical_mirror,
                        only_vertical_mirror,
                    } = &rule.placement
                    else {
                        return Err(InvalidCosmetic);
                    };
                    let slot = slots
                        .projection_slots
                        .get(&decal.slot_id)
                        .ok_or(InvalidCosmetic)?;
                    if !rule.seasons.contains(&season)
                        || !slot.supports_form(form.as_deref())
                        || !used_slots.insert(decal.slot_id)
                        || decal.scale > 3
                        || decal.options & !3 != 0
                        || (decal.options & 1 != 0 && !horizontal_mirror && !only_vertical_mirror)
                        || (decal.options & 2 != 0 && (!vertical_mirror || !slot.vertical_mirror))
                        || (*only_vertical_mirror && !matches!(decal.options, 0 | 3))
                    {
                        return Err(InvalidCosmetic);
                    }
                }
                if outfit.is_empty() {
                    outfits.remove(&season);
                } else {
                    outfits.insert(season, outfit);
                }
            }
            if current.map_or(outfits.is_empty(), |current| current == &outfits) {
                tracing::Span::current().record("output.purchases.count", 0);
                return Ok(false);
            }
            let mut free: BTreeMap<_, _> = state
                .owned
                .iter()
                .flat_map(|(&kind, items)| {
                    items.iter().map(move |(&id, &count)| ((kind, id), count))
                })
                .collect();
            for (other, item, units) in state.mounted() {
                if other != vehicle
                    && let Some(count) = free.get_mut(&item)
                {
                    *count = count.saturating_sub(units);
                }
            }
            for (item, units) in outfits
                .values()
                .flat_map(VehicleOutfit::items)
                .filter(|(item, _)| item.0 != CosmeticKind::StatTracker)
            {
                let stock = free.entry(item).or_default();
                let used = units.min(*stock);
                *stock -= used;
                if units > used {
                    let missing: &mut u32 = purchases.entry(item).or_default();
                    *missing = missing.checked_add(units - used).ok_or(InvalidCosmetic)?;
                }
            }
            next_outfits = Some(outfits);
        }
    }

    let mut cost = VehicleCost::default();
    let mut units_purchased = 0u64;
    for (&(kind, id), &count) in &purchases {
        let rule = rules.get(&(kind, id)).ok_or(InvalidCosmetic)?;
        let price = rule.price.ok_or(ProgressionError::NotForSale)?;
        let total = state
            .owned
            .get(&kind)
            .and_then(|items| items.get(&id))
            .copied()
            .unwrap_or(0)
            .checked_add(count)
            .ok_or(InvalidCosmetic)?;
        if rule.max_owned.is_some_and(|max| total > max) {
            return Err(InvalidCosmetic);
        }
        cost = cost.checked_add(price.checked_mul(count)?)?;
        units_purchased += u64::from(count);
    }
    if !purchases.is_empty() {
        cost.charge(snapshot)?;
        for ((kind, id), count) in purchases {
            *snapshot
                .progression
                .cosmetics
                .owned
                .entry(kind)
                .or_default()
                .entry(id)
                .or_default() += count;
        }
    }
    if let Some(outfits) = next_outfits {
        let layouts = &mut snapshot.progression.cosmetics.outfits;
        if outfits.is_empty() {
            layouts.remove(&vehicle);
        } else {
            layouts.insert(vehicle, outfits);
        }
    }
    tracing::Span::current().record("output.purchases.count", units_purchased);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OwnedVehicle;

    #[test]
    fn seasonal_outfits_draw_from_shared_ownership_and_failed_writes_are_atomic() {
        let mut snapshot = AccountSnapshot {
            id: 1,
            display_name: "Player".into(),
            revision: 1,
            balances: super::super::Balances {
                gold: 7,
                ..Default::default()
            },
            crew: None,
            vehicles: [1, 2]
                .map(|inventory_id| OwnedVehicle {
                    inventory_id,
                    catalog_key: String::new(),
                    ammunition: None,
                    has_crew: false,
                })
                .to_vec(),
            depot_shells: Default::default(),
            progression: serde_json::from_value(serde_json::json!({
                "unlocks": [], "vehicle_xp": {}, "depot": {}, "installed": {}
            }))
            .unwrap(),
            settings: Default::default(),
        };
        let rules = BTreeMap::from([(
            (CosmeticKind::Camouflage, 10),
            CosmeticRule {
                price: Some(VehicleCost {
                    credits: 0,
                    gold: 5,
                }),
                seasons: BTreeSet::from([OutfitSeason::Summer, OutfitSeason::Desert]),
                placement: CosmeticPlacement::Camouflage {
                    surfaces: BTreeSet::from([VehicleSurface::Hull]),
                    palettes: 1,
                    pattern_sizes: 3,
                },
                max_owned: None,
            },
        )]);
        let camo = Camouflage {
            id: 10,
            pattern_size: 1,
            palette: 0,
        };
        let layout = BTreeMap::from([(VehicleSurface::Hull, camo)]);
        let apply = |season, layout| {
            CosmeticChange::apply(BTreeMap::from([(
                season,
                VehicleOutfit {
                    camouflage: layout,
                    ..Default::default()
                },
            )]))
        };
        let mount_summer = || apply(OutfitSeason::Summer, layout.clone());
        let mount_desert = || apply(OutfitSeason::Desert, layout.clone());

        assert!(
            change_cosmetics(
                &mut snapshot,
                1,
                None,
                mount_summer(),
                &rules,
                &BTreeSet::new(),
                &VehicleCosmeticSlots::default()
            )
            .unwrap()
        );
        let fitted = snapshot.progression.cosmetics.clone();
        assert_eq!(snapshot.balances.gold, 2);
        assert_eq!(
            &fitted.owned()[&CosmeticKind::Camouflage],
            &BTreeMap::from([(10, 1)])
        );
        assert!(
            !change_cosmetics(
                &mut snapshot,
                1,
                None,
                mount_summer(),
                &rules,
                &BTreeSet::new(),
                &VehicleCosmeticSlots::default()
            )
            .unwrap()
        );

        for (vehicle, change) in [(1, mount_desert()), (2, mount_summer())] {
            assert_eq!(
                change_cosmetics(
                    &mut snapshot,
                    vehicle,
                    None,
                    change,
                    &rules,
                    &BTreeSet::new(),
                    &VehicleCosmeticSlots::default()
                ),
                Err(ProgressionError::NotEnoughMoney)
            );
        }
        let invalid = apply(
            OutfitSeason::Summer,
            BTreeMap::from([(VehicleSurface::Gun, camo)]),
        );
        assert_eq!(
            change_cosmetics(
                &mut snapshot,
                2,
                None,
                invalid,
                &rules,
                &BTreeSet::new(),
                &VehicleCosmeticSlots::default()
            ),
            Err(ProgressionError::InvalidCosmetic)
        );
        assert_eq!(snapshot.balances.gold, 2);
        assert_eq!(snapshot.progression.cosmetics, fitted);

        snapshot.remove_sold_vehicle(1);
        assert!(
            change_cosmetics(
                &mut snapshot,
                2,
                None,
                mount_desert(),
                &rules,
                &BTreeSet::new(),
                &VehicleCosmeticSlots::default()
            )
            .unwrap()
        );
        assert_eq!(snapshot.balances.gold, 2);
        let state = snapshot.progression.cosmetics;
        assert_eq!(
            state.mounted().collect::<Vec<_>>(),
            vec![(2, (CosmeticKind::Camouflage, 10), 1)]
        );
        assert_eq!(
            serde_json::from_value::<CosmeticState>(serde_json::to_value(&state).unwrap()).unwrap(),
            state
        );
    }
}
