use super::{GarageCatalog, items::IntCompactDescr};
use crate::{AccountSnapshot, CrewRoster, ProgressionError};
use std::collections::BTreeMap;

const UNDERTRAINED_LIMIT: u32 = 210_060;

impl GarageCatalog {
    pub(super) fn convert_undertrained_crew(
        &self,
        snapshot: &mut AccountSnapshot,
        crew: &mut CrewRoster,
    ) -> Result<(), ProgressionError> {
        let ids: Vec<_> = crew
            .members
            .iter()
            .filter_map(|(&id, member)| {
                let passports = self.passports.get(usize::from(member.nation))?;
                (member.seat.is_none()
                    && member.dismissed_at.is_none()
                    && member.training.skill_xp() < UNDERTRAINED_LIMIT
                    && passports.is_standard_passport(&member.passport))
                .then_some(id)
            })
            .collect();

        let mut by_nation = BTreeMap::new();
        for id in ids {
            Self::return_crew_skin(snapshot, crew, id)?;
            let member = crew
                .members
                .remove(&id)
                .ok_or(ProgressionError::CrewMemberNotFound)?;
            let xp = member.training.skill_xp();
            if xp > 0 {
                let vehicle = self.specialization_vehicle(&member)?;
                let slots = u32::try_from(vehicle.roles.len())
                    .map_err(|_| ProgressionError::InvalidRequest)?;
                let share = xp
                    .checked_div(slots)
                    .ok_or(ProgressionError::InvalidRequest)?;
                let total = by_nation.entry(member.nation).or_insert(0_u32);
                *total = total
                    .checked_add(share)
                    .ok_or(ProgressionError::InvalidRequest)?;
            }
        }

        for (nation, xp) in by_nation {
            for (book, count) in self.conversion_books(nation, xp)? {
                if count > 0 {
                    let stock = snapshot.progression.depot.entry(book).or_default();
                    *stock = stock
                        .checked_add(count)
                        .ok_or(ProgressionError::InvalidRequest)?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn specialization_vehicle(
        &self,
        member: &crate::CrewMember,
    ) -> Result<crate::CrewVehicle, ProgressionError> {
        let cd = IntCompactDescr {
            item_type: super::items::item_type::VEHICLE,
            nation: member.nation,
            id: member.specialization,
        };
        self.crew_vehicle(cd.to_int() as i32)
    }
}
