use crate::amount::PositiveAmount;
use crate::id::{ClientId, TxId};

#[derive(Debug)]
pub enum Transaction {
    Deposit {
        client: ClientId,
        tx: TxId,
        amount: PositiveAmount,
    },
    Withdrawal {
        client: ClientId,
        tx: TxId,
        amount: PositiveAmount,
    },
    Chargeback {
        client: ClientId,
        tx: TxId,
    },
    Dispute {
        client: ClientId,
        tx: TxId,
    },
    Resolve {
        client: ClientId,
        tx: TxId,
    },
}

impl Transaction {
    pub fn client(&self) -> ClientId {
        match self {
            Transaction::Deposit { client, .. } => *client,
            Transaction::Withdrawal { client, .. } => *client,
            Transaction::Chargeback { client, .. } => *client,
            Transaction::Dispute { client, .. } => *client,
            Transaction::Resolve { client, .. } => *client,
        }
    }

    pub fn tx(&self) -> TxId {
        match self {
            Transaction::Deposit { tx, .. } => *tx,
            Transaction::Withdrawal { tx, .. } => *tx,
            Transaction::Chargeback { tx, .. } => *tx,
            Transaction::Dispute { tx, .. } => *tx,
            Transaction::Resolve { tx, .. } => *tx,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Transaction::Deposit { .. } => "deposit",
            Transaction::Withdrawal { .. } => "withdrawal",
            Transaction::Dispute { .. } => "dispute",
            Transaction::Resolve { .. } => "resolve",
            Transaction::Chargeback { .. } => "chargeback",
        }
    }

    pub fn amount(&self) -> Option<PositiveAmount> {
        match self {
            Transaction::Deposit { amount, .. } | Transaction::Withdrawal { amount, .. } => {
                Some(*amount)
            }
            Transaction::Chargeback { .. }
            | Transaction::Dispute { .. }
            | Transaction::Resolve { .. } => None,
        }
    }
}
