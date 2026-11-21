#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AccountId(i64);

impl AccountId {
    pub(crate) fn new(value: i64) -> Option<Self> {
        (value > 0).then_some(Self(value))
    }

    pub(crate) fn get(self) -> i64 {
        self.0
    }
}

pub(super) mod vehicle_performance;
