use crate::amount::Amount;

#[derive(Debug)]
pub enum Transaction {
    Deposit {
        client: u32,
        tx: u32,
        amount: Amount<u32>,
    },
    Withdrawal {
        client: u32,
        tx: u32,
        amount: Amount<u32>,
    },
    Chargeback {
        client: u32,
        tx: u32,
    },
    Dispute {
        client: u32,
        tx: u32,
    },
    Resolve {
        client: u32,
        tx: u32,
    },
}

impl Transaction {
    pub fn client(&self) -> u32 {
        match self {
            Transaction::Deposit { client, .. } => *client,
            Transaction::Withdrawal { client, .. } => *client,
            Transaction::Chargeback { client, .. } => *client,
            Transaction::Dispute { client, .. } => *client,
            Transaction::Resolve { client, .. } => *client,
        }
    }
}
