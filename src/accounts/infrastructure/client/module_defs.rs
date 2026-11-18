use super::{
    item_defs::{self, VehicleDef},
    item_prices::ItemPrices,
    items::{IntCompactDescr, VehicleDescr},
};
use crate::{
    accounts::{
        AmmunitionRules, AttachmentSlot, InscriptionSlot, ModuleDefinition, ModuleKind,
        ModuleLoadout, ModuleRules, PaintRegion, ProjectionDecalSlot, ResearchStep, ShellPrice,
        VehicleCosmeticSlots, VehicleSurface,
    },
    platform::client_formats::pxml::Element,
};
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use wot_resources::ResourceBundle;

pub(super) struct VehicleModules {
    pub rules: ModuleRules,
    pub ammunition: BTreeMap<(i32, i32), AmmunitionRules>,
    exterior_slots: BTreeMap<(i32, i32), VehicleCosmeticSlots>,
    chassis_exteriors: BTreeMap<i32, ChassisExterior>,
}

struct ChassisExterior {
    paintable: bool,
    effect_slot: bool,
    projection_slots: BTreeMap<u16, ProjectionDecalSlot>,
    attachment_slots: BTreeMap<u16, AttachmentSlot>,
}

impl VehicleModules {
    pub(super) fn exterior_slots(&self, modules: &ModuleLoadout) -> Option<VehicleCosmeticSlots> {
        let mut exterior = self
            .exterior_slots
            .get(&(
                *modules.get(&ModuleKind::Turret)?,
                *modules.get(&ModuleKind::Gun)?,
            ))?
            .clone();
        if let Some(chassis) = modules
            .get(&ModuleKind::Chassis)
            .and_then(|id| self.chassis_exteriors.get(id))
        {
            if chassis.paintable {
                exterior.paints.insert(PaintRegion::Chassis);
            }
            exterior.effect_slot |= chassis.effect_slot;
            exterior.projection_slots.extend(
                chassis
                    .projection_slots
                    .iter()
                    .map(|(&id, slot)| (id, slot.clone())),
            );
            exterior.attachment_slots.extend(
                chassis
                    .attachment_slots
                    .iter()
                    .map(|(&id, slot)| (id, slot.clone())),
            );
        }
        Some(exterior)
    }

