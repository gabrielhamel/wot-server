use super::*;
use crate::accounts::{
    CrewCommand, CrewRecruitment, MAX_MAJOR_PERKS, PassportChoice, ProgressionCommand,
};

pub(super) const MAX_QUEUED_CREW_COMMANDS: usize = 32;
const MAX_CREW_GROUP_SIZE: i32 = 32;
const ACCELERATED_TRAINING_FLAG: i64 = 1;
const AUTO_RETURN_FLAG: i64 = 512;

const ASSIGN: i16 = 105;
const VEHICLE_SETTINGS: i16 = 107;
const LEARN_PERK: i16 = 151;
const RESET_PERKS: i16 = 152;
const RETRAIN: i16 = 153;
const PASSPORT: i16 = 154;
const TRAIN_WITH_FREE_XP: i16 = 155;
const LEARN_PERKS: i16 = 156;
const RETURN_CREW: i16 = 157;
const CHANGE_ROLE: i16 = 158;
const EQUIP_SKIN: i16 = 160;
const UNEQUIP_SKIN: i16 = 161;
const USE_BOOK: i16 = 162;
const CONVERT_UNDERTRAINED: i16 = 165;
const BUY_BERTHS: i16 = 206;
const HIRE: i16 = 303;
const DISMISS: i16 = 306;
const HIRE_AND_SEAT: i16 = 309;
const RESTORE: i16 = 10015;

pub(super) struct QueuedCrewCommand {
    pub parent: tracing::Span,
    pub request_id: i16,
    pub command: ProgressionCommand,
    pub shop_revision: Option<i64>,
}

impl Gateway {
    pub(super) fn crew_command(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
        request: &AccountBase,
    ) -> Result<bool> {
        let (request_id, command, shop_revision) = match request {
            AccountBase::DoCmdInt {
                request_id,
                cmd: CONVERT_UNDERTRAINED,
                arg,
            } => (
                *request_id,
                (*arg == 0).then_some(CrewCommand::ConvertUndertrained),
                None,
            ),
            AccountBase::DoCmdInt3 {
                request_id,
                cmd,
                arg0,
                arg1,
                arg2,
            } => match *cmd {
                LEARN_PERK => (*request_id, learn(*arg0, *arg1, &[*arg2]), None),
                RESET_PERKS => (
                    *request_id,
                    u8::try_from(*arg2)
                        .ok()
                        .filter(|&course| course <= 4)
                        .map(|course| CrewCommand::Reset {
                            member: *arg1,
                            course,
                        }),
                    Some(*arg0),
                ),
                RETURN_CREW => (
                    *request_id,
                    (*arg1 == 0 && *arg2 == 0).then_some(CrewCommand::Return(*arg0)),
                    None,
                ),
                EQUIP_SKIN => (
                    *request_id,
                    u16::try_from(*arg1)
                        .ok()
                        .filter(|&skin| skin > 0 && *arg2 == 0)
                        .map(|skin| CrewCommand::EquipSkin {
                            member: *arg0,
                            skin: Some(skin),
                        }),
                    None,
                ),
                UNEQUIP_SKIN => (
                    *request_id,
                    (*arg1 == 0 && *arg2 == 0).then_some(CrewCommand::EquipSkin {
                        member: *arg0,
                        skin: None,
                    }),
                    None,
                ),
                BUY_BERTHS => (
                    *request_id,
                    u32::try_from(*arg1)
                        .ok()
                        .filter(|_| *arg2 == 0)
                        .map(CrewCommand::BuyBerths),
                    Some(*arg0),
                ),
                DISMISS => (
                    *request_id,
                    (*arg1 == 0 && *arg2 == 0).then_some(CrewCommand::Dismiss(*arg0)),
                    None,
                ),
                _ => return Ok(false),
            },
            AccountBase::DoCmdInt4 {
                request_id,
                cmd,
                arg0,
                arg1,
                arg2,
                arg3,
            } => match *cmd {
                HIRE => (*request_id, hire(*arg1, *arg2, *arg3), Some(*arg0)),
                HIRE_AND_SEAT => (*request_id, hire_and_seat(*arg1, *arg2, *arg3), Some(*arg0)),
                VEHICLE_SETTINGS if *arg1 == ACCELERATED_TRAINING_FLAG => (
                    *request_id,
                    match arg2 {
                        0 | 1 => Some(CrewCommand::AcceleratedTraining {
                            vehicle: *arg0,
                            enabled: *arg2 == 1,
                        }),
                        _ => None,
                    },
                    None,
                ),
                VEHICLE_SETTINGS if *arg1 == AUTO_RETURN_FLAG => (
                    *request_id,
                    match arg2 {
                        0 | 1 => Some(CrewCommand::AutoReturn {
                            vehicle: *arg0,
                            enabled: *arg2 == 1,
                        }),
                        _ => None,
                    },
                    None,
                ),
                _ => return Ok(false),
            },
            AccountBase::DoCmdIntArr {
                request_id,
                cmd,
                args,
            } => {
                let (command, shop_revision) = match *cmd {
                    ASSIGN | RETRAIN | PASSPORT | TRAIN_WITH_FREE_XP | LEARN_PERKS
                    | CHANGE_ROLE | USE_BOOK | RESTORE => crew_array(*cmd, args),
                    _ => return Ok(false),
                };
                (*request_id, command, shop_revision)
            }
            _ => return Ok(false),
        };
        self.change_progression(
            client,
            entity,
            request_id,
            command.map(ProgressionCommand::Crew),
            shop_revision,
        )?;
        Ok(true)
    }
}

