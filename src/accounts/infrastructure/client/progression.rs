use super::{GarageCatalog, items, shop};
use crate::{
    accounts::{
        AccountSnapshot, ModuleKind, ProgressionChange, ProgressionCommand, ProgressionError,
        VehicleAmmunition, VehicleCost,
    },
    platform::client_formats::pickle::Value,
};
use std::collections::{BTreeMap, BTreeSet};

impl GarageCatalog {
    pub(super) fn elite_vehicles(
        &self,
        snapshot: &AccountSnapshot,
        defaults: &BTreeSet<i32>,
    ) -> anyhow::Result<BTreeSet<i32>> {
        let mut elite = BTreeSet::new();
        for owned in &snapshot.vehicles {
            let template = self.vehicle(&owned.catalog_key)?;
            if template.is_elite(defaults, &snapshot.progression.unlocks) {
                elite.insert(template.modules.rules.vehicle);
            }
        }
        Ok(elite)
    }

    pub(super) fn default_unlocks(
        &self,
        snapshot: &AccountSnapshot,
    ) -> anyhow::Result<BTreeSet<i32>> {
        let mut keys: BTreeSet<_> = snapshot
            .vehicles
            .iter()
            .map(|v| v.catalog_key.as_str())
            .collect();
        for id in &snapshot.progression.unlocks {
            if let Some(offer) = self.offers.get(id) {
                keys.insert(&offer.key);
            }
        }
        let mut unlocks = BTreeSet::new();
        for key in keys {
            let template = self.vehicle(key)?;
            unlocks.insert(template.modules.rules.vehicle);
            unlocks.extend(template.modules.rules.stock.values());
        }
        Ok(unlocks)
    }

