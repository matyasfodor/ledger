use std::{error::Error, io};

use serde::Deserialize;

use crate::amount::{Amount, deserialize_optional_amount};
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
    client: u32,
    tx: u32,
    #[serde(deserialize_with = "deserialize_optional_amount")]
    amount: Option<Amount>,
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

pub fn parse_csv(reader: impl io::Read) -> Result<Vec<Transaction>, Box<dyn Error>> {
    let mut rdr = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(reader);
    let mut transactions: Vec<Transaction> = Vec::new();
    for result in rdr.deserialize() {
        let record: Record = result?;
        transactions.push(record.try_into()?);
    }
    Ok(transactions)
}
