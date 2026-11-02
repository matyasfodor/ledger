use std::{error::Error, io, process};

use ledger::amount::{deserialize_optional_amount, Amount};
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

fn parse_csv() -> Result<(), Box<dyn Error>> {
    let mut rdr = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(io::stdin());
    for result in rdr.deserialize() {
        let record: Record = result?;
        println!("{:?}", record.tx_type);
    }
    Ok(())
}


fn main() {
    if let Err(err) = parse_csv() {
        println!("error running example: {}", err);
        process::exit(1);
    }
}