    #[tracing::instrument(name = "accounts.progression.plan_progression", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = snapshot.id, input.revision = snapshot.revision), err)]
    pub fn plan_progression(
        &self,
        mut snapshot: AccountSnapshot,
        command: ProgressionCommand,
    ) -> Result<ProgressionChange, ProgressionError> {
        let previous_revision = snapshot.revision;
        let defaults = self
            .default_unlocks(&snapshot)
            .map_err(Self::catalog_failure)?;
        let mut acquired_vehicle = None;
        let mut sold_vehicle = None;
        let mut hired_crew_member = None;
        let mut changed = true;
        let mut consumable_purchases = Vec::new();
        let mut shell_purchases = Vec::new();
        match command {
            ProgressionCommand::Crew(command) => {
                let mut roster = self.crew_roster(&snapshot).map_err(Self::catalog_failure)?;
                let before = roster.clone();
                let balances = snapshot.balances.clone();
                hired_crew_member =
                    self.apply_crew(&mut snapshot, &mut roster, command, &defaults)?;
                changed = roster != before || snapshot.balances != balances;
                snapshot.crew = Some(roster);
            }
            ProgressionCommand::Cosmetics {
                inventory_id,
                change,
            } => {
                changed = self.change_cosmetics(&mut snapshot, inventory_id, change)?;
            }
            ProgressionCommand::Sell(items) => self.sell_depot_items(&mut snapshot, items)?,
            ProgressionCommand::SellVehicle(sale) => {
                let id = sale.inventory_id;
                self.sell_vehicle(&mut snapshot, sale)?;
                sold_vehicle = Some(id);
            }
            ProgressionCommand::Buy { item, count }
                if items::IntCompactDescr::from_int(i64::from(item)).item_type
                    == items::item_type::OPTIONAL_DEVICE =>
            {
                self.equipment.buy(&mut snapshot, item, count)?;
            }
            ProgressionCommand::Equipment {
                inventory_id,
                change,
            } => {
                let owned = snapshot
                    .vehicles
                    .iter()
                    .find(|vehicle| vehicle.inventory_id == inventory_id)
                    .ok_or(ProgressionError::VehicleNotFound)?;
                let template = self
                    .vehicle(&owned.catalog_key)
                    .map_err(Self::catalog_failure)?;
                changed = self.equipment.apply(
                    &mut snapshot,
                    inventory_id,
                    &template.equipment,
                    change,
                )?;
            }
            ProgressionCommand::FillConsumables {
                inventory_id,
                slots,
                shells,
            } => {
                let vehicle = self.owned_vehicle_template(&snapshot, inventory_id)?;
                let slot_count = vehicle.supply_slots.consumables;
                let (regular, trailing) = slots
                    .split_at_checked(slot_count)
                    .ok_or(ProgressionError::InvalidConsumables)?;
                let installed = snapshot.progression.directive_slot(inventory_id).installed;
                match trailing {
                    // AutoFillVehicleLayoutProcessor sends shells and regular slots only.
                    [] if !shells.is_empty() || vehicle.supply_slots.directives == 0 => {}
                    [directive]
                        if shells.is_empty()
                            && vehicle.supply_slots.directives == 1
                            && !directive.alternate_price
                            && (directive.item != 0).then_some(directive.item) == installed => {}
                    _ => return Err(ProgressionError::InvalidConsumables),
                }
                let previous = snapshot.progression.vehicle_consumables(inventory_id);
                consumable_purchases = self.consumables.fill(
                    &mut snapshot,
                    inventory_id,
                    &vehicle.equipment,
                    slot_count,
                    regular,
                )?;
                changed = previous != snapshot.progression.vehicle_consumables(inventory_id);
                if !shells.is_empty() {
                    let ammunition = self
                        .plan_ammunition(snapshot, inventory_id, &shells)
                        .map_err(ProgressionError::Ammunition)?;
                    changed |= ammunition.changed();
                    shell_purchases = ammunition.purchases;
                    snapshot = ammunition.snapshot;
                }
            }
            ProgressionCommand::ConsumableAutoResupply {
                inventory_id,
                enabled,
            } => {
                let vehicle = self.owned_vehicle_template(&snapshot, inventory_id)?;
                if vehicle.supply_slots.consumables == 0 {
                    return Err(ProgressionError::InvalidConsumables);
                }
                let mut state = snapshot.progression.vehicle_consumables(inventory_id);
                changed = state.auto_resupply != enabled;
                state.auto_resupply = enabled;
                snapshot
                    .progression
                    .set_vehicle_consumables(inventory_id, state);
            }
            ProgressionCommand::InstallDirective {
                inventory_id,
                installed_consumables,
                item,
            } => {
                let vehicle = self.directive_vehicle(&snapshot, inventory_id)?;
                let state = snapshot.progression.vehicle_consumables(inventory_id);
                if state.layout.get(..vehicle.supply_slots.consumables)
                    != Some(installed_consumables.as_slice())
                {
                    return Err(ProgressionError::InvalidDirective);
                }
                changed = self.directives.install(&mut snapshot, inventory_id, item)?;
            }
            ProgressionCommand::DirectiveAutoResupply {
                inventory_id,
                enabled,
            } => {
                self.directive_vehicle(&snapshot, inventory_id)?;
                let mut slot = snapshot.progression.directive_slot(inventory_id);
                changed = slot.auto_resupply != enabled;
                slot.auto_resupply = enabled;
                snapshot.progression.set_directive_slot(inventory_id, slot);
            }
            ProgressionCommand::Buy { item, count }
                if self.directives.items.contains_key(&item) =>
            {
                self.directives.buy(&mut snapshot, item, count)?;
            }
            ProgressionCommand::Buy { item, count }
                if self.consumables.items.contains_key(&item) =>
            {
                self.consumables.buy(&mut snapshot, item, count)?;
            }
            ProgressionCommand::Buy { item, count } => {
                let cd = items::IntCompactDescr::from_int(i64::from(item));
                let can_buy = match cd.item_type {
                    items::item_type::CREW_BOOK | items::item_type::CREW_SKIN => true,
                    items::item_type::CHASSIS..=items::item_type::RADIO => {
                        defaults.contains(&item) || snapshot.progression.unlocks.contains(&item)
                    }
                    _ => false,
                };
                if !can_buy || !(1..=100).contains(&count) || cd.to_int() != i64::from(item) {
                    return Err(ProgressionError::InvalidRequest);
                }
                let cost = self
                    .prices
                    .prices
                    .iter()
                    .find(|(id, _)| *id == cd)
                    .filter(|(id, p)| {
                        p.currency == "credits"
                            && p.amount >= 0
                            && !self.prices.not_in_shop.contains(id)
                    })
                    .and_then(|(_, p)| p.amount.checked_mul(i64::from(count)))
                    .ok_or(ProgressionError::NotForSale)?;
                if snapshot.balances.credits < cost {
                    return Err(ProgressionError::NotEnoughMoney);
                }
                let stock = snapshot
                    .progression
                    .depot
                    .get(&item)
                    .copied()
                    .unwrap_or(0)
                    .checked_add(count)
                    .ok_or(ProgressionError::InvalidRequest)?;
                snapshot.progression.depot.insert(item, stock);
                snapshot.balances.credits -= cost;
            }

            ProgressionCommand::Research { vehicle, index } => {
                if !defaults.contains(&vehicle) && !snapshot.progression.unlocks.contains(&vehicle)
                {
                    return Err(ProgressionError::VehicleNotFound);
                }
                let template = self.vehicle_by_id(vehicle).map_err(Self::catalog_failure)?;
                template
                    .modules
                    .rules
                    .research(&mut snapshot, index, &defaults)?;
            }
            ProgressionCommand::BuyVehicleSlot => {
                let credits = if snapshot.balances.slots < shop::FREE_VEHICLE_SLOTS {
                    0
                } else {
                    shop::VEHICLE_SLOT_CREDITS
                };
                snapshot.buy_vehicle_slot(VehicleCost { credits, gold: 0 })?;
            }
            ProgressionCommand::BuyVehicle {
                vehicle,
                options,
                consumables,
            } => {
                let offer = self
                    .offers
                    .get(&vehicle)
                    .ok_or(ProgressionError::VehicleNotFound)?;
                let researched =
                    defaults.contains(&vehicle) || snapshot.progression.unlocks.contains(&vehicle);
                if offer.requires_research && !researched {
                    return Err(ProgressionError::InvalidRequest);
                }
                if offer.price.is_none() {
                    return Err(ProgressionError::NotForSale);
                }
                let rules = self
                    .purchase_rules(vehicle)
                    .map_err(Self::catalog_failure)?;
                let mut roster = self.crew_roster(&snapshot).map_err(Self::catalog_failure)?;
                let inventory_id = rules.buy(&mut snapshot, options, researched)?;
                acquired_vehicle = Some(inventory_id);
                if !consumables.is_empty() {
                    let template = self.vehicle(&offer.key).map_err(Self::catalog_failure)?;
                    self.consumables.buy_for_vehicle(
                        &mut snapshot,
                        inventory_id,
                        &template.equipment,
                        template.supply_slots.consumables,
                        &consumables,
                    )?;
                }
                if options.crew.is_some() {
                    let spec = self.owned_crew_vehicle(&snapshot, inventory_id)?;
                    self.add_vehicle_crew(&mut roster, &spec)?;
                }
                snapshot.crew = Some(roster);
                snapshot.progression.unlocks.insert(vehicle);
            }

            ProgressionCommand::Equip {
                inventory_id,
                item,
                gun,
                buy,
            } => {
                let owned = snapshot
                    .vehicles
                    .iter()
                    .find(|v| v.inventory_id == inventory_id)
                    .ok_or(ProgressionError::VehicleNotFound)?;
                let template = self
                    .vehicle(&owned.catalog_key)
                    .map_err(Self::catalog_failure)?;
                let current = template.modules.rules.loadout(&snapshot, inventory_id);
                template.modules.rules.equip(
                    &mut snapshot,
                    inventory_id,
                    item,
                    gun,
                    buy,
                    &defaults,
                )?;
                let next = template.modules.rules.loadout(&snapshot, inventory_id);
                if current.get(&ModuleKind::Turret) != next.get(&ModuleKind::Turret)
                    || current.get(&ModuleKind::Gun) != next.get(&ModuleKind::Gun)
                {
                    let old_rules = template
                        .ammunition_rules(&current)
                        .ok_or(ProgressionError::InvalidRequest)?;
                    let new_rules = template
                        .ammunition_rules(&next)
                        .ok_or(ProgressionError::InvalidRequest)?;
                    let owned = snapshot
                        .vehicles
                        .iter_mut()
                        .find(|v| v.inventory_id == inventory_id)
                        .ok_or(ProgressionError::VehicleNotFound)?;
                    let shells = owned
                        .ammunition
                        .as_ref()
                        .map_or(&old_rules.defaults, |ammo| &ammo.loaded);
                    for &(id, count) in shells {
                        let stock = snapshot
                            .depot_shells
                            .get(&id)
                            .copied()
                            .unwrap_or(0)
                            .checked_add(count)
                            .ok_or(ProgressionError::InvalidRequest)?;
                        if stock > 0 {
                            snapshot.depot_shells.insert(id, stock);
                        }
                    }
                    owned.ammunition = Some(VehicleAmmunition {
                        loaded: new_rules.defaults.iter().map(|&(id, _)| (id, 0)).collect(),
                    });
                }
                if let Some(slots) = template.modules.exterior_slots(&next) {
                    snapshot.progression.cosmetics.retain_supported_slots(
                        inventory_id,
                        &slots.camouflage,
                        &slots.paints,
                        &slots.inscriptions,
                        slots.effect_slot,
                        &slots.projection_slots,
                    );
                }
            }
        }
        if changed {
            snapshot.revision = previous_revision
                .checked_add(1)
                .ok_or(ProgressionError::InvalidRequest)?;
        }
        Ok(ProgressionChange {
            acquired_vehicle,
            sold_vehicle,
            consumable_purchases,
            shell_purchases,
            hired_crew_member,
            snapshot,
            previous_revision,
        })
    }

