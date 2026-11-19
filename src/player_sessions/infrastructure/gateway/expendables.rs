use super::ammunition::decode_shells;
use crate::accounts::contract::{ConsumableSelection, MAX_CONSUMABLE_SLOTS, ProgressionCommand};

pub(super) const VEHICLE_SETTINGS: i16 = 107;
pub(super) const AUTO_CONSUMABLES: i64 = 8;
pub(super) const AUTO_DIRECTIVES: i64 = 128;
const REGULAR: i32 = 0;
const DIRECTIVES: i32 = 1;

fn split(args: &[i32]) -> Option<(i64, &[i32], &[i32])> {
    let &[_, vehicle, shell_length, ref layouts @ ..] = args else {
        return None;
    };
    if vehicle <= 0 {
        return None;
    }
    let (shells, equipment) = layouts.split_at_checked(usize::try_from(shell_length).ok()?)?;
    Some((i64::from(vehicle), shells, equipment))
}

pub(super) fn has_layout(args: &[i32]) -> bool {
    split(args).is_some_and(|(_, _, equipment)| equipment != [REGULAR, 0])
}

pub(super) fn install(args: &[i32]) -> Option<ProgressionCommand> {
    let (inventory_id, shells, equipment) = split(args)?;
    let &[kind, length, ref layout @ ..] = equipment else {
        return None;
    };
    if usize::try_from(length).ok()? != layout.len()
        || layout.len() > 2 * (MAX_CONSUMABLE_SLOTS + 1)
    {
        return None;
    }
    let (pairs, []) = layout.as_chunks::<2>() else {
        return None;
    };
    match kind {
        REGULAR => Some(ProgressionCommand::FillConsumables {
            inventory_id,
            slots: pairs
                .iter()
                .copied()
                .map(consumable)
                .collect::<Option<_>>()?,
            shells: decode_shells(shells)?,
        }),
        DIRECTIVES if shells.is_empty() => {
            let (directive, slots) = pairs.split_last()?;
            Some(ProgressionCommand::InstallDirective {
                inventory_id,
                installed_consumables: slots
                    .iter()
                    .map(|&pair| Some(consumable(pair)?.item))
                    .collect::<Option<_>>()?,
                item: decode_directive(*directive)?,
            })
        }
        _ => None,
    }
}

pub(super) fn consumable([id, count]: [i32; 2]) -> Option<ConsumableSelection> {
    match (id, count) {
        (0, 0) => Some(ConsumableSelection {
            item: 0,
            alternate_price: false,
        }),
        (_, 1) if id != 0 => Some(ConsumableSelection {
            item: id.checked_abs()?,
            alternate_price: id < 0,
        }),
        _ => None,
    }
}

fn decode_directive(pair: [i32; 2]) -> Option<Option<i32>> {
    match pair {
        [0, 0] => Some(None),
        [id, 1] if id > 0 => Some(Some(id)),
        _ => None,
    }
}

pub(super) fn auto_resupply(
    inventory_id: i64,
    flag: i64,
    enabled: i64,
) -> Option<ProgressionCommand> {
    if inventory_id <= 0 || !matches!(enabled, 0 | 1) {
        return None;
    }
    let enabled = enabled == 1;
    match flag {
        AUTO_CONSUMABLES => Some(ProgressionCommand::ConsumableAutoResupply {
            inventory_id,
            enabled,
        }),
        AUTO_DIRECTIVES => Some(ProgressionCommand::DirectiveAutoResupply {
            inventory_id,
            enabled,
        }),
        _ => None,
    }
}
