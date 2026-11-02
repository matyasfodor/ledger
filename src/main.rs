use std::{collections::HashMap, error::Error, io, process};

use ledger::balance::Balance;
use ledger::parse::parse_csv;
use ledger::transaction::Transaction;

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
