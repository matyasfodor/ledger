use std::{
    collections::HashMap,
    env,
    error::Error,
    fs,
    io::{self, Write},
    process,
};

use ledger::amount::{Amount, format_amount};
use ledger::balance::Balance;
use ledger::deposit::{AccountTransaction, DepositState};
use ledger::parse::parse_csv;
use ledger::transaction::Transaction;

fn calculate_balances(path: &str) -> Result<(), Box<dyn Error>> {
    let mut out = io::stdout().lock();
    writeln!(out, "client, available, held, total, locked")?;
    let transactions = parse_csv(fs::File::open(path)?)?;
    let mut balances: HashMap<u32, Balance> = HashMap::new();
    let mut transactions_by_id: HashMap<u32, AccountTransaction> = HashMap::new();
    for record in transactions {
        let balance = balances.entry(record.client()).or_insert(Balance {
            available: Amount::from_minor(0),
            held: Amount::from_minor(0),
            locked: false,
        });
        if balance.locked {
            continue;
        }
        match record {
            Transaction::Deposit { client, tx, amount } => {
                if transactions_by_id.contains_key(&tx) {
                    continue;
                }
                transactions_by_id.insert(
                    tx,
                    AccountTransaction::Deposit {
                        client,
                        amount,
                        state: DepositState::Posted,
                    },
                );
                balance.available += amount;
            }
            Transaction::Withdrawal { tx, amount, .. } => {
                if transactions_by_id.contains_key(&tx) || balance.available < amount {
                    continue;
                }
                transactions_by_id.insert(tx, AccountTransaction::Withdrawal);
                balance.available -= amount;
            }
            Transaction::Dispute { client, tx } => {
                let Some(AccountTransaction::Deposit {
                    client: owner,
                    amount,
                    state,
                }) = transactions_by_id.get_mut(&tx)
                else {
                    continue;
                };
                if *owner != client || *state != DepositState::Posted {
                    continue;
                }
                *state = DepositState::Disputed;
                balance.held += *amount;
                balance.available -= *amount;
            }
            Transaction::Chargeback { client, tx } => {
                let Some(AccountTransaction::Deposit {
                    client: owner,
                    amount,
                    state,
                }) = transactions_by_id.get_mut(&tx)
                else {
                    continue;
                };
                if *owner != client || *state != DepositState::Disputed {
                    continue;
                }
                *state = DepositState::ChargedBack;
                balance.held -= *amount;
                balance.locked = true;
            }
            Transaction::Resolve { client, tx } => {
                let Some(AccountTransaction::Deposit {
                    client: owner,
                    amount,
                    state,
                }) = transactions_by_id.get_mut(&tx)
                else {
                    continue;
                };
                if *owner != client || *state != DepositState::Disputed {
                    continue;
                }
                *state = DepositState::Posted;
                balance.held -= *amount;
                balance.available += *amount;
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
