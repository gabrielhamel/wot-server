use super::{AccountSnapshot, CrewRecruitment, ProgressionError, VehicleCost};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet},
    sync::LazyLock,
};

pub const MAX_CREW_MEMBERS: usize = 8192;
pub const MAX_BERTHS: i64 = 2048;
pub const BERTH_PACK_SIZE: i64 = 16;
pub const BERTH_PACK_GOLD: i64 = 300;
pub const FREE_XP_RATE: u32 = 5;
pub const MAX_EFFICIENCY_XP: u32 = 100_000;
pub const MAX_MAJOR_PERKS: usize = 6;
pub const CREW_RESTORE_LIMIT: usize = 100;
pub const CREW_RESTORE_DURATION: u64 = 2_764_800;
pub const CREW_FREE_RESTORE_DURATION: u64 = 172_800;
pub const CREW_RESTORE_CREDITS: i64 = 60_000;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CrewRoster {
    pub members: BTreeMap<i64, CrewMember>,
    pub previous_crews: BTreeMap<i64, Vec<Option<i64>>>,
    pub auto_return: BTreeMap<i64, bool>,
    #[serde(default)]
    pub accelerated_training: BTreeSet<i64>,
    next_id: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct CrewSeat {
    pub vehicle: i64,
    pub slot: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct CrewPassport {
    pub first_name: u16,
    pub last_name: u16,
    pub portrait: u16,
}

#[derive(Debug, Clone, Copy)]
pub struct PassportChoice {
    pub group: u16,
    pub id: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CrewMember {
    pub nation: u8,
    pub specialization: u16,
    pub role: u8,
    pub passport: CrewPassport,
    pub seat: Option<CrewSeat>,
    pub training: CrewTraining,
    pub dismissed_at: Option<u64>,
    #[serde(default)]
    pub role_change_target: Option<u16>,
    #[serde(default)]
    pub skin: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CrewTraining {
    pub efficiency_xp: u32,
    pub unspent_xp: u32,
    pub perks: Vec<u8>,
    pub bonus_perks: Vec<u8>,
    pub last_perk_level: u8,
}

impl Default for CrewTraining {
    fn default() -> Self {
        Self {
            efficiency_xp: MAX_EFFICIENCY_XP,
            unspent_xp: 0,
            perks: Vec::new(),
            bonus_perks: Vec::new(),
            last_perk_level: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CrewVehicle {
    pub inventory_id: Option<i64>,
    pub nation: u8,
    pub specialization: u16,
    pub roles: Vec<Vec<u8>>,
    pub crew_locked: bool,
    pub perks_locked: bool,
    pub premium: bool,
    pub no_transfer_penalty: bool,
    pub class_tag: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub enum CrewCommand {
    Hire {
        vehicle: i32,
        role: u8,
        course: CrewRecruitment,
    },
    HireAndSeat {
        vehicle: i64,
        slot: usize,
        course: CrewRecruitment,
    },
    Assign {
        vehicle: i64,
        slot: Option<usize>,
        member: Option<i64>,
    },
    Return(i64),
    AutoReturn {
        vehicle: i64,
        enabled: bool,
    },
    AcceleratedTraining {
        vehicle: i64,
        enabled: bool,
    },
    Dismiss(i64),
    Restore(i64),
    BuyBerths(u32),
    ConvertUndertrained,
    Train {
        member: i64,
        free_xp: u32,
    },
    Learn {
        member: i64,
        bonus: bool,
        perks: Vec<u8>,
    },
    Reset {
        member: i64,
        course: u8,
    },
    Retrain {
        member: i64,
        vehicle: i32,
        course: CrewRecruitment,
    },
    ChangeRole {
        member: i64,
        vehicle: i32,
        role: u8,
    },
    EquipSkin {
        member: i64,
        skin: Option<u16>,
    },
    ChangePassport {
        member: i64,
        first_name: Option<PassportChoice>,
        last_name: Option<PassportChoice>,
        portrait: Option<PassportChoice>,
    },
    UseBook {
        book: i32,
        count: u32,
        vehicle: Option<i64>,
        member: Option<i64>,
    },
}

impl CrewRoster {
    #[tracing::instrument(name = "accounts.crew.insert", skip_all, fields(wot.context = "accounts", input.roster.count = self.members.len()), err)]
    pub fn insert(&mut self, member: CrewMember) -> Result<i64, ProgressionError> {
        if self.members.len() >= MAX_CREW_MEMBERS {
            return Err(ProgressionError::InvalidRequest);
        }
        self.next_id = self
            .next_id
            .checked_add(1)
            .filter(|&id| id <= i64::from(i32::MAX))
            .ok_or(ProgressionError::InvalidRequest)?;
        self.members.insert(self.next_id, member);
        Ok(self.next_id)
    }

    pub fn next_passport_index(&self) -> usize {
        usize::try_from(self.next_id).unwrap_or_default()
    }

    pub fn seats(&self, vehicle: i64, slots: usize) -> Vec<Option<i64>> {
        let mut crew = vec![None; slots];
        for (&id, member) in &self.members {
            if let Some(seat) = member.seat
                && seat.vehicle == vehicle
                && let Some(slot) = crew.get_mut(seat.slot)
            {
                *slot = Some(id);
            }
        }
        crew
    }

    #[tracing::instrument(name = "accounts.crew.validate_berths", skip_all, fields(wot.context = "accounts", input.berths = berths), err)]
    pub fn validate_berths(&self, berths: i64) -> Result<(), ProgressionError> {
        let in_barracks = self
            .members
            .values()
            .filter(|member| member.seat.is_none() && member.dismissed_at.is_none())
            .count();
        if i64::try_from(in_barracks).map_or(true, |count| count > berths) {
            return Err(ProgressionError::NoFreeBerth);
        }
        Ok(())
    }

    #[tracing::instrument(name = "accounts.crew.dismiss", skip_all, fields(wot.context = "accounts", input.id = id, input.at = at), err)]
    pub fn dismiss(&mut self, id: i64, at: u64) -> Result<(), ProgressionError> {
        self.active(id)?;
        self.members.retain(|_, member| {
            member
                .dismissed_at
                .is_none_or(|dismissed| at.saturating_sub(dismissed) < CREW_RESTORE_DURATION)
        });
        let dismissed = self
            .members
            .iter()
            .filter_map(|(&id, member)| member.dismissed_at.map(|at| (at, id)));
        if dismissed.clone().count() >= CREW_RESTORE_LIMIT
            && let Some((_, oldest)) = dismissed.min()
        {
            self.members.remove(&oldest);
        }
        let member = self.active_mut(id)?;
        member.seat = None;
        member.dismissed_at = Some(at);
        Ok(())
    }

    #[tracing::instrument(name = "accounts.crew.restore", skip_all, fields(wot.context = "accounts", input.id = id, input.at = at), err)]
    pub fn restore(
        &mut self,
        id: i64,
        at: u64,
        snapshot: &mut AccountSnapshot,
    ) -> Result<(), ProgressionError> {
        let member = self
            .members
            .get_mut(&id)
            .ok_or(ProgressionError::CrewMemberNotFound)?;
        let age = at.saturating_sub(
            member
                .dismissed_at
                .ok_or(ProgressionError::CrewMemberNotFound)?,
        );
        if age >= CREW_RESTORE_DURATION {
            return Err(ProgressionError::CrewMemberNotFound);
        }
        if age >= CREW_FREE_RESTORE_DURATION {
            VehicleCost {
                credits: CREW_RESTORE_CREDITS,
                gold: 0,
            }
            .charge(snapshot)?;
        }
        member.dismissed_at = None;
        Ok(())
    }

    pub fn remaining_member_capacity(&self) -> i64 {
        MAX_CREW_MEMBERS.saturating_sub(self.members.len()) as i64
    }

    pub fn active(&self, id: i64) -> Result<&CrewMember, ProgressionError> {
        self.members
            .get(&id)
            .filter(|member| member.dismissed_at.is_none())
            .ok_or(ProgressionError::CrewMemberNotFound)
    }

    pub fn active_mut(&mut self, id: i64) -> Result<&mut CrewMember, ProgressionError> {
        self.members
            .get_mut(&id)
            .filter(|member| member.dismissed_at.is_none())
            .ok_or(ProgressionError::CrewMemberNotFound)
    }

    #[tracing::instrument(name = "accounts.crew.assign", skip_all, fields(wot.context = "accounts", input.slot = slot, input.member = member), err)]
    pub fn assign(
        &mut self,
        vehicle: &CrewVehicle,
        slot: Option<usize>,
        member: Option<i64>,
    ) -> Result<(), ProgressionError> {
        let id = vehicle
            .inventory_id
            .ok_or(ProgressionError::VehicleNotFound)?;
        if vehicle.crew_locked {
            return Err(ProgressionError::InvalidRequest);
        }
        let Some(slot) = slot else {
            if member.is_some() {
                return Err(ProgressionError::InvalidRequest);
            }
            for member in self.members.values_mut() {
                if member.seat.is_some_and(|seat| seat.vehicle == id) {
                    member.seat = None;
                }
            }
            return Ok(());
        };
        let role = vehicle
            .roles
            .get(slot)
            .and_then(|roles| roles.first())
            .ok_or(ProgressionError::InvalidRequest)?;
        if let Some(member) = member {
            let candidate = self.active(member)?;
            if candidate.nation != vehicle.nation || candidate.role != *role {
                return Err(ProgressionError::InvalidRequest);
            }
        }
        let seat = CrewSeat { vehicle: id, slot };
        for tankman in self.members.values_mut() {
            if tankman.seat == Some(seat) {
                tankman.seat = None;
            }
        }
        if let Some(member) = member {
            self.active_mut(member)?.seat = Some(seat);
            let previous = self
                .previous_crews
                .entry(id)
                .or_insert_with(|| vec![None; vehicle.roles.len()]);
            *previous
                .get_mut(slot)
                .ok_or(ProgressionError::InvalidRequest)? = Some(member);
        }
        Ok(())
    }
}

impl CrewRecruitment {
    pub fn cost(self) -> VehicleCost {
        match self {
            Self::Free => VehicleCost::default(),
            Self::Credits => VehicleCost {
                credits: 20_000,
                gold: 0,
            },
            Self::Gold => VehicleCost {
                credits: 0,
                gold: 200,
            },
        }
    }

    pub fn retraining_efficiency(self) -> u32 {
        match self {
            Self::Free => 30_000,
            Self::Credits => 60_000,
            Self::Gold => MAX_EFFICIENCY_XP,
        }
    }
}

impl AccountSnapshot {
    /// Applies an award using the crew in seat order and the training flag captured for the battle.
    #[tracing::instrument(name = "accounts.crew.award_battle_experience", skip_all, fields(wot.context = "accounts", input.vehicle_type = vehicle_type, input.accelerated_training = accelerated_training, input.combat_xp = combat_xp, input.crew_xp = crew_xp), err)]
    pub fn award_battle_experience(
        &mut self,
        vehicle_type: i32,
        battle_crew: &[Option<i64>],
        accelerated_training: bool,
        combat_xp: u32,
        crew_xp: u32,
    ) -> Result<(), ProgressionError> {
        let roster = self.crew.as_mut().ok_or(ProgressionError::InvalidRequest)?;
        let mut seen = BTreeSet::new();
        for id in battle_crew.iter().flatten().copied() {
            if !seen.insert(id) || !roster.members.contains_key(&id) {
                return Err(ProgressionError::InvalidRequest);
            }
        }
        let bonus_recipient = if accelerated_training {
            battle_crew
                .iter()
                .flatten()
                .copied()
                .filter_map(|id| {
                    let training = &roster.members.get(&id)?.training;
                    (training.xp_capacity() > 0).then_some((
                        Reverse(MAX_EFFICIENCY_XP.saturating_sub(training.efficiency_xp)),
                        training.skill_xp(),
                        id,
                    ))
                })
                .min_by_key(|&(efficiency, skill, _)| (efficiency, skill))
                .map(|(_, _, id)| id)
        } else {
            None
        };
        if bonus_recipient.is_none() && combat_xp > 0 {
            let xp = self
                .progression
                .vehicle_xp
                .get(&vehicle_type)
                .copied()
                .unwrap_or_default()
                .checked_add(i64::from(combat_xp))
                .ok_or(ProgressionError::InvalidRequest)?;
            self.progression.vehicle_xp.insert(vehicle_type, xp);
        }
        for id in seen {
            if let Some(member) = roster.members.get_mut(&id) {
                let bonus = if bonus_recipient == Some(id) {
                    combat_xp
                } else {
                    0
                };
                member.training.add_xp(crew_xp.saturating_add(bonus));
            }
        }
        Ok(())
    }

    #[tracing::instrument(name = "accounts.crew.buy_berths", skip_all, fields(wot.context = "accounts", input.packs = packs), err)]
    pub fn buy_berths(&mut self, packs: u32) -> Result<(), ProgressionError> {
        let count = i64::from(packs) * BERTH_PACK_SIZE;
        let new_total = self
            .balances
            .berths
            .checked_add(count)
            .filter(|&total| count > 0 && total <= MAX_BERTHS)
            .ok_or(ProgressionError::InvalidRequest)?;
        let paid_packs = i64::from(packs) - i64::from(self.balances.berths < BERTH_PACK_SIZE);
        VehicleCost {
            gold: paid_packs * BERTH_PACK_GOLD,
            credits: 0,
        }
        .charge(self)?;
        self.balances.berths = new_total;
        Ok(())
    }

    #[tracing::instrument(name = "accounts.crew.spend_crew_free_xp", skip_all, fields(wot.context = "accounts", input.requested = requested), err)]
    pub fn spend_crew_free_xp(&mut self, requested: u32) -> Result<u32, ProgressionError> {
        if requested == 0 || i64::from(requested) > self.balances.free_xp {
            return Err(ProgressionError::NotEnoughMoney);
        }
        let xp = requested
            .checked_mul(FREE_XP_RATE)
            .ok_or(ProgressionError::InvalidRequest)?;
        self.balances.free_xp -= i64::from(requested);
        Ok(xp)
    }
}

pub fn perk_role(perk: u8) -> Option<Option<u8>> {
    Some(match perk {
        6 | 8 | 9 => None,
        16 | 17 | 21..=26 => Some(1),
        7 | 54 | 57..=62 => Some(2),
        28..=31 | 33..=36 => Some(3),
        38..=45 => Some(4),
        46..=52 | 63 => Some(5),
        _ => return None,
    })
}

#[tracing::instrument(name = "accounts.crew.reset_perk_terms", skip_all, fields(wot.context = "accounts", input.course = course), err)]
pub fn reset_perk_terms(course: u8) -> Result<(VehicleCost, u32), ProgressionError> {
    match course {
        0 => Ok((VehicleCost::default(), 80)),
        1 => Ok((CrewRecruitment::Credits.cost(), 90)),
        2 => Ok((CrewRecruitment::Gold.cost(), 100)),
        4 => Ok((
            VehicleCost {
                credits: 100_000,
                gold: 0,
            },
            100,
        )),
        _ => Err(ProgressionError::InvalidRequest),
    }
}

impl CrewTraining {
    fn level_cost(level: u8, perk_number: usize) -> u32 {
        static COSTS: LazyLock<Vec<u32>> = LazyLock::new(|| {
            std::iter::once(0)
                .chain((0..100).scan(0, |total, index| {
                    *total += (50.0 * 100_f64.powf(f64::from(index) / 100.0)).round() as u32;
                    Some(*total)
                }))
                .collect()
        });
        COSTS[usize::from(level.min(100))] << perk_number.min(MAX_MAJOR_PERKS)
    }

    fn max_skill_xp() -> u32 {
        Self::level_cost(100, 1) * 63
    }

    pub fn skill_xp(&self) -> u32 {
        self.allocated_xp().saturating_add(self.unspent_xp)
    }

    pub fn full_perks(&self) -> usize {
        let xp = self.skill_xp();
        (1..=MAX_MAJOR_PERKS)
            .take_while(|&count| xp >= Self::level_cost(100, 1) * ((1 << count) - 1))
            .count()
    }

    pub fn xp_capacity(&self) -> u32 {
        MAX_EFFICIENCY_XP.saturating_sub(self.efficiency_xp)
            + Self::max_skill_xp().saturating_sub(self.skill_xp())
    }

    fn level_up(&mut self) {
        let count = self.perks.len();
        if count == 0 {
            return;
        }
        for level in self.last_perk_level..100 {
            let cost = Self::level_cost(level + 1, count) - Self::level_cost(level, count);
            if cost > self.unspent_xp {
                break;
            }
            self.unspent_xp -= cost;
            self.last_perk_level = level + 1;
        }
    }

    pub fn add_xp(&mut self, xp: u32) {
        let xp = xp.min(self.xp_capacity());
        let efficiency = xp.min(MAX_EFFICIENCY_XP.saturating_sub(self.efficiency_xp));
        self.efficiency_xp += efficiency;
        self.unspent_xp += xp - efficiency;
        self.level_up();
    }

    #[tracing::instrument(name = "accounts.crew.learn", skip_all, fields(wot.context = "accounts", input.role = role, input.bonus = bonus), err)]
    pub fn learn(
        &mut self,
        role: u8,
        secondary_roles: &[u8],
        bonus: bool,
        perks: &[u8],
    ) -> Result<(), ProgressionError> {
        if perks.is_empty() || perks.len() > MAX_MAJOR_PERKS {
            return Err(ProgressionError::InvalidRequest);
        }
        for &perk in perks {
            let skill_role = perk_role(perk).ok_or(ProgressionError::InvalidRequest)?;
            if bonus {
                let skill_role = skill_role
                    .filter(|&skill_role| secondary_roles.contains(&skill_role))
                    .ok_or(ProgressionError::InvalidRequest)?;
                let available = (self.full_perks() / 2 + 1).min(3);
                let selected = self
                    .bonus_perks
                    .iter()
                    .filter(|&&p| perk_role(p) == Some(Some(skill_role)))
                    .count();
                if selected >= available || self.bonus_perks.contains(&perk) {
                    return Err(ProgressionError::InvalidRequest);
                }
                self.bonus_perks.push(perk);
            } else {
                if skill_role.is_some_and(|skill_role| skill_role != role)
                    || self.perks.contains(&perk)
                    || self.perks.len() >= MAX_MAJOR_PERKS
                    || (!self.perks.is_empty() && self.last_perk_level != 100)
                    || self.efficiency_xp != MAX_EFFICIENCY_XP
                {
                    return Err(ProgressionError::InvalidRequest);
                }
                self.perks.push(perk);
                self.last_perk_level = 0;
                self.level_up();
            }
        }
        Ok(())
    }

    fn allocated_xp(&self) -> u32 {
        let count = self.perks.len();
        (1..count)
            .map(|index| Self::level_cost(100, index))
            .sum::<u32>()
            + Self::level_cost(self.last_perk_level, count)
    }

    pub fn change_role(&mut self, role: u8) {
        let xp = self.skill_xp();
        let last_stays = self
            .perks
            .last()
            .and_then(|&perk| perk_role(perk))
            .is_some_and(|perk_role| perk_role.is_none_or(|perk_role| perk_role == role));
        self.perks.retain(|&perk| {
            perk_role(perk)
                .is_some_and(|perk_role| perk_role.is_none_or(|perk_role| perk_role == role))
        });
        self.bonus_perks.clear();
        if self.perks.is_empty() {
            self.last_perk_level = 0;
        } else if !last_stays {
            self.last_perk_level = 100;
        }
        self.unspent_xp = xp.saturating_sub(self.allocated_xp());
        self.level_up();
    }

    #[tracing::instrument(name = "accounts.crew.reset", skip_all, fields(wot.context = "accounts", input.reuse_percent = reuse_percent), err)]
    pub fn reset(&mut self, reuse_percent: u32) -> Result<(), ProgressionError> {
        if self.perks.is_empty() && self.bonus_perks.is_empty() {
            return Err(ProgressionError::InvalidRequest);
        }
        let refund = self.allocated_xp() * reuse_percent.min(100) / 100;
        self.unspent_xp += refund;
        self.perks.clear();
        self.bonus_perks.clear();
        self.last_perk_level = 0;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn battle_account() -> AccountSnapshot {
        let mut crew = CrewRoster::default();
        for _ in 0..3 {
            crew.insert(CrewMember {
                nation: 0,
                specialization: 111,
                role: 5,
                passport: CrewPassport {
                    first_name: 0,
                    last_name: 0,
                    portrait: 0,
                },
                seat: None,
                training: CrewTraining::default(),
                dismissed_at: None,
                role_change_target: None,
                skin: None,
            })
            .unwrap();
        }
        AccountSnapshot {
            id: 1,
            display_name: String::new(),
            revision: 1,
            balances: Default::default(),
            crew: Some(crew),
            vehicles: Vec::new(),
            depot_shells: Default::default(),
            progression: Default::default(),
            settings: Default::default(),
        }
    }

    #[test]
    fn accelerated_combat_xp_follows_the_battle_crew_and_uses_seat_order_for_ties() {
        let mut account = battle_account();
        let seats = [Some(2), None, Some(1)];
        account.progression.vehicle_xp.insert(1, 900);
        account
            .award_battle_experience(1, &seats, true, 100, 100)
            .unwrap();
        let crew = account.crew.as_ref().unwrap();
        assert_eq!(crew.members[&2].training.skill_xp(), 200);
        assert_eq!(crew.members[&1].training.skill_xp(), 100);
        assert_eq!(crew.members[&3].training.skill_xp(), 0);
        assert_eq!(account.progression.vehicle_xp[&1], 900);

        let crew = account.crew.as_mut().unwrap();
        crew.members.get_mut(&2).unwrap().training.efficiency_xp = MAX_EFFICIENCY_XP - 50;
        account
            .award_battle_experience(1, &seats, true, 100, 100)
            .unwrap();
        let crew = account.crew.as_ref().unwrap();
        assert_eq!(crew.members[&2].training.efficiency_xp, MAX_EFFICIENCY_XP);
        assert_eq!(crew.members[&2].training.skill_xp(), 350);
        assert_eq!(crew.members[&1].training.skill_xp(), 200);

        account
            .award_battle_experience(1, &seats, true, 100, 100)
            .unwrap();
        let crew = account.crew.as_ref().unwrap();
        assert_eq!(crew.members[&2].training.skill_xp(), 450);
        assert_eq!(crew.members[&1].training.skill_xp(), 400);
    }

    #[test]
    fn combat_xp_accrues_when_disabled_or_without_a_trainable_recipient() {
        let mut account = battle_account();
        let seats = [Some(1)];
        account
            .award_battle_experience(1, &seats, false, 70, 100)
            .unwrap();
        let crew = account.crew.as_mut().unwrap();
        let training = &mut crew.members.get_mut(&1).unwrap().training;
        assert_eq!(training.skill_xp(), 100);
        training.add_xp(u32::MAX);
        account
            .award_battle_experience(1, &seats, true, 50, 100)
            .unwrap();
        assert_eq!(account.progression.vehicle_xp[&1], 120);
    }

    #[test]
    fn invalid_battle_crew_cannot_partially_award_xp() {
        let mut account = battle_account();
        let before = account.crew.clone();
        for seats in [[Some(1), Some(1)], [Some(1), Some(99)]] {
            assert_eq!(
                account.award_battle_experience(1, &seats, true, 100, 100),
                Err(ProgressionError::InvalidRequest)
            );
        }
        assert_eq!(account.crew, before);
        assert!(account.progression.vehicle_xp.is_empty());
    }
}
