use std::fmt;

/// Why one input row was not applied. Parse failures and business rejects use the same type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skip {
    UnknownType,
    MissingAmount,
    InvalidClient,
    InvalidTx,
    InvalidAmount { input: String },
    BrokenRecord { message: String },
    Duplicate,
    Locked,
    InsufficientFunds,
    UnknownTransaction,
    WrongClient,
    NotADeposit,
    AlreadyDisputed,
    NotDisputed,
    ChargedBack,
    ZeroAmount,
    Overflow,
    NegativeHeld,
    MissingBalance,
}

/// A skipped row together with its 1-based line in the input file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowFailure {
    pub line: u64,
    pub skip: Skip,
    /// Raw type, client, tx, and amount when the row could be read.
    pub detail: String,
}

impl fmt::Display for Skip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Skip::UnknownType => write!(f, "unknown transaction type"),
            Skip::MissingAmount => write!(f, "amount is required"),
            Skip::InvalidClient => write!(f, "invalid client id"),
            Skip::InvalidTx => write!(f, "invalid transaction id"),
            Skip::InvalidAmount { input } => write!(f, "invalid amount '{input}'"),
            Skip::BrokenRecord { message } => write!(f, "broken record: {message}"),
            Skip::Duplicate => write!(f, "duplicate transaction id"),
            Skip::Locked => write!(f, "account is locked"),
            Skip::InsufficientFunds => write!(f, "insufficient available funds"),
            Skip::UnknownTransaction => write!(f, "referenced transaction does not exist"),
            Skip::WrongClient => write!(f, "referenced transaction belongs to another client"),
            Skip::NotADeposit => write!(f, "only deposits can be disputed"),
            Skip::AlreadyDisputed => write!(f, "transaction is already under dispute"),
            Skip::NotDisputed => write!(f, "transaction is not under dispute"),
            Skip::ChargedBack => write!(f, "transaction was already charged back"),
            Skip::ZeroAmount => write!(f, "amount must be positive"),
            Skip::Overflow => write!(f, "amount overflow"),
            Skip::NegativeHeld => write!(f, "held funds would become negative"),
            Skip::MissingBalance => write!(f, "client balance is missing"),
        }
    }
}

impl fmt::Display for RowFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.detail.is_empty() {
            write!(f, "line {}: {}", self.line, self.skip)
        } else {
            write!(f, "line {}: {}, {}", self.line, self.skip, self.detail)
        }
    }
}

impl std::error::Error for Skip {}

impl std::error::Error for RowFailure {}
