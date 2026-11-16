use super::{Account, ClientKey, EntityKey, FAILURE, Gateway, Pickle, PreparedAccount, SUCCESS};
use crate::{
    accounts::{
        DepotSaleRequest, EquipmentChange, MAX_DEPOT_SALE_LINES, MAX_EQUIPMENT_SLOTS,
        ProgressionCommand, ProgressionError, VehicleSaleRequest,
    },
    client_formats::pickle::Value,
};

pub(super) const RESEARCH: i16 = 201;
pub(super) const EQUIP: i16 = 101;
pub(super) const BUY: i16 = 302;
pub(super) const SELL: i16 = 305;
pub(super) const SELL_MULTIPLE: i16 = 310;
pub(super) const BUY_VEHICLE: i16 = 301;
pub(super) const BUY_VEHICLE_SLOT: i16 = 205;
pub(super) const SELL_VEHICLE: i16 = 304;
pub(super) const EQUIP_DEVICE: i16 = 102;
pub(super) const EQUIP_DEVICE_SEQUENCE: i16 = 10060;
pub(super) const BUY_AND_EQUIP: i16 = 308;

pub(super) struct Completion {
    client: ClientKey,
    account_id: i64,
    request_id: i16,
    result: anyhow::Result<Outcome>,
}

enum Outcome {
    Unchanged,
    Committed {
        prepared: Box<PreparedAccount>,
        diff: Value,
        response: Value,
    },
    Rejected(ProgressionError),
    Stale,
}

impl Gateway {
    pub(super) fn change_progression(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
        request_id: i16,
        command: Option<ProgressionCommand>,
        shop_revision: Option<i64>,
    ) -> anyhow::Result<()> {
        let Some(session) = self.sessions.get_mut(&client) else {
            return Ok(());
        };
        let Some(command) = command else {
            self.base.entity(entity)?.onCmdResponse(
                request_id,
                -2,
                "invalidProgressionRequest".into(),
            )?;
            return Ok(());
        };
        if shop_revision.is_some_and(|revision| revision != i64::from(self.catalog.shop_revision()))
        {
            self.base.entity(entity)?.onCmdResponseExt(
                request_id,
                -4,
                String::new(),
                Pickle(crate::pickle!({"shopRev":(self.catalog.shop_revision())})),
            )?;
            return Ok(());
        }
        let is_crew = matches!(command, ProgressionCommand::Crew(_));
        if is_crew
            && session.crew_running
            && session.crew_queue.len() < super::crew::MAX_QUEUED_CREW_COMMANDS
        {
            session
                .crew_queue
                .push_back(super::crew::QueuedCrewCommand {
                    request_id,
                    command,
                    shop_revision,
                });
            return Ok(());
        }
        let permit = (!session.saving)
            .then(|| self.workers.clone().try_acquire_owned().ok())
            .flatten();
        let Some(permit) = permit else {
            self.base
                .entity(entity)?
                .onCmdResponse(request_id, -5, "accountBusy".into())?;
            return Ok(());
        };
        let previous = session.account.snapshot.clone();
        let account_id = previous.id;
        session.saving = true;
        session.crew_running = is_crew;
        self.pending_writes.insert(account_id);
        let catalog = self.catalog.clone();
        let accounts = self.accounts.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let result = async {
                let projection = tokio::task::spawn_blocking(move || {
                    let change = match catalog.plan_progression(previous.clone(), command.clone()) {
                        Ok(change) => change,
                        Err(error) => {
                            tracing::debug!(%error,account_id,"Rejected garage progression command");
                            return Ok(Err(error));
                        }
                    };
                    if change.snapshot.revision == change.previous_revision {
                        return Ok(Ok(None));
                    }
                    let diff = catalog.progression_diff(&previous, &change)?;
                    let response = catalog.progression_response(&previous, &change, &command);
                    let prepared = catalog.prepare(change.snapshot.clone())?;
                    Ok::<_, anyhow::Error>(Ok(Some((change, prepared, diff, response))))
                })
                .await??;
                let (change, prepared, diff, response) = match projection {
                    Ok(Some(values)) => values,
                    Ok(None) => return Ok(Outcome::Unchanged),
                    Err(error)=>return Ok(Outcome::Rejected(error)),
                };
                if !accounts.commit_progression(&change).await? {
                    return Ok(Outcome::Stale);
                }
                Ok(Outcome::Committed {prepared:Box::new(prepared),diff,response})
            }
            .await;
            tokio::task::spawn_blocking(move || {
                let _permit = permit;
                let _ = sender.send(super::Completion::Progression(Completion {
                    client,
                    account_id,
                    request_id,
                    result,
                }));
            })
            .await
            .ok();
        });
        Ok(())
    }

    pub(super) fn complete_progression(&mut self, completion: Completion) {
        let Completion {
            client,
            account_id,
            request_id,
            result,
        } = completion;
        self.pending_writes.remove(&account_id);
        let Some(session) = self.sessions.get_mut(&client) else {
            return;
        };
        session.saving = false;
        session.crew_running = false;
        let Some(entity) = session.entity else {
            return;
        };
        let response = self
            .base
            .entity(entity)
            .and_then(|mut account| match result {
                Ok(Outcome::Committed {
                    prepared,
                    diff,
                    response,
                }) => {
                    session.account = *prepared;
                    account.update(Pickle(diff))?;
                    account.onCmdResponseExt(request_id, SUCCESS, String::new(), Pickle(response))
                }
                Ok(Outcome::Unchanged) => account.onCmdResponseExt(
                    request_id,
                    SUCCESS,
                    String::new(),
                    Pickle(crate::pickle!({})),
                ),
                Ok(Outcome::Rejected(error)) => {
                    account.onCmdResponse(request_id, FAILURE, error.to_string())
                }
                Ok(Outcome::Stale) => {
                    account.onCmdResponse(request_id, FAILURE, "accountChanged".into())
                }
                Err(error) => {
                    tracing::error!(%error,account_id,"Save garage progression failed");
                    account.onCmdResponse(request_id, FAILURE, "databaseUnavailable".into())
                }
            });
        if let Err(error) = response {
            tracing::error!(%error,account_id,"Send garage progression response failed");
        }
        while let Some(queued) = self.sessions.get_mut(&client).and_then(|session| {
            if session.saving {
                return None;
            }
            session.crew_queue.pop_front()
        }) {
            if let Err(error) = self.change_progression(
                client,
                entity,
                queued.request_id,
                Some(queued.command),
                queued.shop_revision,
            ) {
                tracing::error!(%error, account_id, "Start queued crew command failed");
            }
        }
    }
}

