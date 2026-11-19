mod account;
mod cosmetic_catalog;
mod cosmetic_inventory;
mod outfit;
pub use cosmetic_catalog::cosmetic_item;
pub use outfit::decode_outfits;
mod consumables;
mod crew;
mod crew_books;
mod crew_conversion;
mod crew_skins;
mod depot;
mod directives;
mod equipment;
mod grants;
mod item_defs;
mod item_prices;
mod items;
mod localization;
mod module_defs;
mod progression;
mod shop;
mod starter_garage;
mod store;
pub use store::{
    StoreAvailability, StoreCatalog, StoreCategory, StoreListing, StoreOffer, StoreOrder,
    StoreOrderError, StorePrice,
};
mod supply_slots;
mod vehicle_defs;
pub use grants::{CosmeticGrantEntry, VehicleCatalogEntry};
mod vehicle_sales;

use crate::accounts::{
    AccountSnapshot, AmmoError, AmmunitionChange, AmmunitionRules, ShellCurrency, ShellPrice,
    ShellSelection,
    contract::{AccountView, PreparedAccount},
};
use crate::platform::client_formats::pickle::Value;
use crate::platform::client_formats::pxml;
use anyhow::{Context, Result};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::{Arc, RwLock},
};
use wot_resources::ResourceBundle;

pub struct GarageCatalog {
    templates: RwLock<HashMap<String, Arc<CatalogVehicle>>>,
    offers: BTreeMap<i32, vehicle_defs::VehicleOffer>,
    keys: HashMap<String, i32>,
    tech_tree: BTreeSet<i32>,
    filesystem: ResourceBundle,
    passports: Vec<crew::Passports>,
    crew_books: BTreeMap<i32, crew_books::CrewBook>,
    crew_skins: BTreeMap<u16, crew_skins::CrewSkin>,
    prices: item_prices::ItemPrices,
    cosmetics: cosmetic_catalog::CosmeticCatalog,
    shell_prices: BTreeMap<i32, ShellPrice>,
    equipment: crate::accounts::EquipmentRules,
    equipment_grants: equipment::EquipmentGrantCatalog,
    store_offers: Vec<StoreOffer>,
    directives: crate::accounts::DirectiveRules,
    consumables: crate::accounts::ConsumableRules,
    supply_slots: supply_slots::SupplySlots,
    observer_descriptor: Option<Vec<u8>>,
    elite_progression_trees: BTreeSet<String>,
    pub(in crate::accounts) required_version: String,
    pub(in crate::accounts) shop: Arc<Vec<u8>>,
}

struct CatalogVehicle {
    template: account::Vehicle,
    crew_roles: Vec<Vec<u8>>,
    modules: module_defs::VehicleModules,
    equipment: crate::accounts::EquipmentVehicle,
    supply_slots: supply_slots::VehicleSupplySlots,
    elite_by_progression: bool,
    customization_nation: String,
    default_emblem: Option<u16>,
}

impl CatalogVehicle {
    fn is_elite(&self, defaults: &BTreeSet<i32>, unlocks: &BTreeSet<i32>) -> bool {
        !self.elite_by_progression
            && self
                .modules
                .rules
                .research
                .iter()
                .all(|step| defaults.contains(&step.item) || unlocks.contains(&step.item))
    }

    fn project(
        &self,
        owned: &crate::accounts::OwnedVehicle,
        snapshot: &AccountSnapshot,
    ) -> Result<account::Vehicle> {
        let mut vehicle = self.template.clone();
        let installed = self.modules.rules.loadout(snapshot, owned.inventory_id);
        self.modules.project(&mut vehicle.descr, &installed)?;
        vehicle.directive = snapshot
            .progression
            .directive_slot(owned.inventory_id)
            .installed;
        vehicle.consumables = snapshot
            .progression
            .vehicle_consumables(owned.inventory_id)
            .layout;
        vehicle.equipment = snapshot.progression.equipment_layout(owned.inventory_id);
        vehicle.descr.optional_devices = vehicle
            .equipment
            .get(..self.equipment.slots)
            .context("Invalid equipment slot count")?
            .iter()
            .rev()
            .map(|&item| (item != 0).then(|| items::IntCompactDescr::from_int(i64::from(item)).id))
            .collect();
        let rules = self
            .ammunition_rules(&installed)
            .context("Missing gun ammunition rules")?;
        vehicle.shells = owned
            .ammunition
            .as_ref()
            .map_or_else(|| rules.defaults.clone(), |ammo| ammo.loaded.clone());
        Ok(vehicle)
    }

