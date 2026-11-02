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
use ledger::deposit::{Deposit, DepositState};
use ledger::parse::parse_csv;
use ledger::transaction::Transaction;

fn calculate_balances(path: &str) -> Result<(), Box<dyn Error>> {
    let mut out = io::stdout().lock();
    writeln!(out, "client, available, held, total, locked")?;
    let transactions = parse_csv(fs::File::open(path)?)?;
    let mut balances: HashMap<u32, Balance> = HashMap::new();
    let mut deposits_by_id: HashMap<u32, Deposit> = HashMap::new();
    for record in transactions {
        let balance = balances.entry(record.client()).or_insert(Balance {
            available: 0,
            held: 0,
            locked: false,
        });
        if balance.locked {
            continue;
        }
        match record {
            Transaction::Deposit { client, tx, amount } => {
                deposits_by_id.insert(
                    tx,
                    Deposit {
                        client,
                        amount,
                        state: DepositState::Posted,
                    },
                );
                balance.available += amount;
            }
            Transaction::Withdrawal { amount, .. } => {
                if balance.available >= amount {
                    balance.available -= amount;
                }
            }
            // Handle chargeback, dispute, and resolve later
            Transaction::Dispute { client, tx } => {
                let Some(deposit) = deposits_by_id.get_mut(&tx) else {
                    continue;
                };
                if deposit.state == DepositState::Posted && balance.available >= deposit.amount {
                    deposit.state = DepositState::Disputed;
                    balance.held += deposit.amount;
                    balance.available -= deposit.amount;
                    balance.locked = false;
                }
            }
            Transaction::Chargeback { client, tx } => {
                let Some(deposit) = deposits_by_id.get_mut(&tx) else {
                    continue;
                };
                if deposit.state == DepositState::Disputed {
                    deposit.state = DepositState::ChargedBack;
                    balance.held -= deposit.amount;
                    balance.locked = true;
                }
            }
            Transaction::Resolve { client, tx } => {
                let Some(deposit) = deposits_by_id.get_mut(&tx) else {
                    continue;
                };
                if deposit.state == DepositState::Disputed {
                    deposit.state = DepositState::Posted;
                    balance.held -= deposit.amount;
                    balance.available += deposit.amount;
                    balance.locked = false;
                }
            }
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
