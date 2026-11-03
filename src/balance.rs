use crate::amount::{PositiveAmount, SignedAmount};

pub struct Balance {
    pub available: SignedAmount,
    pub held: PositiveAmount,
    pub locked: bool,
}
