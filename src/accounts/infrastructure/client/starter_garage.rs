use super::GarageCatalog;
use crate::accounts::{AccountSnapshot, OwnedVehicle, VehicleAmmunition};
use anyhow::{Context, Result, ensure};

impl GarageCatalog {
    pub fn tier_one_keys(&self) -> Result<Vec<String>> {
        let mut keys = Vec::new();
        for nation in super::item_defs::NATIONS {
            let path = format!("gui/flash/techtree/{nation}-tree.xml");
            let tree = super::item_defs::read_xml(&self.filesystem, &path)?;
            let nodes = tree
                .get_child_element("nodes")
                .with_context(|| format!("Missing nodes in {path}"))?;
            for (name, _) in nodes.iter_child_elements() {
                let key = format!("{nation}:{name}");
                if self
                    .keys
                    .get(&key)
                    .and_then(|id| self.offers.get(id))
                    .is_some_and(|offer| {
                        offer.level == Some(1) && !offer.tags.contains("collectorVehicle")
                    })
                {
                    keys.push(key);
                }
            }
        }
        keys.sort_unstable();
        keys.dedup();
        ensure!(
            !keys.is_empty(),
            "Resources contain no tech tree tier I vehicles"
        );
        Ok(keys)
    }

    pub(in crate::accounts::infrastructure) fn prepare_starter_garage(
        &self,
        snapshot: &mut AccountSnapshot,
        keys: &[String],
    ) -> Result<()> {
        let mut roster = self.crew_roster(snapshot)?;
        for key in keys {
            let template = self.vehicle(key)?;
            let vehicle_id = template.modules.rules.vehicle;
            snapshot.progression.unlocks.insert(vehicle_id);
            snapshot
                .progression
                .unlocks
                .extend(template.modules.rules.stock.values());
            ensure!(
                snapshot.vehicles.len() < 1024,
                "Garage vehicle limit reached"
            );
            let inventory_id = snapshot.next_vehicle_id()?;
            let loadout = template.modules.rules.stock.clone();
            let ammunition = template
                .ammunition_rules(&loadout)
                .context("Missing gun ammunition")?;
            ensure!(
                ammunition.defaults.iter().any(|&(_, count)| count > 0),
                "Empty default ammunition"
            );
            snapshot.vehicles.push(OwnedVehicle {
                inventory_id,
                catalog_key: key.clone(),
                ammunition: Some(VehicleAmmunition {
                    loaded: ammunition.defaults.clone(),
                }),
                has_crew: true,
            });
            snapshot.progression.installed.insert(inventory_id, loadout);
            let mut vehicle = self.owned_crew_vehicle(snapshot, inventory_id)?;
            vehicle.crew_locked = false;
            for (slot, roles) in vehicle.roles.iter().enumerate() {
                let role = *roles.first().context("Missing crew role")?;
                let member = roster.insert(self.new_crew_member(
                    &vehicle,
                    role,
                    roster.next_passport_index(),
                )?)?;
                roster.assign(&vehicle, Some(slot), Some(member))?;
            }
        }
        snapshot.balances.slots = snapshot
            .balances
            .slots
            .max(i64::try_from(snapshot.vehicles.len())?);
        let defaults: std::collections::BTreeMap<i32, i32> =
            serde_json::from_str(include_str!("starter_settings.json"))?;
        for (key, value) in defaults {
            snapshot.settings.entry(key).or_insert(value);
        }
        snapshot.crew = Some(roster);
        self.prepare(snapshot.clone())?;
        Ok(())
    }
}
