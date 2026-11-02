use crate::amount::Amount;

pub struct Balance {
    pub available: Amount,
    pub held: Amount,
    pub locked: bool,
}
