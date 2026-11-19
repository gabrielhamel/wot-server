use super::{AccountSnapshot, EquipmentPrice, ProgressionError};

pub(in crate::accounts) fn unix_now() -> u64 {
    std::time::UNIX_EPOCH
        .elapsed()
        .map_or(0, |duration| duration.as_secs())
}

pub(in crate::accounts) const PREMIUM_PACKAGES: [(i32, i64); 8] = [
    (1, 250),
    (3, 650),
    (7, 1_250),
    (14, 1_800),
    (30, 2_500),
    (90, 6_900),
    (180, 11_900),
    (360, 20_500),
];

impl AccountSnapshot {
    pub fn expired_premium_at(&self) -> Option<u32> {
        let expiry = self.progression.premium_expires_at;
        (expiry != 0 && u64::from(expiry) <= unix_now()).then_some(expiry)
    }

    pub fn active_premium_until(&self) -> Option<u32> {
        let expiry = self.progression.premium_expires_at;
        (u64::from(expiry) > unix_now()).then_some(expiry)
    }

    #[tracing::instrument(name = "accounts.premium.extend", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = self.id, input.duration.days = days, input.price.gold = tracing::field::Empty, output.premium.expires_at = tracing::field::Empty), err)]
    pub(in crate::accounts) fn extend_premium(
        &mut self,
        days: i32,
        now: u64,
    ) -> Result<(), ProgressionError> {
        let (_, gold) = PREMIUM_PACKAGES
            .into_iter()
            .find(|&(length, _)| length == days)
            .ok_or(ProgressionError::InvalidRequest)?;
        tracing::Span::current().record("input.price.gold", gold);
        let duration = u64::try_from(days)
            .map_err(|_| ProgressionError::InvalidRequest)?
            .checked_mul(86_400)
            .ok_or(ProgressionError::InvalidRequest)?;
        let expires_at = u64::from(self.progression.premium_expires_at)
            .max(now)
            .checked_add(duration)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or(ProgressionError::InvalidRequest)?;
        EquipmentPrice::Gold(gold).charge(&mut self.balances, 1)?;
        self.progression.premium_expires_at = expires_at;
        tracing::Span::current().record("output.premium.expires_at", expires_at);
        Ok(())
    }
}
