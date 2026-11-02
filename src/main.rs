use std::{
    collections::HashMap,
    env,
    error::Error,
    fs,
    io::{self, Write},
    process,
};

use ledger::amount::format_amount;
use ledger::balance::Balance;
use ledger::parse::parse_csv;
use ledger::transaction::Transaction;

fn calculate_balances(path: &str) -> Result<(), Box<dyn Error>> {
    let mut out = io::stdout().lock();
    writeln!(out, "client, available, held, total, locked")?;
    let transactions = parse_csv(fs::File::open(path)?)?;
    let mut balances: HashMap<u32, Balance> = HashMap::new();
    for record in transactions {
        let balance = balances.entry(record.client()).or_insert(Balance {
            available: 0,
            held: 0,
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
        writeln!(
            out,
            "{}, {}, {}, {}, {}",
            record.0,
            format_amount(record.1.available),
            format_amount(record.1.held),
            format_amount(record.1.available + record.1.held),
            record.1.locked,
        )?;
    }
    Ok(())
}

fn main() {
    let mut args = env::args();
    args.next();
    let Some(path) = args.next() else {
        eprintln!("usage: ledger <transactions.csv>");
        process::exit(1);
    };
    if let Err(err) = calculate_balances(&path) {
        eprintln!("error running example: {}", err);
        process::exit(1);
    }
}