    fn ammunition_rules(
        &self,
        installed: &crate::accounts::ModuleLoadout,
    ) -> Option<&AmmunitionRules> {
        self.modules.ammunition.get(&(
            *installed.get(&crate::accounts::ModuleKind::Turret)?,
            *installed.get(&crate::accounts::ModuleKind::Gun)?,
        ))
    }
}

impl GarageCatalog {
    #[tracing::instrument(name = "accounts.training_observer_descriptor", skip_all, fields(ddd.bounded_context.name = "accounts", output.bytes = self.observer_descriptor.as_ref().map(Vec::len)))]
    pub fn training_observer_descriptor(&self) -> Option<&[u8]> {
        self.observer_descriptor.as_deref()
    }

    #[tracing::instrument(name = "accounts.training_vehicle_descriptor", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = snapshot.id, input.inventory_id = inventory_id), err)]
    pub fn training_vehicle_descriptor(
        &self,
        snapshot: &AccountSnapshot,
        inventory_id: i64,
    ) -> Result<Option<Vec<u8>>> {
        let Some(owned) = snapshot
            .vehicles
            .iter()
            .find(|vehicle| vehicle.inventory_id == inventory_id)
        else {
            return Ok(None);
        };
        let vehicle = self.vehicle(&owned.catalog_key)?.project(owned, snapshot)?;
        Ok(Some(vehicle.descr.encode()))
    }

    pub fn load(fs: ResourceBundle, keys: &[String]) -> Result<Self> {
        let def = item_defs::read_xml(&fs, "scripts/entity_defs/interfaces/AccountVersion.def")?;
        let required_version = def
            .get_child_value_at(&["Properties", "requiredVersion_2410", "Default"])
            .and_then(pxml::Value::as_string)
            .context("Unsupported resources: missing requiredVersion_2410")?
            .to_owned();
        let passports = (0..item_defs::NATIONS.len())
            .map(|nation| crew::Passports::load(&fs, nation as u8))
            .collect::<std::io::Result<Vec<_>>>()?;
        let mut prices = item_prices::ItemPrices::load(&fs)?;
        let cosmetics = cosmetic_catalog::CosmeticCatalog::load(&fs, &mut prices)?;
        cosmetic_catalog::index_exterior_shop(&fs, &mut prices)?;
        let crew_books = crew_books::load(&fs, &mut prices)?;
        let crew_skins = crew_skins::load(&fs, &mut prices)?;
        let (equipment, equipment_grants) = equipment::load(&fs, &prices)?;
        let directives = directives::load(&fs, &prices)?;
        let consumables = consumables::load(&fs, &prices)?;
        let supply_slots = supply_slots::SupplySlots::load(&fs)?;
        // The client reserves tree IDs starting at 10000 for vehicle skill trees.
        let progression_trees = item_defs::read_xml(
            &fs,
            "scripts/item_defs/vehicles/common/post_progression/trees.xml",
        )?;
        let elite_progression_trees = progression_trees
            .iter_children_all()
            .filter(|(_, value)| {
                value
                    .as_element()
                    .and_then(|tree| tree.get_child_integer("id"))
                    .is_some_and(|id| id >= 10_000)
            })
            .map(|(name, _)| name.clone())
            .collect();
        let shop = Arc::new(shop::Shop::new(&prices, &equipment)?.data);
        let shell_prices: BTreeMap<_, _> = prices
            .prices
            .iter()
            .filter_map(|(id, price)| {
                if id.item_type != items::item_type::SHELL || prices.not_in_shop.contains(id) {
                    return None;
                }
                let currency = match price.currency {
                    "credits" => ShellCurrency::Credits,
                    "gold" => ShellCurrency::Gold,
                    _ => return None,
                };
                Some((
                    id.to_int() as i32,
                    ShellPrice {
                        currency,
                        amount: price.amount,
                    },
                ))
            })
            .collect();
        let offers = vehicle_defs::offers(&fs, &prices)?;
        let catalog_keys = offers
            .iter()
            .map(|(&id, offer)| (offer.key.clone(), id))
            .collect();
        let observer_descriptor = offers
            .values()
            .find(|offer| offer.key == "ussr:Observer" && offer.tags.contains("observer"))
            .and_then(|offer| {
                match item_defs::VehicleDef::load(&fs, "ussr", "Observer", offer, &supply_slots) {
                    Ok(vehicle) => Some(vehicle.descr.encode()),
                    Err(error) => {
                        tracing::warn!(%error, "Training Room observer vehicle is unavailable");
                        None
                    }
                }
            });
        let mut store_offers =
            store::load(&fs, &prices, &offers, &equipment, &consumables, &directives)?;
        store_offers.extend_from_slice(cosmetics.store_styles());
        let tech_tree = starter_garage::tech_tree(&fs, &catalog_keys, &offers)?;
        let catalog = Self {
            templates: RwLock::new(HashMap::new()),
            offers,
            keys: catalog_keys,
            tech_tree,
            filesystem: fs,
            passports,
            crew_books,
            crew_skins,
            prices,
            cosmetics,
            shell_prices,
            equipment,
            equipment_grants,
            store_offers,
            directives,
            consumables,
            supply_slots,
            observer_descriptor,
            elite_progression_trees,
            required_version,
            shop,
        };
        for key in keys {
            catalog.vehicle(key)?;
        }
        Ok(catalog)
    }