    fn directive_vehicle(
        &self,
        snapshot: &AccountSnapshot,
        inventory_id: i64,
    ) -> Result<std::sync::Arc<super::CatalogVehicle>, ProgressionError> {
        let vehicle = self.owned_vehicle_template(snapshot, inventory_id)?;
        if vehicle.supply_slots.directives != 1 {
            return Err(ProgressionError::InvalidDirective);
        }
        Ok(vehicle)
    }

    pub(super) fn owned_vehicle_template(
        &self,
        snapshot: &AccountSnapshot,
        inventory_id: i64,
    ) -> Result<std::sync::Arc<super::CatalogVehicle>, ProgressionError> {
        let owned = snapshot
            .vehicles
            .iter()
            .find(|owned| owned.inventory_id == inventory_id)
            .ok_or(ProgressionError::VehicleNotFound)?;
        self.vehicle(&owned.catalog_key)
            .map_err(Self::catalog_failure)
    }

    #[tracing::instrument(name = "accounts.progression.progression_diff", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = change.snapshot.id, input.previous_revision = change.previous_revision), err)]
    pub fn progression_diff(
        &self,
        previous: &AccountSnapshot,
        change: &ProgressionChange,
    ) -> anyhow::Result<Value> {
        let next = &change.snapshot;
        let mut inventory = BTreeMap::new();
        for kind in (items::item_type::CHASSIS..=items::item_type::EQUIPMENT)
            .chain([items::item_type::CREW_BOOK])
        {
            if kind == items::item_type::TANKMAN {
                continue;
            }
            let (old, new) = if kind == items::item_type::SHELL {
                (&previous.depot_shells, &next.depot_shells)
            } else {
                (&previous.progression.depot, &next.progression.depot)
            };
            let mut diff = BTreeMap::new();
            for id in old.keys().chain(new.keys()) {
                if items::IntCompactDescr::from_int(i64::from(*id)).item_type != kind {
                    continue;
                }
                if old.get(id) != new.get(id) {
                    diff.insert(
                        Value::from(*id),
                        new.get(id).map_or(Value::None, |&count| Value::from(count)),
                    );
                }
            }
            if !diff.is_empty() {
                inventory.insert(Value::from(i32::from(kind)), Value::Dict(diff));
            }
        }
        let mut descriptors = BTreeMap::new();
        let mut shells = BTreeMap::new();
        let mut layouts = BTreeMap::new();
        let mut equipment_layouts = BTreeMap::new();
        let mut consumables = BTreeMap::new();
        let mut consumable_layouts = BTreeMap::new();
        let mut boosters = BTreeMap::new();
        let mut booster_layouts = BTreeMap::new();
        let mut settings = BTreeMap::new();
        let crew = self.crew_roster(next)?;
        let previous_vehicles: BTreeMap<_, _> = previous
            .vehicles
            .iter()
            .map(|vehicle| (vehicle.inventory_id, vehicle))
            .collect();
        for owned in &next.vehicles {
            if change.acquired_vehicle == Some(owned.inventory_id) {
                continue;
            }
            let modules_changed = previous.progression.installed.get(&owned.inventory_id)
                != next.progression.installed.get(&owned.inventory_id);
            let equipment_changed = previous.progression.equipment_layout(owned.inventory_id)
                != next.progression.equipment_layout(owned.inventory_id);
            let old_directive = previous.progression.directive_slot(owned.inventory_id);
            let new_directive = next.progression.directive_slot(owned.inventory_id);
            let directive_changed = old_directive.installed != new_directive.installed;
            let old_consumables = previous.progression.vehicle_consumables(owned.inventory_id);
            let new_consumables = next.progression.vehicle_consumables(owned.inventory_id);
            let consumables_changed = old_consumables.layout != new_consumables.layout;
            let ammunition_changed = modules_changed
                || previous_vehicles
                    .get(&owned.inventory_id)
                    .is_some_and(|vehicle| vehicle.ammunition != owned.ammunition);
            if old_directive.auto_resupply != new_directive.auto_resupply
                || old_consumables.auto_resupply != new_consumables.auto_resupply
            {
                settings.insert(
                    Value::from(owned.inventory_id),
                    Value::from(super::account::vehicle_settings(
                        &crew,
                        &next.progression,
                        owned.inventory_id,
                    )),
                );
            }
            if !ammunition_changed
                && !equipment_changed
                && !directive_changed
                && !consumables_changed
            {
                continue;
            }
            let vehicle = self.vehicle(&owned.catalog_key)?.project(owned, next)?;
            let id = Value::from(owned.inventory_id);
            if modules_changed || equipment_changed {
                descriptors.insert(id.clone(), Value::Bytes(vehicle.descr.encode()));
            }
            let replacement = Value::Tuple(vec![id.clone(), Value::from("_r")]);
            if ammunition_changed {
                shells.insert(id.clone(), vehicle.shells_value());
                layouts.insert(replacement.clone(), vehicle.shells_layout_value());
            }
            if equipment_changed {
                equipment_layouts.insert(replacement.clone(), vehicle.equipment_layout_value());
            }
            if consumables_changed {
                consumables.insert(id.clone(), vehicle.consumables_value());
                consumable_layouts.insert(replacement.clone(), vehicle.consumables_layout_value());
            }
            if directive_changed {
                boosters.insert(id, vehicle.directives_value());
                booster_layouts.insert(replacement, vehicle.directives_layout_value());
            }
        }
        let vehicle_tables: BTreeMap<_, _> = [
            ("compDescr", descriptors),
            ("shells", shells),
            ("shellsLayout", layouts),
            ("devicesLayout", equipment_layouts),
            ("eqs", consumables),
            ("eqsLayout", consumable_layouts),
            ("boosters", boosters),
            ("boostersLayout", booster_layouts),
            ("settings", settings),
        ]
        .into_iter()
        .filter(|(_, rows)| !rows.is_empty())
        .map(|(name, rows)| (Value::from(name), Value::Dict(rows)))
        .collect();
        if !vehicle_tables.is_empty() {
            inventory.insert(Value::from(1), Value::Dict(vehicle_tables));
        }
        let mut recycle_bin = Value::Dict(BTreeMap::new());
        if change.acquired_vehicle.is_some()
            || change.sold_vehicle.is_some()
            || previous.crew != next.crew
        {
            inventory.extend(self.crew_inventory_diff(previous, next)?);
        }
        if previous.crew != next.crew {
            let previous = super::crew::recycle_rows(&self.crew_roster(previous)?);
            let next = super::crew::recycle_rows(&self.crew_roster(next)?);
            if previous != next {
                recycle_bin = crate::pickle!({"tankmen": {"buffer": (Value::Dict(changed_rows(&previous, &next)))}});
            }
        }
        if previous.crew != next.crew || previous.progression.depot != next.progression.depot {
            let old = super::crew_skins::inventory(
                &self.crew_roster(previous)?,
                &previous.progression.depot,
            );
            let new =
                super::crew_skins::inventory(&self.crew_roster(next)?, &next.progression.depot);
            let tables = changed_tables(Some(&old), &new);
            if !tables.is_empty() {
                inventory.insert(
                    Value::from(i32::from(items::item_type::CREW_SKIN)),
                    Value::Dict(tables),
                );
            }
        }
        if previous.progression.cosmetics != next.progression.cosmetics {
            let project = |snapshot: &AccountSnapshot| {
                let vehicle_types = snapshot
                    .vehicles
                    .iter()
                    .filter_map(|vehicle| {
                        Some((vehicle.inventory_id, *self.keys.get(&vehicle.catalog_key)?))
                    })
                    .collect();
                super::cosmetic_inventory::CosmeticInventory::project(
                    &snapshot.progression.cosmetics,
                    &vehicle_types,
                )
            };
            let changes = project(next).diff(&project(previous));
            if !changes.is_empty() {
                inventory.insert(
                    Value::from(i32::from(items::item_type::CUSTOMIZATION)),
                    Value::Dict(changes),
                );
            }
        }
        let remaining_crew_capacity = crew.remaining_member_capacity();
        let mut unlocks = self.default_unlocks(next)?;
        let (elite_key, elite) = if change.sold_vehicle.is_some() {
            (
                Value::Tuple(vec![Value::from("eliteVehicles"), Value::from("_r")]),
                self.elite_vehicles(next, &unlocks)?
                    .into_iter()
                    .map(Value::from)
                    .collect(),
            )
        } else if change.acquired_vehicle.is_some()
            || previous.progression.unlocks != next.progression.unlocks
        {
            let previous_elite = self.elite_vehicles(previous, &self.default_unlocks(previous)?)?;
            let added = self
                .elite_vehicles(next, &unlocks)?
                .difference(&previous_elite)
                .copied()
                .map(Value::from)
                .collect();
            (Value::from("eliteVehicles"), added)
        } else {
            (Value::from("eliteVehicles"), BTreeSet::new())
        };
        unlocks.extend(&next.progression.unlocks);
        Ok(
            crate::pickle!({"prevRev":(change.previous_revision),"rev":(next.revision),"inventory":(Value::Dict(inventory)),"recycleBin":(recycle_bin),"stats":{
                "berths":(next.balances.berths),"slots":(next.balances.slots),
                "freeTMenLeft":(remaining_crew_capacity),
                "gold":(next.balances.gold),"crystal":(next.balances.bonds),"credits":(next.balances.credits),"freeXP":(next.balances.free_xp),
                "equipCoin":(next.balances.equipment_components),
                "unlocks":(Value::Set(unlocks.into_iter().map(Value::from).collect())),
                (elite_key):(Value::Set(elite)),
                "vehTypeXP":(Value::Dict(next.progression.vehicle_xp.iter().map(|(&id,&xp)|(Value::from(id),Value::from(xp))).collect())),
            }}),
        )
    }
    fn crew_inventory_diff(
        &self,
        previous: &AccountSnapshot,
        next: &AccountSnapshot,
    ) -> anyhow::Result<BTreeMap<Value, Value>> {
        let Value::Dict(old) = self.project_account(previous)?.inventory() else {
            anyhow::bail!("Invalid previous inventory");
        };
        let Value::Dict(new) = self.project_account(next)?.inventory() else {
            anyhow::bail!("Invalid next inventory");
        };
        let mut result = BTreeMap::new();
        for kind in [1, 8] {
            let key = Value::from(kind);
            if let Some(tables) = new.get(&key) {
                let changes = changed_tables(old.get(&key), tables);
                if !changes.is_empty() {
                    result.insert(key, Value::Dict(changes));
                }
            }
        }
        Ok(result)
    }