pub(super) fn sale_line(item_type: i64, item: i64, count: i64) -> Option<DepotSaleRequest> {
    Some(DepotSaleRequest {
        item_type: u8::try_from(item_type).ok().filter(|&kind| kind > 0)?,
        item: i32::try_from(item).ok().filter(|&item| item > 0)?,
        count: u32::try_from(count).ok().filter(|&count| count > 0)?,
    })
}

pub(super) fn sell_multiple(args: &[i32]) -> Option<ProgressionCommand> {
    let (_, lines) = args.split_first()?;
    let (rows, remainder) = lines.as_chunks::<3>();
    if !remainder.is_empty() || !(1..=MAX_DEPOT_SALE_LINES).contains(&rows.len()) {
        return None;
    }
    rows.iter()
        .map(|&[kind, item, count]| sale_line(kind.into(), item.into(), count.into()))
        .collect::<Option<Vec<_>>>()
        .map(ProgressionCommand::Sell)
}

pub(super) fn sell_vehicle(args: &[i32]) -> Option<ProgressionCommand> {
    let &[_, inventory_id, dismiss_crew @ (0 | 1), ref lists @ ..] = args else {
        return None;
    };
    let (mounted_items, lists) = sale_items(lists)?;
    let (depot_items, remaining) = sale_items(lists)?;
    // A permanent vehicle with the current garage features has no customization,
    // demount-kit or subscription entries and is sold on its own.
    if inventory_id <= 0 || remaining != [0, 0, 0] {
        return None;
    }
    Some(ProgressionCommand::SellVehicle(VehicleSaleRequest {
        inventory_id: i64::from(inventory_id),
        dismiss_crew: dismiss_crew == 1,
        mounted_items: mounted_items.to_vec(),
        depot_items: depot_items.to_vec(),
    }))
}

fn sale_items(args: &[i32]) -> Option<(&[i32], &[i32])> {
    let (&count, rest) = args.split_first()?;
    let count = usize::try_from(count).ok()?;
    if count > MAX_DEPOT_SALE_LINES {
        return None;
    }
    let (items, remaining) = rest.split_at_checked(count)?;
    items
        .iter()
        .all(|&item| item > 0)
        .then_some((items, remaining))
}

