use super::{Balances, ProgressionState};

pub const MAX_GRANTED_BALANCE: i64 = i32::MAX as i64;

#[derive(Debug, Clone, Copy)]
pub enum GrantCurrency {
    Credits,
    Gold,
    Bonds,
    FreeXp,
    EquipmentComponents,
}

#[derive(Debug, Clone, Copy)]
pub enum PlayerGrant {
    Currency {
        currency: GrantCurrency,
        amount: i64,
    },
    Vehicle {
        vehicle: i32,
    },
    StatTracker {
        vehicle: i64,
        id: u16,
        count: u32,
    },
    CrewBook {
        id: i32,
        count: u32,
    },
    ImprovedEquipment {
        id: i32,
        count: u32,
    },
    BountyEquipment {
        id: i32,
        count: u32,
    },
    ExperimentalEquipment {
        id: i32,
        count: u32,
    },
    Style2d {
        id: u16,
        count: u32,
    },
    Style3d {
        id: u16,
        count: u32,
    },
    CrewSkin {
        id: u16,
        count: u32,
    },
    SpecialRecruit {
        id: i32,
    },
    Attachment {
        id: u16,
        count: u32,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum GrantError {
    #[error("Enter a positive whole amount.")]
    InvalidAmount,
    #[error("The grant would exceed the supported balance limit.")]
    BalanceLimit,
    #[error("Select a style of the requested type from the installed catalog.")]
    UnknownStyle,
    #[error("The grant would exceed the style ownership limit.")]
    StyleLimit,
    #[error("Select a crew skin from the installed catalog.")]
    UnknownCrewSkin,
    #[error("The grant would exceed the crew skin ownership limit.")]
    CrewSkinLimit,
    #[error("Select a special recruit from the installed catalog.")]
    UnknownSpecialRecruit,
    #[error("The player already has this special recruit.")]
    SpecialRecruitOwned,
    #[error("No compatible specialization is available for this recruit.")]
    SpecialRecruitVehicle,
    #[error("Select an attachment from the installed catalog.")]
    UnknownAttachment,
    #[error("The grant would exceed the attachment ownership limit.")]
    AttachmentLimit,
    #[error("Select a compatible stat tracker and vehicle from the player garage.")]
    IncompatibleStatTracker,
    #[error("The grant would exceed the stat tracker ownership limit.")]
    StatTrackerLimit,
    #[error("Select a crew book from the installed catalog.")]
    UnknownCrewBook,
    #[error("Select Bounty Equipment from the installed catalog.")]
    UnknownBountyEquipment,
    #[error("Select Improved Equipment from the installed catalog.")]
    UnknownImprovedEquipment,
    #[error("Select Experimental Equipment from the installed catalog.")]
    UnknownExperimentalEquipment,
    #[error("Enter a whole number of copies from 1 to 100.")]
    InvalidDepotCount,
    #[error("The grant would exceed the supported depot stock limit.")]
    DepotLimit,
    #[error(transparent)]
    Vehicle(#[from] super::ProgressionError),
}

impl GrantCurrency {
    pub(crate) fn balance(self, balances: &Balances) -> i64 {
        match self {
            Self::Credits => balances.credits,
            Self::Gold => balances.gold,
            Self::Bonds => balances.bonds,
            Self::FreeXp => balances.free_xp,
            Self::EquipmentComponents => balances.equipment_components,
        }
    }

    #[tracing::instrument(name = "accounts.grants.grant", skip_all, fields(ddd.bounded_context.name = "accounts", input.currency = ?self, input.amount = amount, output.balance = tracing::field::Empty), err)]
    pub(crate) fn grant(self, balances: &mut Balances, amount: i64) -> Result<(), GrantError> {
        if amount <= 0 {
            return Err(GrantError::InvalidAmount);
        }
        let total = self
            .balance(balances)
            .checked_add(amount)
            .filter(|&total| total <= MAX_GRANTED_BALANCE)
            .ok_or(GrantError::BalanceLimit)?;
        match self {
            Self::Credits => balances.credits = total,
            Self::Gold => balances.gold = total,
            Self::Bonds => balances.bonds = total,
            Self::FreeXp => balances.free_xp = total,
            Self::EquipmentComponents => balances.equipment_components = total,
        }
        tracing::Span::current().record("output.balance", total);
        Ok(())
    }
}

impl ProgressionState {
    #[tracing::instrument(name = "accounts.grants.grant_depot_stock", skip_all, fields(ddd.bounded_context.name = "accounts", input.item = item, input.count = count, output.owned.count = tracing::field::Empty), err)]
    pub(crate) fn grant_depot_stock(&mut self, item: i32, count: u32) -> Result<(), GrantError> {
        if !(1..=100).contains(&count) {
            return Err(GrantError::InvalidDepotCount);
        }
        let stock = self.depot.get(&item).copied().unwrap_or_default();
        let total = stock
            .checked_add(count)
            .filter(|&total| total <= i32::MAX as u32)
            .ok_or(GrantError::DepotLimit)?;
        self.depot.insert(item, total);
        tracing::Span::current().record("output.owned.count", total);
        Ok(())
    }
}
