use super::{
    GarageCatalog,
    item_defs::{self, NATIONS},
    items::{TankmanDescr, tankman_flag},
};
use crate::{
    AccountSnapshot, CrewCommand, CrewMember, CrewPassport, CrewRecruitment, CrewRoster,
    CrewTraining, CrewVehicle, PassportChoice, ProgressionError, VehicleCost, reset_perk_terms,
};
use std::{
    collections::BTreeMap,
    io,
    time::{SystemTime, UNIX_EPOCH},
};
use wot_platform::client_formats::{pickle::Value, pxml::Element};
use wot_resources::ResourceBundle;

pub(super) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub(super) fn crew_descriptor(member: &CrewMember) -> Vec<u8> {
    let mut flags = 0;
    if member.special_recruit.is_some() {
        flags |= tankman_flag::IS_PREMIUM;
    }
    if member.female {
        flags |= tankman_flag::IS_FEMALE;
    }
    TankmanDescr {
        nation: member.nation,
        flags: vec![flags],
        vehicle_type_id: member.specialization,
        role: member.role,
        skills_efficiency_xp: member.training.efficiency_xp,
        skills: member.training.perks.clone(),
        last_skill_level: member.training.last_perk_level,
        free_skills: 0,
        bonus_skills: member.training.bonus_perks.clone(),
        first_name_id: member.passport.first_name,
        last_name_id: member.passport.last_name,
        icon_id: member.passport.portrait,
        free_xp: member.training.unspent_xp,
        dossier: Vec::new(),
    }
    .encode()
}

pub(super) fn recycle_rows(crew: &CrewRoster) -> BTreeMap<Value, Value> {
    crew.members
        .iter()
        .filter_map(|(&id, member)| {
            let at = i64::try_from(member.dismissed_at?).ok()?;
            Some((
                Value::from(id),
                wot_platform::pickle!(((Value::Bytes(crew_descriptor(member))), (at))),
            ))
        })
        .collect()
}

pub(super) fn recycle_bin(crew: &CrewRoster) -> Value {
    wot_platform::pickle!({"tankmen": {"buffer": (Value::Dict(recycle_rows(crew)))}})
}

impl GarageCatalog {
    pub(super) fn crew_roster(&self, snapshot: &AccountSnapshot) -> anyhow::Result<CrewRoster> {
        if let Some(crew) = &snapshot.crew {
            return Ok(crew.clone());
        }
        let mut crew = CrewRoster::default();
        for owned in &snapshot.vehicles {
            if owned.has_crew {
                let vehicle = self.owned_crew_vehicle(snapshot, owned.inventory_id)?;
                self.add_vehicle_crew(&mut crew, &vehicle)?;
            }
        }
        Ok(crew)
    }

    pub(super) fn add_vehicle_crew(
        &self,
        crew: &mut CrewRoster,
        vehicle: &CrewVehicle,
    ) -> Result<(), ProgressionError> {
        // A locked crew can receive its factory assignment.
        let mut initial_vehicle = vehicle.clone();
        initial_vehicle.crew_locked = false;
        for (slot, roles) in vehicle.roles.iter().enumerate() {
            let role = *roles.first().ok_or(ProgressionError::InvalidRequest)?;
            let member = crew.insert(self.new_crew_member(vehicle, role, slot)?)?;
            crew.assign(&initial_vehicle, Some(slot), Some(member))?;
        }
        Ok(())
    }

    pub(super) fn owned_crew_vehicle(
        &self,
        snapshot: &AccountSnapshot,
        id: i64,
    ) -> Result<CrewVehicle, ProgressionError> {
        let owned = snapshot
            .vehicles
            .iter()
            .find(|vehicle| vehicle.inventory_id == id)
            .ok_or(ProgressionError::VehicleNotFound)?;
        let cd = *self
            .keys
            .get(&owned.catalog_key)
            .ok_or(ProgressionError::VehicleNotFound)?;
        let mut vehicle = self.crew_vehicle(cd)?;
        vehicle.inventory_id = Some(id);
        Ok(vehicle)
    }

    pub(super) fn crew_vehicle(&self, cd: i32) -> Result<CrewVehicle, ProgressionError> {
        let offer = self
            .offers
            .get(&cd)
            .ok_or(ProgressionError::VehicleNotFound)?;
        let template = self.vehicle(&offer.key).map_err(Self::catalog_failure)?;
        Ok(CrewVehicle {
            inventory_id: None,
            nation: template.template.descr.nation,
            specialization: template.template.descr.type_id,
            roles: template.crew_roles.clone(),
            crew_locked: offer.tags.contains("lockCrew"),
            perks_locked: offer.tags.contains("lockCrewSkills"),
            premium: offer.tags.contains("premium") || offer.tags.contains("premiumIGR"),
            no_transfer_penalty: offer.tags.contains("noCrewTransferPenalty")
                || offer.tags.contains("wotPlus"),
            class_tag: ["lightTank", "mediumTank", "heavyTank", "SPG", "AT-SPG"]
                .into_iter()
                .find(|&tag| offer.tags.contains(tag)),
        })
    }