    fn vehicle(&self, key: &str) -> Result<Arc<CatalogVehicle>> {
        let offer = self
            .keys
            .get(key)
            .and_then(|id| self.offers.get(id))
            .with_context(|| format!("Unknown garage vehicle {key}"))?;
        if let Some(vehicle) = self
            .templates
            .read()
            .map_err(|_| anyhow::anyhow!("Vehicle catalog lock poisoned"))?
            .get(key)
        {
            return Ok(vehicle.clone());
        }
        let (nation, name) = key
            .split_once(':')
            .context("Vehicle key must be nation:name")?;
        let def =
            item_defs::VehicleDef::load(&self.filesystem, nation, name, offer, &self.supply_slots)
                .with_context(|| format!("Load garage vehicle {key}"))?;
        let modules = module_defs::VehicleModules::load(
            &self.filesystem,
            nation,
            name,
            &def,
            &self.prices,
            &self.shell_prices,
        )
        .with_context(|| format!("Load modules for {key}"))?;
        let equipment = def.equipment.clone();
        let shells = def
            .default_ammo()
            .into_iter()
            .map(|(id, count)| (id.to_int() as i32, count))
            .collect();
        let vehicle = Arc::new(CatalogVehicle {
            template: account::Vehicle {
                descr: def.descr,
                crew_slots: def.crew_roles.len(),
                equipment: Default::default(),
                directive: None,
                consumables: Default::default(),
                shells,
            },
            crew_roles: def.crew_roles,
            customization_nation: def.customization_nation,
            default_emblem: def.default_emblem,
            supply_slots: def.supply_slots,
            elite_by_progression: def
                .post_progression_tree
                .as_ref()
                .is_some_and(|tree| self.elite_progression_trees.contains(tree)),
            modules,
            equipment,
        });
        let mut cache = self
            .templates
            .write()
            .map_err(|_| anyhow::anyhow!("Vehicle catalog lock poisoned"))?;
        Ok(cache.entry(key.to_owned()).or_insert(vehicle).clone())
    }

    pub fn equipment_upgrades_enabled(&self) -> bool {
        self.equipment
            .items
            .values()
            .any(|device| device.upgrade.is_some())
    }

    pub fn shop_revision(&self) -> i32 {
        shop::SHOP_REVISION
    }

    #[tracing::instrument(name = "accounts.plan_ammunition", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = snapshot.id, input.revision = snapshot.revision, input.inventory_id = inventory_id), err)]
    pub fn plan_ammunition(
        &self,
        snapshot: AccountSnapshot,
        inventory_id: i64,
        requested: &[ShellSelection],
    ) -> Result<AmmunitionChange, AmmoError> {
        let owned = snapshot
            .vehicles
            .iter()
            .find(|vehicle| vehicle.inventory_id == inventory_id)
            .ok_or(AmmoError::VehicleNotFound)?;
        let template = self.vehicle(&owned.catalog_key).map_err(|error| {
            tracing::error!(%error,"Load ammunition catalog failed");
            AmmoError::VehicleNotFound
        })?;
        let installed = template.modules.rules.loadout(&snapshot, inventory_id);
        let rules = template
            .ammunition_rules(&installed)
            .ok_or(AmmoError::InvalidLayout)?;
        AmmunitionChange::plan(snapshot, inventory_id, rules, requested)
    }

