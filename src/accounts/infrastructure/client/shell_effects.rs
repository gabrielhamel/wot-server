use super::{item_defs::read_xml, item_prices::ItemPrices};
use crate::{platform::client_formats::pxml::Element, shared_kernel::ShellEffects};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;
use wot_resources::ResourceBundle;

const LEGACY: &str = "scripts/item_defs/vehicles/common/shot_effects.xml";
const PREFABS: &str = "scripts/item_defs/vehicles/common/prefab_effects/shot_effects.xml";
const VEHICLE_COMMON: &str = "scripts/item_defs/vehicles/common/vehicle.xml";
const NO_PREFAB: u8 = 255;
const SHELL_KINDS: [&str; 6] = [
    "HOLLOW_CHARGE",
    "HIGH_EXPLOSIVE",
    "ARMOR_PIERCING",
    "ARMOR_PIERCING_HE",
    "ARMOR_PIERCING_CR",
    "SMOKE",
];

#[derive(Default)]
pub(super) struct ShellEffectCatalog {
    legacy: BTreeMap<String, u8>,
    prefabs: BTreeMap<String, u8>,
    defaults: BTreeMap<String, String>,
}

impl ShellEffectCatalog {
    pub(super) fn load(resources: &ResourceBundle) -> Result<Self> {
        if [LEGACY, PREFABS, VEHICLE_COMMON]
            .iter()
            .all(|path| !resources.has_file(path))
        {
            return Ok(Self::default());
        }
        let legacy = effect_indices(read_xml(resources, LEGACY).context(LEGACY)?.as_ref())?;
        let prefabs = effect_indices(read_xml(resources, PREFABS).context(PREFABS)?.as_ref())?;
        let common = read_xml(resources, VEHICLE_COMMON).context(VEHICLE_COMMON)?;
        let defaults = common
            .get_child_element("defaultPrefabEffects")
            .and_then(|defaults| defaults.get_child_element("shot"))
            .context("Missing default shell prefab effects")?
            .iter_children_all()
            .filter_map(|(name, value)| Some((name.clone(), value.as_string()?.trim().to_owned())))
            .collect();
        Ok(Self {
            legacy,
            prefabs,
            defaults,
        })
    }

    pub(super) fn resolve(&self, shot: Option<&Element>, shell: &Element) -> Option<ShellEffects> {
        let kind = shell.get_child_string("kind")?.trim();
        let caliber_mm = shell
            .get_child_float("effectsCaliber")
            .or_else(|| shell.get_child_float("caliber"))?;
        if !caliber_mm.is_finite() || !(0.0..=6553.5).contains(&caliber_mm) || caliber_mm == 0.0 {
            return None;
        }
        let effect = shot
            .and_then(|shot| shot.get_child_string("effects"))
            .or_else(|| shell.get_child_string("effects"))?;
        let prefab = shell.get_child_string("prefabEffects").or_else(|| {
            self.defaults
                .get(kind)
                .or_else(|| self.defaults.get("default"))
                .map(String::as_str)
        });
        Some(ShellEffects {
            kind: u8::try_from(SHELL_KINDS.iter().position(|&name| name == kind)?).ok()?,
            damage_log_type: damage_log_type(kind, shot, shell),
            premium: ItemPrices::definition_price(shell).currency == "gold"
                || shell.get_child_boolean("improved") == Some(true),
            caliber_mm,
            legacy_index: self.legacy.get(effect.trim()).copied().unwrap_or_default(),
            prefab_index: prefab
                .and_then(|name| self.prefabs.get(name.trim()).copied())
                .unwrap_or(NO_PREFAB),
        })
    }
}

fn damage_log_type(kind: &str, shot: Option<&Element>, shell: &Element) -> Option<u8> {
    Some(match kind {
        "HOLLOW_CHARGE" => 0,
        "ARMOR_PIERCING" => 1,
        "ARMOR_PIERCING_HE" => 2,
        "ARMOR_PIERCING_CR" => 3,
        "SMOKE" => 4,
        "HIGH_EXPLOSIVE"
            if shell.get_child_string("mechanics").map(str::trim) == Some("MODERN") =>
        {
            5
        }
        "HIGH_EXPLOSIVE"
            if shot
                .and_then(|shot| shot.get_child_boolean("hasStun"))
                .or_else(|| shell.get_child_boolean("hasStun"))
                == Some(true) =>
        {
            6
        }
        "HIGH_EXPLOSIVE" => 7,
        _ => return None,
    })
}

fn effect_indices(table: &Element) -> Result<BTreeMap<String, u8>> {
    table
        .iter_children_all()
        .filter(|(name, _)| name != "xmlns:xmlref")
        .enumerate()
        .map(|(index, (name, _))| {
            ensure!(
                index < usize::from(NO_PREFAB),
                "Too many shell effect entries"
            );
            Ok((name.clone(), u8::try_from(index)?))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::client_formats::pxml::Value;

    #[test]
    fn resolves_damage_log_types_independently_of_projectile_types() {
        let effects = ShellEffectCatalog::default();
        for (kind, projectile_type, log_type) in [
            ("HOLLOW_CHARGE", 0, 0),
            ("HIGH_EXPLOSIVE", 1, 7),
            ("ARMOR_PIERCING", 2, 1),
            ("ARMOR_PIERCING_HE", 3, 2),
            ("ARMOR_PIERCING_CR", 4, 3),
            ("SMOKE", 5, 4),
        ] {
            let mut shell = Element::new();
            shell.add_children("kind", Value::String(kind.into()));
            shell.add_children("caliber", Value::Integer(100));
            shell.add_children("effects", Value::String("example".into()));
            let resolved = effects.resolve(None, &shell).unwrap();
            assert_eq!(
                (resolved.kind, resolved.damage_log_type),
                (projectile_type, Some(log_type))
            );
            assert!(!resolved.premium);
        }
    }

    #[test]
    fn resolves_he_mechanics_and_uses_local_stun_override() {
        let mut shell = Element::new();
        shell.add_children("hasStun", Value::Boolean(true));
        let mut shot = Element::new();
        shot.add_children("hasStun", Value::Boolean(false));
        assert_eq!(damage_log_type("HIGH_EXPLOSIVE", None, &shell), Some(6));
        assert_eq!(
            damage_log_type("HIGH_EXPLOSIVE", Some(&shot), &shell),
            Some(7)
        );
        shell.add_children("mechanics", Value::String("MODERN".into()));
        assert_eq!(damage_log_type("HIGH_EXPLOSIVE", None, &shell), Some(5));
    }
}
