use super::{AccountSnapshot, ProgressionError, VehicleCost};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_COSMETIC_PURCHASE_LINES: usize = 12;
const MAX_UNITS_PER_PURCHASE: u32 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CosmeticKind {
    Camouflage,
    Inscription,
    PersonalNumber,
    Effect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum OutfitSeason {
    Winter,
    Summer,
    Desert,
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

pub type CamouflageLayout = BTreeMap<VehicleSurface, Camouflage>;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VehicleOutfit {
    pub camouflage: CamouflageLayout,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inscriptions: BTreeMap<InscriptionSlot, Inscription>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<u16>,
}

impl VehicleOutfit {
    pub fn is_empty(&self) -> bool {
        self.camouflage.is_empty() && self.inscriptions.is_empty() && self.effect.is_none()
    }

    fn items(&self) -> impl Iterator<Item = CosmeticItem> + '_ {
        self.camouflage
            .values()
            .map(|item| (CosmeticKind::Camouflage, item.id))
            .chain(self.inscriptions.values().map(Inscription::item))
            .chain(self.effect.map(|id| (CosmeticKind::Effect, id)))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CosmeticState {
    // Ownership includes mounted copies. Free stock is derived from the outfits.
    owned: BTreeMap<CosmeticKind, BTreeMap<u16, u32>>,
    pub outfits: BTreeMap<i64, BTreeMap<OutfitSeason, VehicleOutfit>>,
}

impl CosmeticState {
    pub(crate) fn retain_supported_slots(
        &mut self,
        vehicle: i64,
        supported: &BTreeSet<VehicleSurface>,
        inscription_slots: &BTreeSet<InscriptionSlot>,
        supports_effect: bool,
    ) {
        if let Some(seasons) = self.outfits.get_mut(&vehicle) {
            seasons.retain(|_, outfit| {
                outfit
                    .camouflage
                    .retain(|surface, _| supported.contains(surface));
                outfit
                    .inscriptions
                    .retain(|slot, _| inscription_slots.contains(slot));
                if !supports_effect {
                    outfit.effect = None;
                }
                !outfit.is_empty()
            });
            if seasons.is_empty() {
                self.outfits.remove(&vehicle);
            }
        }
    }

    pub fn owned(&self) -> &BTreeMap<CosmeticKind, BTreeMap<u16, u32>> {
        &self.owned
    }

    pub fn mounted(&self) -> impl Iterator<Item = (i64, CosmeticItem)> + '_ {
        self.outfits.iter().flat_map(|(&vehicle, seasons)| {
            seasons
                .values()
                .flat_map(move |outfit| outfit.items().map(move |item| (vehicle, item)))
        })
    }
}

#[derive(Debug, Clone)]
pub enum CosmeticChange {
    Buy(Vec<(CosmeticItem, u32)>),
    Apply(BTreeMap<OutfitSeason, VehicleOutfit>),
}

impl CosmeticChange {
    pub fn line_count(&self) -> usize {
        match self {
            Self::Buy(items) => items.len(),
            Self::Apply(seasons) => seasons
                .values()
                .map(|outfit| {
                    outfit.camouflage.len()
                        + outfit.inscriptions.len()
                        + usize::from(outfit.effect.is_some())
                })
                .sum(),
        }
    }

    pub fn operation(&self) -> &'static str {
        match self {
            Self::Buy(_) => "buy",
            Self::Apply(_) => "apply",
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
}

#[tracing::instrument(
    name = "accounts.cosmetics.change",
    skip_all,
    fields(
        ddd.bounded_context.name = "accounts",
        user.id = snapshot.id,
        input.inventory_id = vehicle,
        input.revision = snapshot.revision,
        input.operation = change.operation(),
        input.items.count = change.line_count(),
        output.purchases.count = tracing::field::Empty,
        output.outcome = tracing::field::Empty,
        otel.status_code = tracing::field::Empty,
    )
)]
pub fn change_cosmetics(
    snapshot: &mut AccountSnapshot,
    vehicle: i64,
    change: CosmeticChange,
    rules: &BTreeMap<CosmeticItem, CosmeticRule>,
    prohibited_numbers: &BTreeSet<String>,
) -> Result<bool, ProgressionError> {
    let result = plan_cosmetics(snapshot, vehicle, change, rules, prohibited_numbers);
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
    change: CosmeticChange,
    rules: &BTreeMap<CosmeticItem, CosmeticRule>,
    prohibited_numbers: &BTreeSet<String>,
) -> Result<bool, ProgressionError> {
    use ProgressionError::InvalidCosmetic;

    let unbound_purchase = vehicle == 0 && matches!(change, CosmeticChange::Buy(_));
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
    match change {
        CosmeticChange::Buy(lines) => {
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
        CosmeticChange::Apply(seasons) => {
            let current = state.outfits.get(&vehicle);
            let mut outfits = current.cloned().unwrap_or_default();
            for (season, outfit) in seasons {
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
            for (other, item) in state.mounted() {
                if other != vehicle
                    && let Some(count) = free.get_mut(&item)
                {
                    *count = count.saturating_sub(1);
                }
            }
            for item in outfits.values().flat_map(VehicleOutfit::items) {
                if let Some(count) = free.get_mut(&item).filter(|count| **count > 0) {
                    *count -= 1;
                } else {
                    *purchases.entry(item).or_default() += 1;
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
    use crate::accounts::OwnedVehicle;

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
            CosmeticChange::Apply(BTreeMap::from([(
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
            change_cosmetics(&mut snapshot, 1, mount_summer(), &rules, &BTreeSet::new()).unwrap()
        );
        let fitted = snapshot.progression.cosmetics.clone();
        assert_eq!(snapshot.balances.gold, 2);
        assert_eq!(
            &fitted.owned()[&CosmeticKind::Camouflage],
            &BTreeMap::from([(10, 1)])
        );
        assert!(
            !change_cosmetics(&mut snapshot, 1, mount_summer(), &rules, &BTreeSet::new()).unwrap()
        );

        for (vehicle, change) in [(1, mount_desert()), (2, mount_summer())] {
            assert_eq!(
                change_cosmetics(&mut snapshot, vehicle, change, &rules, &BTreeSet::new()),
                Err(ProgressionError::NotEnoughMoney)
            );
        }
        let invalid = apply(
            OutfitSeason::Summer,
            BTreeMap::from([(VehicleSurface::Gun, camo)]),
        );
        assert_eq!(
            change_cosmetics(&mut snapshot, 2, invalid, &rules, &BTreeSet::new()),
            Err(ProgressionError::InvalidCosmetic)
        );
        assert_eq!(snapshot.balances.gold, 2);
        assert_eq!(snapshot.progression.cosmetics, fitted);

        snapshot.remove_sold_vehicle(1);
        assert!(
            change_cosmetics(&mut snapshot, 2, mount_desert(), &rules, &BTreeSet::new()).unwrap()
        );
        assert_eq!(snapshot.balances.gold, 2);
        let state = snapshot.progression.cosmetics;
        assert_eq!(
            state.mounted().collect::<Vec<_>>(),
            vec![(2, (CosmeticKind::Camouflage, 10))]
        );
        assert_eq!(
            serde_json::from_value::<CosmeticState>(serde_json::to_value(&state).unwrap()).unwrap(),
            state
        );
    }
}
