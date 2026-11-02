use crate::amount::Amount;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepositState {
    Posted,
    Disputed,
    ChargedBack,
}

/// A posted deposit kept so a later dispute can find its amount and client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deposit {
    pub client: u32,
    pub amount: Amount,
    pub state: DepositState,
}
