use crate::amount::Amount;

pub struct Balance {
    pub available: Amount<i64>,
    pub held: Amount<u32>,
    pub locked: bool,
}
