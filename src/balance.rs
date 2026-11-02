use crate::amount::Amount;

pub struct Balance {
    pub available: Amount,
    pub held: Amount,
    pub total: Amount,
    pub locked: bool,
}