fn course(index: i64) -> Option<CrewRecruitment> {
    match index {
        0 => Some(CrewRecruitment::Free),
        1 => Some(CrewRecruitment::Credits),
        2 => Some(CrewRecruitment::Gold),
        _ => None,
    }
}

fn hire(vehicle: i64, role: i64, index: i64) -> Option<CrewCommand> {
    Some(CrewCommand::Hire {
        vehicle: i32::try_from(vehicle).ok()?,
        role: u8::try_from(role).ok()?,
        course: course(index)?,
    })
}

fn hire_and_seat(vehicle: i64, slot: i64, index: i64) -> Option<CrewCommand> {
    Some(CrewCommand::HireAndSeat {
        vehicle,
        slot: usize::try_from(slot).ok()?,
        course: course(index)?,
    })
}

fn learn(member: i64, utilization: i64, perks: &[i64]) -> Option<CrewCommand> {
    if !matches!(utilization, 1 | 2) || !(1..=MAX_MAJOR_PERKS).contains(&perks.len()) {
        return None;
    }
    let perks = perks
        .iter()
        .map(|&perk| u8::try_from(perk).ok())
        .collect::<Option<_>>()?;
    Some(CrewCommand::Learn {
        member,
        bonus: utilization == 2,
        perks,
    })
}

fn optional_id(id: i32) -> Option<Option<i64>> {
    match id {
        -1 => Some(None),
        1.. => Some(Some(i64::from(id))),
        _ => None,
    }
}

fn passport_choice(group: i32, id: i32) -> Option<Option<PassportChoice>> {
    if id == -1 {
        return Some(None);
    }
    Some(Some(PassportChoice {
        group: u16::try_from(group).ok()?,
        id: u16::try_from(id).ok()?,
    }))
}

fn crew_array(cmd: i16, args: &[i32]) -> (Option<CrewCommand>, Option<i64>) {
    let shop_revision = matches!(cmd, RETRAIN | PASSPORT | TRAIN_WITH_FREE_XP | CHANGE_ROLE)
        .then(|| args.first().copied().map(i64::from))
        .flatten();
    let command = (|| {
        Some(match (cmd, args) {
            (ASSIGN, &[vehicle, slot, member, _, 1..=MAX_CREW_GROUP_SIZE]) => CrewCommand::Assign {
                vehicle: vehicle.into(),
                slot: if slot == -1 {
                    None
                } else {
                    Some(usize::try_from(slot).ok()?)
                },
                member: optional_id(member)?,
            },
            (RETRAIN, &[_, member, index, vehicle, _, 1..=MAX_CREW_GROUP_SIZE]) => {
                CrewCommand::Retrain {
                    member: member.into(),
                    vehicle,
                    course: course(index.into())?,
                }
            }
            (
                PASSPORT,
                &[
                    _,
                    member,
                    0,
                    -1 | 0,
                    first_group,
                    first,
                    last_group,
                    last,
                    portrait_group,
                    portrait,
                ],
            ) => CrewCommand::ChangePassport {
                member: member.into(),
                first_name: passport_choice(first_group, first)?,
                last_name: passport_choice(last_group, last)?,
                portrait: passport_choice(portrait_group, portrait)?,
            },
            (TRAIN_WITH_FREE_XP, &[_, member, free_xp, _, 1..=MAX_CREW_GROUP_SIZE]) => {
                CrewCommand::Train {
                    member: member.into(),
                    free_xp: u32::try_from(free_xp).ok()?,
                }
            }
            (LEARN_PERKS, &[member, utilization, ref perks @ ..])
                if perks.len() <= MAX_MAJOR_PERKS =>
            {
                let perks: Vec<_> = perks.iter().copied().map(i64::from).collect();
                return learn(member.into(), utilization.into(), &perks);
            }
            (CHANGE_ROLE, &[_, member, role, vehicle, _, 1..=MAX_CREW_GROUP_SIZE]) => {
                CrewCommand::ChangeRole {
                    member: member.into(),
                    vehicle,
                    role: u8::try_from(role).ok()?,
                }
            }
            (USE_BOOK, &[book, count, vehicle, member, _, 1..=MAX_CREW_GROUP_SIZE]) => {
                CrewCommand::UseBook {
                    book,
                    count: u32::try_from(count).ok()?,
                    vehicle: optional_id(vehicle)?,
                    member: optional_id(member)?,
                }
            }
            (RESTORE, &[member, _, 1..=MAX_CREW_GROUP_SIZE]) => CrewCommand::Restore(member.into()),
            _ => return None,
        })
    })();
    (command, shop_revision)
}