    pub fn ammunition_response(&self, change: &AmmunitionChange) -> Value {
        crate::pickle!({"shells": (account::shell_purchases_value(&change.purchases))})
    }

    #[tracing::instrument(name = "accounts.ammunition_diff", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = change.snapshot.id, input.previous_revision = change.previous_revision), err)]
    pub fn ammunition_diff(
        &self,
        previous: &AccountSnapshot,
        change: &AmmunitionChange,
    ) -> Result<Value> {
        let next = &change.snapshot;
        let owned = next
            .vehicles
            .iter()
            .find(|vehicle| vehicle.inventory_id == change.inventory_id)
            .context("Missing owned vehicle in ammunition diff")?;
        let vehicle = self.vehicle(&owned.catalog_key)?.project(owned, next)?;
        let depot = next
            .depot_shells
            .iter()
            .filter(|(id, count)| previous.depot_shells.get(id) != Some(*count))
            .map(|(&id, &count)| (Value::from(id), Value::from(count)))
            .chain(
                previous
                    .depot_shells
                    .keys()
                    .filter(|id| !next.depot_shells.contains_key(id))
                    .map(|&id| (Value::from(id), Value::None)),
            )
            .collect();
        let id = Value::from(owned.inventory_id);
        let replacement = Value::Tuple(vec![id.clone(), Value::from("_r")]);
        Ok(crate::pickle!({
            "prevRev": (change.previous_revision),
            "rev": (next.revision),
            "inventory": {
                (i32::from(items::item_type::VEHICLE)): {
                    "shells": { (id): (vehicle.shells_value()) },
                    "shellsLayout": { (replacement): (vehicle.shells_layout_value()) },
                },
                (i32::from(items::item_type::SHELL)): (Value::Dict(depot)),
            },
            "stats": {"credits": (next.balances.credits), "gold": (next.balances.gold)},
        }))
    }

    #[tracing::instrument(name = "accounts.project_garage", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = snapshot.id, input.revision = snapshot.revision, input.vehicle_count = snapshot.vehicles.len()), err)]
    fn project_account(&self, snapshot: &AccountSnapshot) -> Result<account::AccountData> {
        let default_unlocks = self.default_unlocks(snapshot)?;
        let vehicles = snapshot
            .vehicles
            .iter()
            .map(|owned| self.vehicle(&owned.catalog_key)?.project(owned, snapshot))
            .collect::<Result<Vec<_>>>()?;
        Ok(account::AccountData {
            vehicles,
            ids: snapshot.vehicles.iter().map(|v| v.inventory_id).collect(),
            balances: snapshot.balances.clone(),
            crew: self.crew_roster(snapshot)?,
            settings: snapshot.settings.clone(),
            depot_shells: snapshot.depot_shells.clone(),
            progression: snapshot.progression.clone(),
            elite_vehicles: self.elite_vehicles(snapshot, &default_unlocks)?,
            default_unlocks,
            premium_expires_at: snapshot.active_premium_until(),
            max_researched_tiers: self.max_researched_tiers(snapshot),
        })
    }

    #[tracing::instrument(name = "accounts.prepare", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = snapshot.id, input.revision = snapshot.revision), err)]
    pub fn prepare(&self, snapshot: AccountSnapshot) -> Result<PreparedAccount> {
        let data = self.project_account(&snapshot)?;
        let cache = self.encode_garage(snapshot.revision, &data)?;
        Ok(PreparedAccount {
            snapshot: AccountView(snapshot),
            cache: Arc::new(cache),
        })
    }

    #[tracing::instrument(name = "accounts.encode_garage", skip_all, fields(ddd.bounded_context.name = "accounts", input.revision = revision, output.bytes = tracing::field::Empty), err)]
    fn encode_garage(&self, revision: i64, data: &account::AccountData) -> Result<Vec<u8>> {
        let cache = account::compress(&data.data(revision, self.tech_tree_economics()))?;
        tracing::Span::current().record("output.bytes", cache.len());
        Ok(cache)
    }
}
