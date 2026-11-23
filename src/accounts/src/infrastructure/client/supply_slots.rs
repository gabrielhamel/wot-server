use super::item_defs::read_xml;
use anyhow::{Context, Result};
use std::collections::BTreeSet;
use wot_platform::client_formats::pxml::Element;
use wot_resources::ResourceBundle;

#[derive(Default)]
pub(super) struct SupplySlots {
    optional_devices: BTreeSet<i64>,
    consumables: BTreeSet<i64>,
    directives: BTreeSet<i64>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct VehicleSupplySlots {
    pub optional_devices: usize,
    pub consumables: usize,
    pub directives: usize,
}

impl SupplySlots {
    pub fn load(fs: &ResourceBundle) -> Result<Self> {
        let data = read_xml(
            fs,
            "scripts/item_defs/vehicles/common/supply_slot_types.xml",
        )?;
        let slots = data
            .get_child_element("slots")
            .context("Missing supply slots")?;
        let mut result = Self::default();
        for (_, slot) in slots.iter_child_elements() {
            let Some(id) = slot.get_child_integer("id") else {
                continue;
            };
            if slot.get_child_string("type") == Some("optionalDevice")
                || slot.get_child_string("slotClass") == Some("OptionalDeviceSlot")
            {
                result.optional_devices.insert(id);
            } else if let Some(meta) = slot.get_child_element("meta") {
                match meta.get_child_string("equipmentType") {
                    Some("regular") => {
                        result.consumables.insert(id);
                    }
                    Some("battleBoosters") => {
                        result.directives.insert(id);
                    }
                    _ => {}
                }
            }
        }
        Ok(result)
    }

    pub fn vehicle(&self, vehicle: &Element) -> Result<VehicleSupplySlots> {
        let mut slots = VehicleSupplySlots::default();
        for id in vehicle
            .get_child_string("supplySlots")
            .unwrap_or_default()
            .split_whitespace()
        {
            let id = id.parse()?;
            slots.optional_devices += usize::from(self.optional_devices.contains(&id));
            slots.consumables += usize::from(self.consumables.contains(&id));
            slots.directives += usize::from(self.directives.contains(&id));
        }
        Ok(slots)
    }
}
