use std::collections::HashMap;
use std::io::{self, Write};

use crate::amount::{PositiveAmount, format_amount};
use crate::balance::Balance;
use crate::deposit::{AccountTransaction, DepositState};
use crate::id::{ClientId, TxId};
use crate::parse::parse_csv;
use crate::transaction::Transaction;

/// Client balances and the transactions that have already been accepted.
#[derive(Debug, Default)]
pub struct Accounts {
    balances: HashMap<ClientId, Balance>,
    transactions: HashMap<TxId, AccountTransaction>,
}

impl Accounts {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn balance(&self, client: ClientId) -> Option<&Balance> {
        self.balances.get(&client)
    }

    pub fn transaction(&self, tx: TxId) -> Option<&AccountTransaction> {
        self.transactions.get(&tx)
    }

    /// Apply one transaction. Invalid or ignored rows leave accepted state unchanged,
    /// except that the client still gets a balance row once they appear in the input.
    pub fn apply(&mut self, transaction: Transaction) {
        let client = transaction.client();
        self.balances.entry(client).or_insert_with(Balance::zero);
        if self.balances[&client].locked {
            return;
        }

        match transaction {
            Transaction::Deposit { tx, amount, .. } => {
                if self.transactions.contains_key(&tx) {
                    return;
                }
                self.transactions.insert(
                    tx,
                    AccountTransaction::Deposit {
                        client,
                        amount,
                        state: DepositState::Posted,
                    },
                );
                self.balances.get_mut(&client).unwrap().available += amount;
            }
            Transaction::Withdrawal { tx, amount, .. } => {
                if self.transactions.contains_key(&tx) || self.balances[&client].available < amount
                {
                    return;
                }
                self.transactions.insert(tx, AccountTransaction::Withdrawal);
                self.balances.get_mut(&client).unwrap().available -= amount;
            }
            Transaction::Dispute { tx, .. } => {
                let Some(amount) = transition_deposit(
                    &mut self.transactions,
                    client,
                    tx,
                    DepositState::Posted,
                    DepositState::Disputed,
                ) else {
                    return;
                };
                let balance = self.balances.get_mut(&client).unwrap();
                balance.held += amount;
                balance.available -= amount;
            }
            Transaction::Chargeback { tx, .. } => {
                let Some(amount) = transition_deposit(
                    &mut self.transactions,
                    client,
                    tx,
                    DepositState::Disputed,
                    DepositState::ChargedBack,
                ) else {
                    return;
                };
                let balance = self.balances.get_mut(&client).unwrap();
                balance.held -= amount;
                balance.locked = true;
            }
            Transaction::Resolve { tx, .. } => {
                let Some(amount) = transition_deposit(
                    &mut self.transactions,
                    client,
                    tx,
                    DepositState::Disputed,
                    DepositState::Posted,
                ) else {
                    return;
                };
                let balance = self.balances.get_mut(&client).unwrap();
                balance.held -= amount;
                balance.available += amount;
            }
        }
    }
}

fn transition_deposit(
    transactions: &mut HashMap<TxId, AccountTransaction>,
    client: ClientId,
    tx: TxId,
    from: DepositState,
    to: DepositState,
) -> Option<PositiveAmount> {
    match transactions.get_mut(&tx) {
        Some(AccountTransaction::Deposit {
            client: owner,
            amount,
            state,
        }) if *owner == client && *state == from => {
            *state = to;
            Some(*amount)
        }
        _ => None,
    }
}

pub fn write_balances(out: &mut impl Write, accounts: &Accounts) -> io::Result<()> {
    writeln!(out, "client, available, held, total, locked")?;
    let mut rows: Vec<_> = accounts.balances.iter().collect();
    rows.sort_by_key(|(client, _)| *client);
    for (client, balance) in rows {
        writeln!(
            out,
            "{}, {}, {}, {}, {}",
            client,
            format_amount(balance.available),
            format_amount(balance.held),
            format_amount(balance.available + balance.held),
            balance.locked,
        )?;
    }
    Ok(())
}