    pub fn load(
        fs: &ResourceBundle,
        nation: &str,
        name: &str,
        stock: &VehicleDef,
        prices: &ItemPrices,
        shell_prices: &BTreeMap<i32, ShellPrice>,
    ) -> Result<Self> {
        let dir = item_defs::vehicle_dirs(fs, nation)
            .into_iter()
            .find(|dir| {
                item_defs::read_xml(fs, &format!("{dir}/list.xml"))
                    .is_ok_and(|list| list.get_child(name).is_some())
            })
            .context("Missing vehicle directory")?;
        let vehicle = item_defs::read_xml(fs, &format!("{dir}/{name}.xml"))?;
        let nation_id = stock.descr.nation;
        let mut components = BTreeMap::new();
        let mut ids = BTreeMap::new();
        for (kind, file) in [
            (1, "list"),
            (2, "chassis"),
            (3, "turrets"),
            (4, "guns"),
            (5, "engines"),
            (6, "fuelTanks"),
            (7, "radios"),
        ] {
            let xml = item_defs::read_xml(
                fs,
                &if kind == 1 {
                    format!("{dir}/list.xml")
                } else {
                    format!("{dir}/components/{file}.xml")
                },
            )?;
            let entries = if kind == 1 {
                Some(xml.as_ref())
            } else {
                xml.get_child_element("ids")
            };
            for (name, value) in entries.into_iter().flat_map(Element::iter_children_all) {
                let id = if kind == 1 {
                    value.as_element().and_then(|e| e.get_child_integer("id"))
                } else {
                    value.as_integer()
                };
                if let Some(id) = id {
                    let id = u16::try_from(id).context("Invalid module identifier")?;
                    ids.insert(
                        (kind, name.clone()),
                        IntCompactDescr {
                            item_type: kind,
                            nation: nation_id,
                            id,
                        }
                        .to_int() as i32,
                    );
                }
            }
            components.insert(kind, xml);
        }
        let mut rules = ModuleRules {
            vehicle: stock.descr.int_descr().to_int() as i32,
            stock: loadout(&stock.descr),
            hull_weight: vehicle
                .get_child_element("hull")
                .and_then(|h| h.get_child_float("weight"))
                .context("Missing hull weight")?,
            ..Default::default()
        };
        read_research(&vehicle, &ids, &[], &mut rules.research)?;
        let mut ammunition = BTreeMap::new();
        let mut exterior_slots = BTreeMap::new();
        let mut chassis_exteriors = BTreeMap::new();
        let hull_paints: BTreeSet<_> = paint_regions(vehicle.get_child_element("hull")).collect();
        let hull_projection_slots =
            projection_decal_slots(vehicle.get_child_element("hull")).collect::<BTreeMap<_, _>>();
        let hull_attachment_slots =
            attachment_slots(vehicle.get_child_element("hull")).collect::<BTreeMap<_, _>>();
        let hull_effect_slot = has_effect_slot(vehicle.get_child_element("hull"));
        let hull_inscriptions = inscription_slots(vehicle.get_child_element("hull"));
        let hull_camouflage = vehicle
            .get_child_element("hull")
            .is_some_and(|hull| super::cosmetic_catalog::has_camouflage_area(hull, "HULL"));
        let shells = item_defs::read_xml(fs, &format!("{dir}/components/shells.xml"))?;
        for (kind, section) in [
            (2, "chassis"),
            (5, "engines"),
            (6, "fuelTanks"),
            (7, "radios"),
            (3, "turrets0"),
        ] {
            for (name, local) in vehicle
                .get_child_element(section)
                .into_iter()
                .flat_map(Element::iter_children_all)
            {
                let id = *ids
                    .get(&(kind, name.clone()))
                    .context("Missing component ID")?;
                let shared = components
                    .get(&kind)
                    .and_then(|xml| xml.get_child_element("shared"))
                    .and_then(|xml| xml.get_child_element(name));
                let local = local.as_element();
                let base = shared.or(local).context("Missing module definition")?;
                let number = |key| {
                    local
                        .and_then(|l| l.get_child_float(key))
                        .or_else(|| base.get_child_float(key))
                };
                let price = prices
                    .prices
                    .iter()
                    .find(|(cd, _)| cd.to_int() == i64::from(id))
                    .filter(|(cd, p)| p.currency == "credits" && !prices.not_in_shop.contains(cd))
                    .map(|(_, p)| p.amount);
                rules.modules.insert(
                    id,
                    ModuleDefinition {
                        kind: module_kind(kind)?,
                        price,
                        weight: number("weight").context("Missing module weight")?,
                        max_weight: if kind == 2 { number("maxLoad") } else { None },
                    },
                );
                if kind == 2 {
                    chassis_exteriors.insert(
                        id,
                        ChassisExterior {
                            paintable: paint_regions(Some(base))
                                .any(|region| region == PaintRegion::Chassis),
                            effect_slot: has_effect_slot(Some(base)),
                            projection_slots: projection_decal_slots(Some(base)).collect(),
                            attachment_slots: attachment_slots(Some(base)).collect(),
                        },
                    );
                }
                if kind == 3 {
                    let guns = local
                        .and_then(|l| l.get_child_element("guns"))
                        .or_else(|| base.get_child_element("guns"))
                        .context("Missing turret guns")?;
                    let mut compatible = BTreeSet::new();
                    for (name, gun_local) in guns.iter_children_all() {
                        let gun = *ids.get(&(4, name.clone())).context("Missing gun ID")?;
                        compatible.insert(gun);
                        let gun_local = gun_local.as_element();
                        let gun_shared = components
                            .get(&4)
                            .and_then(|xml| xml.get_child_element("shared"))
                            .and_then(|xml| xml.get_child_element(name));
                        let gun_base =
                            gun_shared.or(gun_local).context("Missing gun definition")?;
                        let surfaces = [
                            (VehicleSurface::Hull, hull_camouflage),
                            (
                                VehicleSurface::Turret,
                                super::cosmetic_catalog::has_camouflage_area(base, "TURRET"),
                            ),
                            (
                                VehicleSurface::Gun,
                                gun_local.is_some_and(|gun| {
                                    super::cosmetic_catalog::has_camouflage_area(gun, "GUN")
                                }),
                            ),
                        ];
                        use InscriptionSlot::*;
                        let show_on_gun = base.get_child_boolean("showEmblemsOnGun") == Some(true);
                        let turret_slots = inscription_slots(Some(base));
                        let gun_slot_definition = gun_local
                            .filter(|gun| {
                                gun.get_child("emblemSlots").is_some()
                                    || gun.get_child("customizationSlots").is_some()
                            })
                            .unwrap_or(gun_base);
                        let gun_slots = inscription_slots(Some(gun_slot_definition));
                        let inscription_regions = [
                            ([HullFirst, HullSecond], hull_inscriptions),
                            (
                                [TurretFirst, TurretSecond],
                                if show_on_gun { 0 } else { turret_slots },
                            ),
                            (
                                [GunFirst, GunSecond],
                                if show_on_gun { turret_slots } else { gun_slots },
                            ),
                        ];
                        exterior_slots.insert(
                            (id, gun),
                            VehicleCosmeticSlots {
                                paints: hull_paints
                                    .iter()
                                    .copied()
                                    .chain(paint_regions(Some(base)))
                                    .chain(paint_regions(gun_local))
                                    .collect(),
                                effect_slot: hull_effect_slot
                                    || has_effect_slot(Some(base))
                                    || has_effect_slot(Some(gun_slot_definition)),
                                projection_slots: projection_decal_slots(Some(gun_slot_definition))
                                    .chain(projection_decal_slots(Some(base)))
                                    .chain(
                                        hull_projection_slots
                                            .iter()
                                            .map(|(&id, slot)| (id, slot.clone())),
                                    )
                                    .collect(),
                                attachment_slots: attachment_slots(Some(gun_slot_definition))
                                    .chain(attachment_slots(Some(base)))
                                    .chain(
                                        hull_attachment_slots
                                            .iter()
                                            .map(|(&id, slot)| (id, slot.clone())),
                                    )
                                    .collect(),
                                camouflage: surfaces
                                    .into_iter()
                                    .filter_map(|(surface, supported)| supported.then_some(surface))
                                    .collect(),
                                inscriptions: inscription_regions
                                    .into_iter()
                                    .flat_map(|(slots, count)| slots.into_iter().take(count))
                                    .collect(),
                            },
                        );
                        let gun_number = |key| {
                            gun_local
                                .and_then(|l| l.get_child_float(key))
                                .or_else(|| gun_base.get_child_float(key))
                        };
                        let price = prices
                            .prices
                            .iter()
                            .find(|(cd, _)| cd.to_int() == i64::from(gun))
                            .filter(|(cd, p)| {
                                p.currency == "credits" && !prices.not_in_shop.contains(cd)
                            })
                            .map(|(_, p)| p.amount);
                        rules.modules.insert(
                            gun,
                            ModuleDefinition {
                                kind: ModuleKind::Gun,
                                price,
                                weight: gun_number("weight").context("Missing gun weight")?,
                                max_weight: None,
                            },
                        );
                        let max_ammo = gun_local
                            .and_then(|l| l.get_child_integer("maxAmmo"))
                            .or_else(|| gun_base.get_child_integer("maxAmmo"))
                            .context("Missing gun capacity")?;
                        let capacity = u32::try_from(max_ammo).context("Invalid gun capacity")?;
                        let shots = gun_local
                            .and_then(|l| l.get_child_element("shots"))
                            .or_else(|| gun_base.get_child_element("shots"))
                            .context("Missing gun shots")?;
                        let mut defaults = Vec::new();
                        for (name, shot) in shots.iter_children_all() {
                            let shell_id = shells
                                .get_child_element(name)
                                .and_then(|s| s.get_child_integer("id"))
                                .context("Missing shell ID")?;
                            let shell_id = IntCompactDescr {
                                item_type: 10,
                                nation: nation_id,
                                id: u16::try_from(shell_id)?,
                            }
                            .to_int() as i32;
                            let portion = shot
                                .as_element()
                                .and_then(|s| s.get_child_float("defaultPortion"))
                                .unwrap_or(0.0);
                            defaults.push((shell_id, (portion * capacity as f32).round() as u32));
                        }
                        let total: u64 = defaults.iter().map(|(_, n)| u64::from(*n)).sum();
                        if let Some((_, first)) = defaults.first_mut() {
                            *first = u32::try_from(
                                (u64::from(*first) + u64::from(capacity)).saturating_sub(total),
                            )?;
                        }
                        let prices = defaults
                            .iter()
                            .filter_map(|(id, _)| Some((*id, *shell_prices.get(id)?)))
                            .collect();
                        ammunition.insert(
                            (id, gun),
                            AmmunitionRules {
                                capacity,
                                defaults,
                                prices,
                                gold_to_credits: super::shop::AMMO_EXCHANGE_RATE,
                            },
                        );
                        if let Some(local) = gun_local {
                            read_research(local, &ids, &[gun, id], &mut rules.research)?;
                        }
                    }
                    rules.turret_guns.insert(id, compatible);
                }
                if let Some(local) = local {
                    read_research(local, &ids, &[id], &mut rules.research)?;
                }
            }
        }
        let mut requirements: BTreeMap<i32, BTreeSet<i32>> = BTreeMap::new();
        for step in &rules.research {
            requirements
                .entry(step.item)
                .or_default()
                .extend(step.prerequisites.iter().copied());
        }
        for _ in 0..requirements.len() {
            let previous = requirements.clone();
            for deps in requirements.values_mut() {
                for id in deps.clone() {
                    if let Some(inherited) = previous.get(&id) {
                        deps.extend(inherited);
                    }
                }
            }
            if requirements == previous {
                break;
            }
        }
        for step in &mut rules.research {
            step.prerequisites = requirements
                .get(&step.item)
                .into_iter()
                .flatten()
                .copied()
                .collect();
        }
        Ok(Self {
            rules,
            ammunition,
            exterior_slots,
            chassis_exteriors,
        })
    }

