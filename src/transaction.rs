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
}