    pub(super) fn new_crew_member(
        &self,
        vehicle: &CrewVehicle,
        role: u8,
        index: usize,
    ) -> Result<CrewMember, ProgressionError> {
        let passports = self
            .passports
            .get(usize::from(vehicle.nation))
            .ok_or(ProgressionError::CatalogUnavailable)?;
        Ok(CrewMember {
            nation: vehicle.nation,
            specialization: vehicle.specialization,
            role,
            passport: passports.passport(index),
            seat: None,
            training: CrewTraining::default(),
            dismissed_at: None,
            role_change_target: None,
            skin: None,
            special_recruit: None,
            female: false,
        })
    }

    pub(super) fn check_crew_seat(
        &self,
        snapshot: &AccountSnapshot,
        member: &CrewMember,
        perks: bool,
    ) -> Result<(), ProgressionError> {
        if let Some(seat) = member.seat {
            let vehicle = self.owned_crew_vehicle(snapshot, seat.vehicle)?;
            if if perks {
                vehicle.perks_locked
            } else {
                vehicle.crew_locked
            } {
                return Err(ProgressionError::InvalidRequest);
            }
        }
        Ok(())
    }

    pub(super) fn apply_crew(
        &self,
        snapshot: &mut AccountSnapshot,
        crew: &mut CrewRoster,
        command: CrewCommand,
        defaults: &std::collections::BTreeSet<i32>,
    ) -> Result<Option<i64>, ProgressionError> {
        match command {
            CrewCommand::Hire {
                vehicle,
                role,
                course,
            } => {
                let vehicle = self.crew_vehicle(vehicle)?;
                let member = self.hire(snapshot, crew, &vehicle, role, course)?;
                crew.validate_berths(snapshot.balances.berths)?;
                return Ok(Some(member));
            }
            CrewCommand::HireAndSeat {
                vehicle,
                slot,
                course,
            } => {
                let vehicle = self.owned_crew_vehicle(snapshot, vehicle)?;
                let role = vehicle
                    .roles
                    .get(slot)
                    .and_then(|roles| roles.first())
                    .copied()
                    .ok_or(ProgressionError::InvalidRequest)?;
                let member = self.hire(snapshot, crew, &vehicle, role, course)?;
                crew.assign(&vehicle, Some(slot), Some(member))?;
                crew.validate_berths(snapshot.balances.berths)?;
                return Ok(Some(member));
            }
            CrewCommand::Assign {
                vehicle,
                slot,
                member,
            } => {
                let vehicle = self.owned_crew_vehicle(snapshot, vehicle)?;
                if let Some(member) = member {
                    self.check_crew_seat(snapshot, crew.active(member)?, false)?;
                }
                crew.assign(&vehicle, slot, member)?;
            }
            CrewCommand::Return(id) => {
                let vehicle = self.owned_crew_vehicle(snapshot, id)?;
                let previous = crew
                    .previous_crews
                    .get(&id)
                    .cloned()
                    .ok_or(ProgressionError::CrewReturnUnavailable)?;
                let mut available = false;
                for (slot, member) in previous.into_iter().enumerate() {
                    let Some(member) = member else { continue };
                    let Ok(candidate) = crew.active(member) else {
                        continue;
                    };
                    if vehicle.roles.get(slot).and_then(|roles| roles.first())
                        != Some(&candidate.role)
                        || self.check_crew_seat(snapshot, candidate, false).is_err()
                    {
                        continue;
                    }
                    crew.assign(&vehicle, Some(slot), Some(member))?;
                    available = true;
                }
                if !available {
                    return Err(ProgressionError::CrewReturnUnavailable);
                }
            }
            CrewCommand::AutoReturn { vehicle, enabled } => {
                let vehicle_spec = self.owned_crew_vehicle(snapshot, vehicle)?;
                if vehicle_spec.crew_locked {
                    return Err(ProgressionError::InvalidRequest);
                }
                crew.auto_return.insert(vehicle, enabled);
            }
            CrewCommand::AcceleratedTraining { vehicle, enabled } => {
                let template = self.owned_vehicle_template(snapshot, vehicle)?;
                if enabled {
                    if !template.is_elite(defaults, &snapshot.progression.unlocks) {
                        return Err(ProgressionError::InvalidRequest);
                    }
                    crew.accelerated_training.insert(vehicle);
                } else {
                    crew.accelerated_training.remove(&vehicle);
                }
            }
            CrewCommand::Dismiss(id) => {
                self.check_crew_seat(snapshot, crew.active(id)?, false)?;
                Self::return_crew_skin(snapshot, crew, id)?;
                crew.dismiss(id, now())?;
            }
            CrewCommand::Restore(id) => crew.restore(id, now(), snapshot)?,
            CrewCommand::ConvertUndertrained => self.convert_undertrained_crew(snapshot, crew)?,
            CrewCommand::BuyBerths(packs) => snapshot.buy_berths(packs)?,
            CrewCommand::Train { member, free_xp } => {
                self.check_crew_seat(snapshot, crew.active(member)?, true)?;
                let xp = snapshot.spend_crew_free_xp(free_xp)?;
                let training = &mut crew.active_mut(member)?.training;
                if xp
                    > training
                        .xp_capacity()
                        .saturating_add(crate::FREE_XP_RATE - 1)
                    || training.xp_capacity() == 0
                {
                    return Err(ProgressionError::InvalidRequest);
                }
                training.add_xp(xp);
            }
            CrewCommand::Learn {
                member,
                bonus,
                perks,
            } => {
                let member = crew.active_mut(member)?;
                self.check_crew_seat(snapshot, member, true)?;
                let secondary = if let Some(seat) = member.seat {
                    let vehicle = self.owned_crew_vehicle(snapshot, seat.vehicle)?;
                    vehicle
                        .roles
                        .get(seat.slot)
                        .map_or_else(Vec::new, |roles| roles.iter().skip(1).copied().collect())
                } else {
                    let vehicle = self.specialization_vehicle(member)?;
                    vehicle
                        .roles
                        .iter()
                        .filter(|roles| roles.first() == Some(&member.role))
                        .flat_map(|roles| roles.iter().skip(1).copied())
                        .collect()
                };
                member
                    .training
                    .learn(member.role, &secondary, bonus, &perks)?;
            }
            CrewCommand::Reset { member, course } => {
                self.check_crew_seat(snapshot, crew.active(member)?, true)?;
                let (price, refund) = reset_perk_terms(course)?;
                price.charge(snapshot)?;
                crew.active_mut(member)?.training.reset(refund)?;
            }
            CrewCommand::Retrain {
                member,
                vehicle,
                course,
            } => {
                let vehicle = self.crew_vehicle(vehicle)?;
                let member = crew.active_mut(member)?;
                self.retrain(snapshot, member, &vehicle, member.role, course)?;
            }
            CrewCommand::ChangeRole {
                member,
                vehicle,
                role,
            } => {
                let member = crew.active_mut(member)?;
                let vehicle = if vehicle == 0 {
                    self.specialization_vehicle(member)?
                } else {
                    self.crew_vehicle(vehicle)?
                };
                self.check_crew_seat(snapshot, member, false)?;
                if !self.recruit_allows_role(member, role) {
                    return Err(ProgressionError::InvalidRequest);
                }
                validate_specialization(member, &vehicle, role)?;
                if member.role != role {
                    member.training.change_role(role);
                    member.role = role;
                    member.seat = None;
                    member.role_change_target = Some(vehicle.specialization);
                }
            }
            CrewCommand::EquipSkin { member, skin } => {
                self.equip_crew_skin(snapshot, crew, member, skin)?
            }
            CrewCommand::ChangePassport {
                member,
                first_name,
                last_name,
                portrait,
            } => {
                let member = crew.active_mut(member)?;
                self.check_crew_seat(snapshot, member, false)?;
                if member.special_recruit.is_some() {
                    return Err(ProgressionError::InvalidRequest);
                }
                let passports = self
                    .passports
                    .get(usize::from(member.nation))
                    .ok_or(ProgressionError::CatalogUnavailable)?;
                passports.apply(&mut member.passport, [first_name, last_name, portrait])?;
            }
            CrewCommand::UseBook {
                book,
                count,
                vehicle,
                member,
            } => {
                self.use_crew_book(snapshot, crew, book, count, vehicle, member)?;
            }
        }
        crew.validate_berths(snapshot.balances.berths)?;
        Ok(None)
    }