    pub fn project(&self, descr: &mut VehicleDescr, installed: &ModuleLoadout) -> Result<()> {
        for (&kind, &id) in installed {
            anyhow::ensure!(
                self.rules.modules.get(&id).is_some_and(|m| m.kind == kind),
                "Invalid saved module"
            );
            let id = IntCompactDescr::from_int(i64::from(id)).id;
            match kind {
                ModuleKind::Chassis => descr.chassis = id,
                ModuleKind::Turret => descr.turret = id,
                ModuleKind::Gun => descr.gun = id,
                ModuleKind::Engine => descr.engine = id,
                ModuleKind::FuelTank => descr.fuel_tank = id,
                ModuleKind::Radio => descr.radio = id,
            }
        }
        Ok(())
    }
}

fn paint_regions(part: Option<&Element>) -> impl Iterator<Item = PaintRegion> + '_ {
    part.and_then(|part| {
        part.get_child_value_at(&["customization", "customizableVehicleAreas", "paint"])
    })
    .and_then(|areas| areas.as_string())
    .unwrap_or_default()
    .split_whitespace()
    .filter_map(super::cosmetic_catalog::paint_region)
}

fn has_effect_slot(part: Option<&Element>) -> bool {
    // Vehicle falls back to the style anchor when there is no explicit effect anchor.
    part.and_then(|part| part.get_child_element("customizationSlots"))
        .into_iter()
        .flat_map(Element::iter_child_elements)
        .any(|(_, slot)| matches!(slot.get_child_string("slotType"), Some("style" | "effect")))
}

