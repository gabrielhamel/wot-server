pub(super) mod battle_outfits;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccountId(i64);

impl AccountId {
    pub fn new(value: i64) -> Option<Self> {
        (value > 0).then_some(Self(value))
    }

    pub fn get(self) -> i64 {
        self.0
    }
}

pub(super) mod vehicle_performance;