pub(super) fn equip(args: &[i32]) -> Option<ProgressionCommand> {
    let &[_, item, inventory_id, slot, removal, gun] = args else {
        return None;
    };
    if item <= 0 || inventory_id <= 0 || slot != 0 || removal != 0 || gun < 0 {
        return None;
    }
    Some(ProgressionCommand::Equip {
        inventory_id: i64::from(inventory_id),
        item,
        gun,
        buy: true,
    })
}

pub(super) fn equip_devices(command: i16, args: &[i32]) -> Option<ProgressionCommand> {
    let &[_, inventory_id, ref payload @ ..] = args else {
        return None;
    };
    if inventory_id <= 0 {
        return None;
    }
    let change = match (command, payload) {
        (EQUIP_DEVICE_SEQUENCE, &[count, ref items @ ..])
            if usize::try_from(count).ok() == Some(items.len())
                && items.len() <= MAX_EQUIPMENT_SLOTS
                && items.iter().all(|&item| item >= 0) =>
        {
            EquipmentChange::Apply(items.to_vec())
        }
        (EQUIP_DEVICE, &[item, slot, 0 | 1, paid_removal @ (0 | 1), 0])
            if item >= 0 && (item == 0 || paid_removal == 0) =>
        {
            let slot = usize::try_from(slot)
                .ok()
                .filter(|&slot| slot < MAX_EQUIPMENT_SLOTS)?;
            EquipmentChange::SetSlot {
                slot,
                item,
                paid_removal: paid_removal != 0,
            }
        }
        _ => return None,
    };
    Some(ProgressionCommand::Equipment {
        inventory_id: i64::from(inventory_id),
        change,
    })
}

pub(super) fn buy_vehicle(args: &[i32]) -> Option<ProgressionCommand> {
    use crate::accounts::{CrewRecruitment, MAX_CONSUMABLE_SLOTS, VehiclePurchaseOptions};
    let &[
        _,
        vehicle,
        flags,
        crew_type,
        rent_period,
        consumables_len,
        ref layout @ ..,
    ] = args
    else {
        return None;
    };
    if vehicle <= 0
        || flags & !17 != 0
        || rent_period != -1
        || usize::try_from(consumables_len).ok()? != layout.len()
        || layout.len() > 2 * MAX_CONSUMABLE_SLOTS
    {
        return None;
    }
    let (pairs, []) = layout.as_chunks::<2>() else {
        return None;
    };
    let crew = match (flags & 1 != 0, crew_type) {
        (true, 0) => Some(CrewRecruitment::Free),
        (true, 1) => Some(CrewRecruitment::Credits),
        (true, 2) => Some(CrewRecruitment::Gold),
        (false, -1..=2) => None,
        _ => return None,
    };
    Some(ProgressionCommand::BuyVehicle {
        vehicle,
        options: VehiclePurchaseOptions {
            crew,
            ammunition: flags & 16 != 0,
        },
        consumables: pairs
            .iter()
            .copied()
            .map(super::expendables::consumable)
            .collect::<Option<_>>()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_commands_do_not_prevent_valid_requests() {
        for args in [
            vec![],
            vec![1, 2, 3],
            vec![1, 2, 3, 0, 0, i32::MIN],
            vec![1, 2, 3, 1, 0, 0],
            vec![1, 2, 3, 0, 1, 0],
            vec![1, -2, 3, 0, 0, 0],
        ] {
            assert!(equip(&args).is_none());
        }
        assert!(equip(&[1, 2, 3, 0, 0, 0]).is_some());
    }
    #[test]
    fn rejects_malformed_vehicle_purchases_then_accepts_native_options() {
        for args in [
            vec![],
            vec![1, 2065, 17, 0, -1],
            vec![1, 2065, 17, 0, -1, 1],
            vec![1, 2065, 17, 0, -1, 0, 99],
            vec![1, 2065, -1, 0, -1, 0],
            vec![1, 2065, 2, 0, -1, 0],
            vec![1, 2065, 1, -1, -1, 0],
            vec![1, 2065, 17, 0, 7, 0],
            vec![1, i32::MIN, 17, 0, -1, 0],
        ] {
            assert!(buy_vehicle(&args).is_none(), "{args:?}");
        }
        for args in [
            [1, 2065, 17, 0, -1, 0],
            [1, 2065, 0, -1, -1, 0],
            [1, 2065, 1, 2, -1, 0],
        ] {
            assert!(buy_vehicle(&args).is_some());
        }
    }
}