fn attachment_slots(part: Option<&Element>) -> impl Iterator<Item = (u16, AttachmentSlot)> {
    part.and_then(|part| part.get_child_element("customizationSlots"))
        .into_iter()
        .flat_map(Element::iter_child_elements)
        .filter_map(|(_, slot)| {
            if slot.get_child_string("slotType") != Some("attachment")
                || !user_customization_slot(slot)
            {
                return None;
            }
            let id = u16::try_from(slot.get_child_integer("slotId")?).ok()?;
            let apply_type = slot
                .get_child_string("applyType")
                .unwrap_or_default()
                .to_owned();
            let scale = super::cosmetic_catalog::attachment_scale(slot.get_child_string("size"))?;
            Some((id, AttachmentSlot { apply_type, scale }))
        })
}

fn user_customization_slot(slot: &Element) -> bool {
    !item_defs::xml_flag(slot, "hiddenForUser")
        && slot
            .get_child_string("compatibleModels")
            .is_none_or(|models| models.split_whitespace().any(|model| model == "default"))
}

fn projection_decal_slots(
    part: Option<&Element>,
) -> impl Iterator<Item = (u16, ProjectionDecalSlot)> {
    part.and_then(|part| part.get_child_element("customizationSlots"))
        .into_iter()
        .flat_map(Element::iter_child_elements)
        .filter_map(|(_, slot)| {
            if slot.get_child_string("slotType") != Some("projectionDecal")
                || !user_customization_slot(slot)
            {
                return None;
            }
            let id = u16::try_from(slot.get_child_integer("slotId")?).ok()?;
            let forms = slot
                .get_child_string("tags")
                .unwrap_or_default()
                .split_whitespace()
                .filter(|tag| tag.starts_with("formfactor_"))
                .map(str::to_owned)
                .collect();
            Some((
                id,
                ProjectionDecalSlot {
                    forms,
                    vertical_mirror: item_defs::xml_flag(slot, "verticalMirror"),
                },
            ))
        })
}

