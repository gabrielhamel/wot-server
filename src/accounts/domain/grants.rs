use super::Balances;

pub const MAX_GRANTED_BALANCE: i64 = i32::MAX as i64;

#[derive(Debug, Clone, Copy)]
pub enum GrantCurrency {
    Credits,
    Gold,
    Bonds,
    FreeXp,
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
    #[error("Select an attachment from the installed catalog.")]
    UnknownAttachment,
    #[error("The grant would exceed the attachment ownership limit.")]
    AttachmentLimit,
    #[error("Select a compatible stat tracker and vehicle from the player garage.")]
    IncompatibleStatTracker,
    #[error("The grant would exceed the stat tracker ownership limit.")]
    StatTrackerLimit,
    #[error(transparent)]
    Vehicle(#[from] super::ProgressionError),
}

impl GrantCurrency {
    pub fn balance(self, balances: &Balances) -> i64 {
        match self {
            Self::Credits => balances.credits,
            Self::Gold => balances.gold,
            Self::Bonds => balances.bonds,
            Self::FreeXp => balances.free_xp,
        }
    }

    #[tracing::instrument(name = "accounts.grants.grant", skip_all, fields(ddd.bounded_context.name = "accounts", input.amount = amount), err)]
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
        }
        Ok(())
    }
}
