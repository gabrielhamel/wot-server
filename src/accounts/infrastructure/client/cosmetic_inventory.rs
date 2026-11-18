use super::{
    cosmetic_catalog::{CAMOUFLAGE_TYPE, cosmetic_subtype},
    outfit,
    progression::changed_rows,
};
use crate::{
    accounts::{CosmeticItem, CosmeticState},
    platform::client_formats::pickle::Value,
};
use std::collections::BTreeMap;

const ITEMS: i32 = 1;
const OUTFITS: i32 = 2;
const DRESSED: i32 = 4;

type ItemRows = BTreeMap<u8, BTreeMap<Value, Value>>;

pub(super) struct CosmeticInventory {
    stock: ItemRows,
    outfits: BTreeMap<Value, Value>,
    dressed: ItemRows,
}

impl CosmeticInventory {
    pub(super) fn project(state: &CosmeticState, vehicle_types: &BTreeMap<i64, i32>) -> Self {
        let mut fitted = BTreeMap::<CosmeticItem, BTreeMap<i32, u32>>::new();
        for (vehicle, item) in state.mounted() {
            if let Some(&vehicle_type) = vehicle_types.get(&vehicle) {
                *fitted
                    .entry(item)
                    .or_default()
                    .entry(vehicle_type)
                    .or_default() += 1;
            }
        }
        let mut stock = ItemRows::from([(CAMOUFLAGE_TYPE, BTreeMap::new())]);
        for (&kind, items) in state.owned() {
            for (&id, &count) in items {
                let installed = fitted
                    .get(&(kind, id))
                    .into_iter()
                    .flat_map(BTreeMap::values)
                    .copied()
                    .sum();
                let free = count.saturating_sub(installed);
                if free > 0 {
                    stock
                        .entry(cosmetic_subtype(kind))
                        .or_default()
                        .insert(Value::from(i32::from(id)), crate::pickle!({0: (free)}));
                }
            }
        }
        let mut dressed = ItemRows::from([(CAMOUFLAGE_TYPE, BTreeMap::new())]);
        for ((kind, id), vehicles) in fitted {
            dressed.entry(cosmetic_subtype(kind)).or_default().insert(
                Value::from(i32::from(id)),
                Value::Dict(
                    vehicles
                        .into_iter()
                        .map(|(cd, count)| (Value::from(cd), Value::from(count)))
                        .collect(),
                ),
            );
        }
        let outfits = state
            .outfits
            .iter()
            .filter_map(|(vehicle, seasons)| {
                let cd = *vehicle_types.get(vehicle)?;
                let outfits = seasons
                    .iter()
                    .map(|(&season, layout)| {
                        (
                            Value::from(outfit::season_code(season)),
                            Value::Bytes(outfit::encode(layout)),
                        )
                    })
                    .collect();
                Some((Value::from(cd), Value::Dict(outfits)))
            })
            .collect();
        Self {
            stock,
            outfits,
            dressed,
        }
    }

    pub(super) fn full(self) -> Value {
        crate::pickle!({
            ITEMS: (item_rows(self.stock)),
            OUTFITS: (Value::Dict(self.outfits)),
            DRESSED: (item_rows(self.dressed)),
        })
    }

    pub(super) fn diff(&self, before: &Self) -> BTreeMap<Value, Value> {
        let outfits = changed_rows(&before.outfits, &self.outfits)
            .into_iter()
            .map(|(vehicle, value)| {
                // ItemsRequester invalidates the vehicle and each season listed in the diff.
                let seasons = match (before.outfits.get(&vehicle), value) {
                    (Some(Value::Dict(before)), Value::Dict(after)) => {
                        Value::Dict(changed_rows(before, &after))
                    }
                    (_, value) => value,
                };
                (vehicle, seasons)
            })
            .collect();
        [
            (ITEMS, binding_diff(&before.stock, &self.stock)),
            (OUTFITS, outfits),
            (DRESSED, binding_diff(&before.dressed, &self.dressed)),
        ]
        .into_iter()
        .filter(|(_, rows)| !rows.is_empty())
        .map(|(table, rows)| (Value::from(table), Value::Dict(rows)))
        .collect()
    }
}

fn item_rows(rows: ItemRows) -> Value {
    Value::Dict(
        rows.into_iter()
            .map(|(kind, rows)| (Value::from(i32::from(kind)), Value::Dict(rows)))
            .collect(),
    )
}

fn binding_diff(before: &ItemRows, after: &ItemRows) -> BTreeMap<Value, Value> {
    let empty = BTreeMap::new();
    before
        .keys()
        .chain(after.keys())
        .copied()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .filter_map(|kind| {
            let items = changed_rows(
                before.get(&kind).unwrap_or(&empty),
                after.get(&kind).unwrap_or(&empty),
            );
            if items.is_empty() {
                return None;
            }
            let replaced = items
                .into_iter()
                .map(|(id, value)| {
                    let key = if value == Value::None {
                        id
                    } else {
                        Value::Tuple(vec![id, Value::from("_r")])
                    };
                    (key, value)
                })
                .collect();
            Some((Value::from(i32::from(kind)), Value::Dict(replaced)))
        })
        .collect()
}