    fn hire(
        &self,
        snapshot: &mut AccountSnapshot,
        crew: &mut CrewRoster,
        vehicle: &CrewVehicle,
        role: u8,
        course: CrewRecruitment,
    ) -> Result<i64, ProgressionError> {
        if vehicle.crew_locked
            || !vehicle
                .roles
                .iter()
                .any(|roles| roles.first() == Some(&role))
        {
            return Err(ProgressionError::InvalidRequest);
        }
        course.cost().charge(snapshot)?;
        let member = self.new_crew_member(vehicle, role, crew.next_passport_index())?;
        crew.insert(member)
    }

    fn retrain(
        &self,
        snapshot: &mut AccountSnapshot,
        member: &mut CrewMember,
        vehicle: &CrewVehicle,
        role: u8,
        course: CrewRecruitment,
    ) -> Result<(), ProgressionError> {
        self.check_crew_seat(snapshot, member, false)?;
        validate_specialization(member, vehicle, role)?;
        let role_changed = member.role_change_target == Some(vehicle.specialization);
        let full_perks = member.training.full_perks();
        let efficiency = match (full_perks, role_changed, course) {
            (_, true, CrewRecruitment::Free) => return Err(ProgressionError::InvalidRequest),
            (0, _, _) => crate::MAX_EFFICIENCY_XP,
            (_, true, CrewRecruitment::Credits) => 40_000,
            _ => course.retraining_efficiency(),
        };
        if !role_changed
            && member.specialization == vehicle.specialization
            && member.training.efficiency_xp >= efficiency
        {
            return Ok(());
        }
        let discount = full_perks.min(2) as i64;
        let price = course.cost();
        VehicleCost {
            credits: price.credits * discount / 2,
            gold: price.gold * discount / 2,
        }
        .charge(snapshot)?;
        member.specialization = vehicle.specialization;
        member.training.efficiency_xp = efficiency;
        member.role_change_target = None;
        Ok(())
    }
}
fn validate_specialization(
    member: &CrewMember,
    vehicle: &CrewVehicle,
    role: u8,
) -> Result<(), ProgressionError> {
    if member.nation != vehicle.nation
        || vehicle.crew_locked
        || !vehicle
            .roles
            .iter()
            .any(|roles| roles.first() == Some(&role))
    {
        return Err(ProgressionError::InvalidRequest);
    }
    Ok(())
}

