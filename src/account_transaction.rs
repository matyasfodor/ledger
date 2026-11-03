use crate::amount::PositiveAmount;
use crate::id::ClientId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepositState {
    Posted,
    Disputed,
    ChargedBack,
}

/// An accepted deposit or withdrawal. A withdrawal has no dispute state; it is
/// kept so a later row cannot reuse its transaction id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountTransaction {
    Deposit {
        client: ClientId,
        amount: PositiveAmount,
        state: DepositState,
    },
    Withdrawal,
}
