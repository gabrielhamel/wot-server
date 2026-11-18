//! Garage-only resource loader adapted from wg-toolkit-wot; see THIRD_PARTY_NOTICES.md.
use super::items::{IntCompactDescr, VehicleDescr, item_type};
use crate::platform::client_formats::pxml::{self, Element};
use std::io;
pub use wot_resources::NATIONS;
use wot_resources::ResourceBundle;
pub const ROLES: &[(&str, u8)] = &[
    ("commander", 1),
    ("radioman", 2),
    ("driver", 3),
    ("gunner", 4),
    ("loader", 5),
];
#[derive(Debug, Clone)]
pub struct VehicleDef {
    pub descr: VehicleDescr,
    pub crew_roles: Vec<Vec<u8>>,
    pub shells: Vec<(IntCompactDescr, f32)>,
    pub max_ammo: u32,
    pub equipment: crate::accounts::EquipmentVehicle,
    pub supply_slots: super::supply_slots::VehicleSupplySlots,
    pub post_progression_tree: Option<String>,
    pub customization_nation: String,
    pub default_emblem: Option<u16>,
}
impl VehicleDef {
    pub(super) fn load(
        fs: &ResourceBundle,
        nation: &str,
        name: &str,
        offer: &super::vehicle_defs::VehicleOffer,
        slots: &super::supply_slots::SupplySlots,
    ) -> io::Result<Self> {
        let nation_id = NATIONS
            .iter()
            .position(|n| *n == nation)
            .ok_or_else(|| invalid(format!("unknown nation {nation}")))?
            as u8;

        let (dir, type_id) = vehicle_dirs(fs, nation)
            .into_iter()
            .find_map(|dir| {
                let id = read_xml(fs, &format!("{dir}/list.xml"))
                    .ok()?
                    .get_child_element(name)?
                    .get_child_integer("id")?;
                Some((dir, id))
            })
            .ok_or_else(|| invalid(format!("no vehicle {name} in {nation}")))?;

        let vehicle = read_xml(fs, &format!("{dir}/{name}.xml"))?;
        let supply_slots = slots
            .vehicle(&vehicle)
            .map_err(|error| invalid(error.to_string()))?;
        let equipment = super::equipment::vehicle(nation, offer, supply_slots.optional_devices)
            .map_err(|error| invalid(error.to_string()))?;
        let component_ids = |kind: &str| -> io::Result<Box<Element>> {
            read_xml(fs, &format!("{dir}/components/{kind}.xml"))
        };

        // The first module of a section, and its id from the components' ids.
        let first = |section: &Element, what: &str| -> io::Result<String> {
            section
                .iter_children_all()
                .next()
                .map(|(module, _)| module.clone())
                .ok_or_else(|| invalid(format!("{name} has no {what}")))
        };
        let module_id = |components: &Element, module: &str| -> io::Result<u16> {
            components
                .get_child_element("ids")
                .and_then(|ids| ids.get_child_integer(module))
                .map(|id| id as u16)
                .ok_or_else(|| invalid(format!("no id for {module}")))
        };
        let default_module = |section: &str, kind: &str| -> io::Result<u16> {
            let section = vehicle
                .get_child_element(section)
                .ok_or_else(|| invalid(format!("{name} has no {section}")))?;
            module_id(&*component_ids(kind)?, &first(section, kind)?)
        };

        let turrets = vehicle
            .get_child_element("turrets0")
            .ok_or_else(|| invalid(format!("{name} has no turret")))?;
        let turret_name = first(turrets, "turret")?;
        let turret_local = turrets.get_child_element(&turret_name);
        let turret_components = component_ids("turrets")?;
        let turret_shared = turret_components
            .get_child_element("shared")
            .and_then(|shared| shared.get_child_element(&turret_name));
        let guns = turret_local
            .and_then(|turret| turret.get_child_element("guns"))
            .or_else(|| turret_shared.and_then(|turret| turret.get_child_element("guns")))
            .ok_or_else(|| invalid(format!("{name} has no guns")))?;
        let gun_name = first(guns, "gun")?;

        let guns_components = component_ids("guns")?;
        let gun_local = guns.get_child_element(&gun_name);
        let gun_def = guns_components
            .get_child_element("shared")
            .and_then(|shared| shared.get_child_element(&gun_name))
            .or(gun_local)
            .ok_or_else(|| invalid(format!("no definition for gun {gun_name}")))?;
        // The vehicle may set its own capacity for a shared gun.
        let max_ammo = guns
            .get_child_element(&gun_name)
            .and_then(|gun| gun.get_child_integer("maxAmmo"))
            .or_else(|| gun_def.get_child_integer("maxAmmo"))
            .ok_or_else(|| invalid(format!("no ammunition capacity for {gun_name}")))?
            as u32;

        let shells_components = component_ids("shells")?;
        let mut shells = Vec::new();
        for (shell_name, shot) in gun_local
            .and_then(|gun| gun.get_child_element("shots"))
            .or_else(|| gun_def.get_child_element("shots"))
            .into_iter()
            .flat_map(Element::iter_children_all)
        {
            let id = shells_components
                .get_child_element(shell_name)
                .and_then(|shell| shell.get_child_integer("id"))
                .ok_or_else(|| invalid(format!("no shell id for {shell_name}")))?;
            let portion = shot
                .as_element()
                .and_then(|shot| shot.get_child_float("defaultPortion"))
                .unwrap_or(0.0);
            shells.push((
                IntCompactDescr {
                    item_type: item_type::SHELL,
                    nation: nation_id,
                    id: id as u16,
                },
                portion,
            ));
        }
        let crew_roles = vehicle
            .get_child_element("crew")
            .into_iter()
            .flat_map(|crew| crew.iter_children_all())
            .filter_map(|(role, value)| {
                let role_id = |role: &str| {
                    ROLES
                        .iter()
                        .find(|&&(name, _)| name == role)
                        .map(|&(_, id)| id)
                };
                let primary = role_id(role)?;
                let secondary = value
                    .as_element()
                    .and_then(|element| element.value.as_string())
                    .or_else(|| value.as_string())
                    .unwrap_or_default();
                Some(
                    std::iter::once(primary)
                        .chain(secondary.split_whitespace().filter_map(role_id))
                        .collect(),
                )
            })
            .collect();

        let descr = VehicleDescr {
            nation: nation_id,
            type_id: type_id as u16,
            chassis: default_module("chassis", "chassis")?,
            engine: default_module("engines", "engines")?,
            fuel_tank: default_module("fuelTanks", "fuelTanks")?,
            radio: default_module("radios", "radios")?,
            turret: module_id(&*component_ids("turrets")?, &turret_name)?,
            gun: module_id(&guns_components, &gun_name)?,
            optional_devices: Vec::new(),
            extra_flags: 0,
            extra: Vec::new(),
        };

        Ok(Self {
            descr,
            crew_roles,
            shells,
            max_ammo,
            equipment,
            supply_slots,
            customization_nation: vehicle
                .get_child_string("customizationNation")
                .filter(|name| !name.is_empty())
                .unwrap_or(nation)
                .to_owned(),
            default_emblem: vehicle
                .get_child_value_at(&["emblems", "default"])
                .and_then(pxml::Value::as_integer)
                .and_then(|id| u16::try_from(id).ok()),
            post_progression_tree: vehicle
                .get_child_string("postProgressionTree")
                .map(str::to_owned),
        })
    }
    pub fn default_ammo(&self) -> Vec<(IntCompactDescr, u32)> {
        let mut ammo: Vec<_> = self
            .shells
            .iter()
            .map(|&(cd, portion)| (cd, (portion * self.max_ammo as f32).round() as u32))
            .collect();
        let total: u64 = ammo.iter().map(|&(_, count)| u64::from(count)).sum();
        if let Some((_, first)) = ammo.first_mut() {
            *first =
                u32::try_from((u64::from(*first) + u64::from(self.max_ammo)).saturating_sub(total))
                    .unwrap_or(0);
        }
        ammo
    }
}

pub(super) fn resource_extensions(fs: &ResourceBundle) -> Vec<String> {
    let mut extensions = fs
        .read_dir("")
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|extension| fs.has_file(&format!("{extension}/extension.xml")))
        .collect::<Vec<_>>();
    extensions.sort();
    extensions
}

pub(super) fn vehicle_dirs(fs: &ResourceBundle, nation: &str) -> Vec<String> {
    let extensions = resource_extensions(fs)
        .into_iter()
        .filter(|extension| {
            read_xml(fs, &format!("{extension}/extension.xml"))
                .is_ok_and(|extension| extension.get_child_boolean("IsEnabled") == Some(true))
        })
        .map(|name| format!("{name}/"));
    std::iter::once(String::new())
        .chain(extensions)
        .map(|root| format!("{root}scripts/item_defs/vehicles/{nation}"))
        .filter(|dir| fs.has_file(&format!("{dir}/list.xml")))
        .collect()
}

pub(crate) fn read_xml(fs: &ResourceBundle, path: &str) -> io::Result<Box<Element>> {
    let file = fs.read(path)?;
    pxml::from_reader(io::BufReader::new(file)).map_err(|e| invalid(format!("{path}: {e}")))
}

pub(crate) fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
