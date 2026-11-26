use super::{Account, ClientKey, EntityKey, FAILURE, Gateway, Pickle, PreparedAccount, SUCCESS};
use wot_accounts::contract::{
    DepotSaleRequest, EquipmentChange, EquipmentUpgradeSource, MAX_DEPOT_SALE_LINES,
    MAX_EQUIPMENT_SLOTS, ProgressionCommand, ProgressionError, VehicleSaleRequest,
    garage::GarageWrite,
};
use wot_platform::client_formats::pickle::Value;

pub(super) const RESEARCH: i16 = 201;
pub(super) const EQUIP: i16 = 101;
pub(super) const BUY: i16 = 302;
pub(super) const SELL: i16 = 305;
pub(super) const SELL_MULTIPLE: i16 = 310;
pub(super) const BUY_VEHICLE: i16 = 301;
pub(super) const BUY_PREMIUM: i16 = 204;
pub(super) const BUY_VEHICLE_SLOT: i16 = 205;
pub(super) const SELL_VEHICLE: i16 = 304;
pub(super) const EQUIP_DEVICE: i16 = 102;
pub(super) const EQUIP_DEVICE_SEQUENCE: i16 = 10060;
pub(super) const DISASSEMBLE_DEVICE: i16 = 10115;
pub(super) const UPGRADE_DEVICE: i16 = 10050;
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
    #[tracing::instrument(name = "player_sessions.gateway.change_progression", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.request_id = request_id, input.has_command = command.is_some(), input.shop_revision = shop_revision, output.outcome = tracing::field::Empty, otel.status_code = tracing::field::Empty), err)]
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
            let span = tracing::Span::current();
            span.record("output.outcome", "invalid_request");
            span.record("otel.status_code", "ERROR");
            tracing::info!("Garage progression request could not be decoded");
            self.base.entity(entity)?.onCmdResponse(
                request_id,
                -2,
                "invalidProgressionRequest".into(),
            )?;
            return Ok(());
        };
        if shop_revision.is_some_and(|revision| revision != i64::from(self.catalog.shop_revision()))
        {
            tracing::Span::current().record("output.outcome", "stale_shop");
            self.base.entity(entity)?.onCmdResponseExt(
                request_id,
                -4,
                String::new(),
                Pickle(wot_platform::pickle!({"shopRev":(self.catalog.shop_revision())})),
            )?;
            return Ok(());
        }
        let is_crew = matches!(command, ProgressionCommand::Crew(_));
        if is_crew
            && session.crew_running
            && session.crew_queue.len() < super::crew::MAX_QUEUED_CREW_COMMANDS
        {
            tracing::Span::current().record("output.outcome", "queued");
            session
                .crew_queue
                .push_back(super::crew::QueuedCrewCommand {
                    parent: tracing::Span::current(),
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
            tracing::Span::current().record("output.outcome", "busy");
            self.base
                .entity(entity)?
                .onCmdResponse(request_id, -5, "accountBusy".into())?;
            return Ok(());
        };
        tracing::Span::current().record("output.outcome", "started");
        let revision = session.account.snapshot.revision();
        let account_id = session.account.snapshot.id();
        session.saving = true;
        session.crew_running = is_crew;
        self.pending_writes.insert(account_id);
        let garage = self.garage.clone();
        let sender = self.sender.clone();
        self.runtime
            .spawn(tracing::Instrument::in_current_span(async move {
                let result = garage
                    .change_progression(account_id, revision, command)
                    .await
                    .map(|outcome| match outcome {
                        GarageWrite::Unchanged => Outcome::Unchanged,
                        GarageWrite::Rejected(error) => Outcome::Rejected(error),
                        GarageWrite::Stale => Outcome::Stale,
                        GarageWrite::Committed { change, receipt } => Outcome::Committed {
                            prepared: Box::new(change.prepared),
                            diff: change.diff,
                            response: receipt,
                        },
                    });
                tokio::task::spawn_blocking(wot_platform::observability::in_current_span(
                    move || {
                        let _permit = permit;
                        let _ = sender.send(super::TracedCompletion::new(
                            super::Completion::Progression(Completion {
                                client,
                                account_id,
                                request_id,
                                result,
                            }),
                        ));
                    },
                ))
                .await
                .ok();
            }));
        Ok(())
    }

    #[tracing::instrument(name = "player_sessions.gateway.progression_response", skip_all, fields(ddd.bounded_context.name = "player_sessions", user.id = completion.account_id, input.request_id = completion.request_id, output.outcome = tracing::field::Empty, output.delivery = tracing::field::Empty, otel.status_code = tracing::field::Empty))]
    pub(super) fn complete_progression(&mut self, completion: Completion) {
        let Completion {
            client,
            account_id,
            request_id,
            result,
        } = completion;
        let span = tracing::Span::current();
        span.record(
            "output.outcome",
            match &result {
                Ok(Outcome::Committed { .. }) => "committed",
                Ok(Outcome::Unchanged) => "unchanged",
                Ok(Outcome::Rejected(_)) => "rejected",
                Ok(Outcome::Stale) => "stale",
                Err(_) => "error",
            },
        );
        if let Err(error) = &result {
            span.record("otel.status_code", "ERROR");
            tracing::error!(%error, account_id, "Save garage progression failed");
        }
        self.finish_account_write(account_id);
        let Some(session) = self.sessions.get_mut(&client) else {
            span.record("output.delivery", "unavailable");
            return;
        };
        session.saving = false;
        session.crew_running = false;
        let Some(entity) = session.entity else {
            span.record("output.delivery", "unavailable");
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
                    Pickle(wot_platform::pickle!({})),
                ),
                Ok(Outcome::Rejected(error)) => {
                    account.onCmdResponse(request_id, FAILURE, error.to_string())
                }
                Ok(Outcome::Stale) => {
                    account.onCmdResponse(request_id, FAILURE, "accountChanged".into())
                }
                Err(_) => account.onCmdResponse(request_id, FAILURE, "databaseUnavailable".into()),
            });
        match response {
            Ok(()) => {
                span.record("output.delivery", "queued");
            }
            Err(error) => {
                span.record("output.delivery", "failed");
                span.record("otel.status_code", "ERROR");
                tracing::error!(%error,account_id,"Send garage progression response failed");
            }
        }
        while let Some(queued) = self.sessions.get_mut(&client).and_then(|session| {
            if session.saving {
                return None;
            }
            session.crew_queue.pop_front()
        }) {
            queued.parent.in_scope(|| {
                if let Err(error) = self.change_progression(
                    client,
                    entity,
                    queued.request_id,
                    Some(queued.command),
                    queued.shop_revision,
                ) {
                    tracing::error!(%error, account_id, "Start queued crew command failed");
                }
            });
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

pub(super) fn upgrade_device(args: &[i32]) -> Option<ProgressionCommand> {
    let source = match *args {
        [_, item, 0, 0, 0] if item > 0 => EquipmentUpgradeSource::Depot { item },
        [_, 0, inventory_id, 0, slot] if inventory_id > 0 => EquipmentUpgradeSource::Mounted {
            inventory_id: i64::from(inventory_id),
            slot: usize::try_from(slot)
                .ok()
                .filter(|&slot| slot < MAX_EQUIPMENT_SLOTS)?,
        },
        _ => return None,
    };
    Some(ProgressionCommand::UpgradeEquipment(source))
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
        (DISASSEMBLE_DEVICE, &[item]) if item > 0 => EquipmentChange::Disassemble { item },
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
    use wot_accounts::contract::{CrewRecruitment, MAX_CONSUMABLE_SLOTS, VehiclePurchaseOptions};
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
