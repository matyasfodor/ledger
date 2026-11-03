use std::{error::Error, io};

use serde::Deserialize;

use crate::amount::{PositiveAmount, deserialize_optional_amount};
use crate::id::{ClientId, TxId};
use crate::transaction::Transaction;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum TxType {
    #[serde(rename = "deposit")]
    Deposit,
    #[serde(rename = "withdrawal")]
    Withdrawal,
    #[serde(rename = "chargeback")]
    Chargeback,
    #[serde(rename = "dispute")]
    Dispute,
    #[serde(rename = "resolve")]
    Resolve,
}

#[derive(Debug, Deserialize)]
struct Record {
    #[serde(rename = "type")]
    tx_type: TxType,
    client: ClientId,
    tx: TxId,
    #[serde(deserialize_with = "deserialize_optional_amount")]
    amount: Option<PositiveAmount>,
}

impl TryFrom<Record> for Transaction {
    type Error = Box<dyn Error>;

    fn try_from(record: Record) -> Result<Self, Self::Error> {
        match record.tx_type {
            TxType::Deposit => Ok(Transaction::Deposit {
                client: record.client,
                tx: record.tx,
                amount: record.amount.ok_or("Amount is required for deposit")?,
            }),
            TxType::Withdrawal => Ok(Transaction::Withdrawal {
                client: record.client,
                tx: record.tx,
                amount: record.amount.ok_or("Amount is required for withdrawal")?,
            }),
            TxType::Chargeback => Ok(Transaction::Chargeback {
                client: record.client,
                tx: record.tx,
            }),
            TxType::Dispute => Ok(Transaction::Dispute {
                client: record.client,
                tx: record.tx,
            }),
            TxType::Resolve => Ok(Transaction::Resolve {
                client: record.client,
                tx: record.tx,
            }),
        }
    }
}

pub fn parse_csv(
    reader: impl io::Read,
) -> impl Iterator<Item = Result<Transaction, Box<dyn Error>>> {
    csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(reader)
        .into_deserialize::<Record>()
        .map(|result| {
            let record: Record = result?;
            record.try_into()
        })
}