fn inscription_slots(part: Option<&Element>) -> usize {
    let Some(part) = part else { return 0 };
    if let Some(slots) = part.get_child_element("emblemSlots") {
        return slots
            .iter_child_elements()
            .filter(|(name, _)| *name == "inscription")
            .count();
    }
    part.get_child_element("customizationSlots")
        .into_iter()
        .flat_map(Element::iter_child_elements)
        .filter(|(_, slot)| slot.get_child_string("slotType") == Some("inscription"))
        .count()
}

fn read_research(
    xml: &Element,
    ids: &BTreeMap<(u8, String), i32>,
    prerequisites: &[i32],
    steps: &mut Vec<ResearchStep>,
) -> Result<()> {
    for (kind, value) in xml
        .get_child_element("unlocks")
        .into_iter()
        .flat_map(Element::iter_children_all)
    {
        let kind = match kind.as_str() {
            "vehicle" => 1,
            "chassis" => 2,
            "turret" => 3,
            "gun" => 4,
            "engine" => 5,
            "fuelTank" => 6,
            "radio" => 7,
            _ => anyhow::bail!("Unknown research type"),
        };
        let xml = value.as_element().context("Invalid research entry")?;
        let name = xml
            .value
            .as_string()
            .context("Missing research item")?
            .trim()
            .to_owned();
        let item = *ids.get(&(kind, name)).context("Unknown research item")?;
        let xp = xml.get_child_integer("cost").unwrap_or(0);
        anyhow::ensure!(xp >= 0, "Negative research cost");
        steps.push(ResearchStep {
            item,
            xp,
            prerequisites: prerequisites.to_vec(),
        });
    }
    Ok(())
}

pub(super) fn loadout(descr: &VehicleDescr) -> ModuleLoadout {
    descr
        .module_int_descrs()
        .into_iter()
        .filter_map(|cd| Some((module_kind(cd.item_type).ok()?, cd.to_int() as i32)))
        .collect()
}

fn module_kind(kind: u8) -> Result<ModuleKind> {
    Ok(match kind {
        2 => ModuleKind::Chassis,
        3 => ModuleKind::Turret,
        4 => ModuleKind::Gun,
        5 => ModuleKind::Engine,
        6 => ModuleKind::FuelTank,
        7 => ModuleKind::Radio,
        _ => anyhow::bail!("Invalid module kind"),
    })
}
