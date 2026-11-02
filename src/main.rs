use std::{collections::HashMap, error::Error, io, process};

use ledger::amount::{Amount, deserialize_optional_amount};
use serde::Deserialize;

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

#[derive(Debug)]
enum Transaction {
    Deposit {
        client: u32,
        tx: u32,
        amount: Amount,
    },
    Withdrawal {
        client: u32,
        tx: u32,
        amount: Amount,
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

impl Transaction {
    fn client(&self) -> u32 {
        match self {
            Transaction::Deposit { client, .. } => *client,
            Transaction::Withdrawal { client, .. } => *client,
            Transaction::Chargeback { client, .. } => *client,
            Transaction::Dispute { client, .. } => *client,
            Transaction::Resolve { client, .. } => *client,
        }
    }
}

struct Balance {
    available: Amount,
    held: Amount,
    total: Amount,
    locked: bool,
}

fn parse_csv() -> Result<Vec<Transaction>, Box<dyn Error>> {
    let mut rdr = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(io::stdin());
    let mut transactions: Vec<Transaction> = Vec::new();
    for result in rdr.deserialize() {
        let record: Record = result?;
        transactions.push(record.try_into()?);
    }
    Ok(transactions)
}

fn calculate_balances() -> Result<(), Box<dyn Error>> {
    let mut writer = csv::WriterBuilder::new().from_writer(io::stdout());
    // Write headers
    writer.write_record(&["client", "available", "held", "total", "locked"])?;
    let transactions = parse_csv()?;
    let mut balances: HashMap<u32, Balance> = HashMap::new();
    for record in transactions {
        let balance = balances.entry(record.client()).or_insert(Balance {
            available: 0,
            held: 0,
            total: 0,
            locked: false,
        });
        match record {
            Transaction::Deposit { amount, .. } => balance.available += amount,
            Transaction::Withdrawal { amount, .. } => balance.available -= amount,
            _ => return Err(format!("Invalid transaction type: {:?}", record).into()),
            // Handle chargeback, dispute, and resolve later
            // Transaction::Chargeback { client, tx } =>
            // Transaction::Dispute { client, tx } =>
            // Transaction::Resolve { client, tx } =>
        }
    }
    for record in balances {
        writer.write_record(&[
            record.0.to_string(),
            record.1.available.to_string(),
            record.1.held.to_string(),
            record.1.total.to_string(),
            record.1.locked.to_string(),
        ])?;
    }
    Ok(())
}

fn main() {
    if let Err(err) = calculate_balances() {
        println!("error running example: {}", err);
        process::exit(1);
    }
}