    pub fn progression_response(
        &self,
        previous: &AccountSnapshot,
        change: &ProgressionChange,
        command: &ProgressionCommand,
    ) -> Value {
        if let Some(id) = change.hired_crew_member
            && let Some(member) = change
                .snapshot
                .crew
                .as_ref()
                .and_then(|crew| crew.members.get(&id))
        {
            return crate::pickle!({
                "tmanInvID": (id),
                "tmanCompDescr": (Value::Bytes(super::crew::crew_descriptor(member))),
            });
        }
        match command {
            ProgressionCommand::FillConsumables { .. } => {
                let purchases = change
                    .consumable_purchases
                    .iter()
                    .map(|purchase| {
                        crate::pickle!((
                            (purchase.item),
                            ((purchase.cost.credits), (purchase.cost.gold)),
                            1
                        ))
                    })
                    .collect();
                crate::pickle!({
                    "eqs": (Value::List(purchases)),
                    "shells": (super::account::shell_purchases_value(&change.shell_purchases)),
                })
            }
            ProgressionCommand::Crew(crate::accounts::CrewCommand::Train { free_xp, .. }) => {
                crate::pickle!([("freeXP", (*free_xp))])
            }
            ProgressionCommand::Crew(crate::accounts::CrewCommand::UseBook {
                book, count, ..
            }) => crate::pickle!([((*book), (*count))]),
            ProgressionCommand::Crew(crate::accounts::CrewCommand::ConvertUndertrained) => {
                let old = self
                    .crew_roster(previous)
                    .map_or(0, |crew| crew.members.len());
                let new = change
                    .snapshot
                    .crew
                    .as_ref()
                    .map_or(0, |crew| crew.members.len());
                crate::pickle!({"count": (old.saturating_sub(new) as i64)})
            }
            _ => crate::pickle!({}),
        }
    }
}

fn changed_tables(old: Option<&Value>, new: &Value) -> BTreeMap<Value, Value> {
    let Value::Dict(tables) = new else {
        return BTreeMap::new();
    };
    let empty = BTreeMap::new();
    tables
        .iter()
        .filter_map(|(table, rows)| {
            let Value::Dict(rows) = rows else { return None };
            let previous = match old {
                Some(Value::Dict(tables)) => match tables.get(table) {
                    Some(Value::Dict(rows)) => rows,
                    _ => &empty,
                },
                _ => &empty,
            };
            let changes = changed_rows(previous, rows);
            (!changes.is_empty()).then(|| (table.clone(), Value::Dict(changes)))
        })
        .collect()
}

pub(super) fn changed_rows(
    old: &BTreeMap<Value, Value>,
    new: &BTreeMap<Value, Value>,
) -> BTreeMap<Value, Value> {
    new.iter()
        .filter(|(id, value)| old.get(*id) != Some(*value))
        .map(|(id, value)| (id.clone(), value.clone()))
        .chain(
            old.keys()
                .filter(|id| !new.contains_key(*id))
                .map(|id| (id.clone(), Value::None)),
        )
        .collect()
}
