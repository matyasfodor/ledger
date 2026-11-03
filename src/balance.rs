use crate::amount::{PositiveAmount, SignedAmount};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Balance {
    pub available: SignedAmount,
    pub held: PositiveAmount,
    pub locked: bool,
}

impl Balance {
    pub(crate) fn zero() -> Self {
        Self {
            available: SignedAmount::from_minor(0),
            held: PositiveAmount::from_minor(0),
            locked: false,
        }
    }
}