pub fn process(
    input: impl io::Read,
    output: &mut impl Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut accounts = Accounts::new();
    for transaction in parse_csv(input) {
        accounts.apply(transaction?);
    }
    write_balances(output, &accounts)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::amount::{format_amount, parse_amount};

    fn amount(input: &str) -> PositiveAmount {
        parse_amount(input).unwrap()
    }

    fn apply_all(transactions: Vec<Transaction>) -> Accounts {
        let mut accounts = Accounts::new();
        for transaction in transactions {
            accounts.apply(transaction);
        }
        accounts
    }

    fn deposit(client: ClientId, tx: TxId, value: &str) -> Transaction {
        Transaction::Deposit {
            client,
            tx,
            amount: amount(value),
        }
    }

    fn withdrawal(client: ClientId, tx: TxId, value: &str) -> Transaction {
        Transaction::Withdrawal {
            client,
            tx,
            amount: amount(value),
        }
    }

    #[test]
    fn deposits_and_withdrawals_update_available_funds() {
        let accounts = apply_all(vec![
            deposit(1, 1, "1.0"),
            deposit(1, 2, "2.0"),
            withdrawal(1, 3, "1.5"),
            withdrawal(1, 4, "2.0"),
            deposit(1, 4, "9.0"),
        ]);

        let balance = accounts.balance(1).unwrap();
        assert_eq!(format_amount(balance.available), "10.5000");
        assert_eq!(balance.held, amount("0"));
        assert!(!balance.locked);
        assert!(matches!(
            accounts.transaction(3),
            Some(AccountTransaction::Withdrawal)
        ));
        assert!(matches!(
            accounts.transaction(4),
            Some(AccountTransaction::Deposit { .. })
        ));
    }

    #[test]
    fn duplicate_transaction_ids_are_ignored() {
        let accounts = apply_all(vec![deposit(1, 1, "1.0"), deposit(2, 1, "5.0")]);

        assert_eq!(
            format_amount(accounts.balance(1).unwrap().available),
            "1.0000"
        );
        assert_eq!(
            format_amount(accounts.balance(2).unwrap().available),
            "0.0000"
        );
    }

    #[test]
    fn dispute_resolve_and_chargeback_follow_deposit_state() {
        let mut accounts = apply_all(vec![
            deposit(1, 1, "5.0"),
            withdrawal(1, 2, "1.0"),
            Transaction::Dispute { client: 1, tx: 1 },
        ]);
        assert_eq!(
            format_amount(accounts.balance(1).unwrap().available),
            "-1.0000"
        );
        assert_eq!(accounts.balance(1).unwrap().held, amount("5.0"));

        accounts.apply(Transaction::Dispute { client: 2, tx: 1 });
        accounts.apply(Transaction::Resolve { client: 2, tx: 1 });
        accounts.apply(Transaction::Chargeback { client: 2, tx: 1 });
        assert_eq!(accounts.balance(1).unwrap().held, amount("5.0"));
        assert!(!accounts.balance(1).unwrap().locked);

        accounts.apply(Transaction::Resolve { client: 1, tx: 1 });
        assert_eq!(
            format_amount(accounts.balance(1).unwrap().available),
            "4.0000"
        );
        assert_eq!(accounts.balance(1).unwrap().held, amount("0"));

        accounts.apply(Transaction::Dispute { client: 1, tx: 1 });
        accounts.apply(Transaction::Chargeback { client: 1, tx: 1 });
        let balance = accounts.balance(1).unwrap();
        assert_eq!(format_amount(balance.available), "-1.0000");
        assert_eq!(balance.held, amount("0"));
        assert!(balance.locked);

        accounts.apply(deposit(1, 3, "10.0"));
        assert_eq!(
            format_amount(accounts.balance(1).unwrap().available),
            "-1.0000"
        );
        assert!(accounts.transaction(3).is_none());
    }

    #[test]
    fn ignored_dispute_actions_do_not_change_funds() {
        let accounts = apply_all(vec![
            deposit(1, 1, "2.0"),
            withdrawal(1, 2, "1.0"),
            Transaction::Dispute { client: 1, tx: 2 },
            Transaction::Dispute { client: 1, tx: 9 },
            Transaction::Resolve { client: 1, tx: 1 },
            Transaction::Chargeback { client: 1, tx: 1 },
        ]);

        let balance = accounts.balance(1).unwrap();
        assert_eq!(format_amount(balance.available), "1.0000");
        assert_eq!(balance.held, amount("0"));
        assert!(!balance.locked);
    }

    #[test]
    fn writes_sorted_balances_to_a_stream() {
        let accounts = apply_all(vec![deposit(2, 1, "0.5"), deposit(1, 2, "1.5")]);
        let mut output = Vec::new();
        write_balances(&mut output, &accounts).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "\
client, available, held, total, locked
1, 1.5000, 0.0000, 1.5000, false
2, 0.5000, 0.0000, 0.5000, false
"
        );
    }

    #[test]
    fn dataset_reports_match_expected_streams() {
        for name in [
            "small_transfers",
            "small_disputes",
            "large_transfers",
            "large_disputes",
        ] {
            let input = std::fs::read_to_string(format!("datasets/{name}.csv")).unwrap();
            let expected =
                std::fs::read_to_string(format!("datasets/{name}.expected.csv")).unwrap();
            let mut output = Vec::new();
            process(input.as_bytes(), &mut output).unwrap();
            assert_eq!(String::from_utf8(output).unwrap(), expected, "{name}");
        }
    }
}