pub(super) struct Passports {
    groups: Vec<(u16, [Vec<u16>; 3])>,
    standard_group: Option<u16>,
}

impl Passports {
    pub(super) fn load(fs: &ResourceBundle, nation: u8) -> io::Result<Self> {
        let nation_name = NATIONS
            .get(usize::from(nation))
            .ok_or_else(|| item_defs::invalid(format!("no nation {nation}")))?;
        let config =
            item_defs::read_xml(fs, &format!("scripts/item_defs/tankmen/{nation_name}.xml"))?;
        let groups = config
            .get_child_element("normalGroups")
            .into_iter()
            .flat_map(Element::iter_child_elements)
            .map(|(_, group)| group)
            .filter(|group| {
                group.get_child_string("sex").map(str::trim) == Some("male")
                    && group.get_child_boolean("notInShop") != Some(true)
            })
            .filter_map(|group| {
                let id = u16::try_from(group.get_child_integer("groupID")?).ok()?;
                let ids = ["firstNames", "lastNames", "icons"].map(|name| {
                    group
                        .get_child_element(name)
                        .into_iter()
                        .flat_map(Element::iter_children_all)
                        .filter_map(|(key, _)| key.strip_prefix('_')?.parse().ok())
                        .collect::<Vec<u16>>()
                });
                Some((id, ids))
            })
            .collect::<Vec<_>>();
        if groups
            .first()
            .is_none_or(|(_, ids)| ids.iter().any(Vec::is_empty))
        {
            return Err(item_defs::invalid(format!(
                "missing normal crew passports for {nation_name}"
            )));
        }
        let standard_group = config
            .get_child_element("normalGroups")
            .and_then(|groups| groups.get_child_element("men1"))
            .and_then(|group| group.get_child_integer("groupID"))
            .and_then(|id| u16::try_from(id).ok());
        Ok(Self {
            groups,
            standard_group,
        })
    }

    fn passport(&self, index: usize) -> CrewPassport {
        let [first_names, last_names, icons] = &self.groups[0].1;
        CrewPassport {
            first_name: first_names[index % first_names.len()],
            last_name: last_names[index % last_names.len()],
            portrait: icons[index % icons.len()],
        }
    }

    pub(super) fn is_standard_passport(&self, passport: &CrewPassport) -> bool {
        self.groups
            .iter()
            .any(|(id, [first_names, last_names, icons])| {
                Some(*id) == self.standard_group
                    && first_names.contains(&passport.first_name)
                    && last_names.contains(&passport.last_name)
                    && icons.contains(&passport.portrait)
            })
    }

    fn apply(
        &self,
        passport: &mut CrewPassport,
        choices: [Option<PassportChoice>; 3],
    ) -> Result<(), ProgressionError> {
        let fields = [
            &mut passport.first_name,
            &mut passport.last_name,
            &mut passport.portrait,
        ];
        for (index, (field, choice)) in fields.into_iter().zip(choices).enumerate() {
            let Some(choice) = choice else { continue };
            let valid = self
                .groups
                .iter()
                .any(|(group, ids)| *group == choice.group && ids[index].contains(&choice.id));
            if !valid {
                return Err(ProgressionError::InvalidRequest);
            }
            *field = choice.id;
        }
        Ok(())
    }
}
