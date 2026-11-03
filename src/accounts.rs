use std::collections::HashMap;
use std::io::{self, Write};

use crate::account_transaction::{AccountTransaction, DepositState};
use crate::amount::{PositiveAmount, SignedAmount, format_amount, format_total};
use crate::balance::Balance;
use crate::id::{ClientId, TxId};
use crate::parse::parse_csv;
use crate::skip::Skip;
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

    /// Apply one transaction. A skipped row leaves accepted state unchanged and does not create a client.
    pub fn apply(&mut self, transaction: &Transaction) -> Result<(), Skip> {
        let client = transaction.client();
        if self
            .balances
            .get(&client)
            .is_some_and(|balance| balance.locked)
        {
            return Err(Skip::Locked);
        }
        match transaction {
            Transaction::Deposit { tx, amount, .. } => self.deposit(client, *tx, *amount),
            Transaction::Withdrawal { tx, amount, .. } => self.withdraw(client, *tx, *amount),
            Transaction::Dispute { tx, .. } => self.dispute(client, *tx),
            Transaction::Chargeback { tx, .. } => self.chargeback(client, *tx),
            Transaction::Resolve { tx, .. } => self.resolve(client, *tx),
        }
    }

    fn deposit(&mut self, client: ClientId, tx: TxId, amount: PositiveAmount) -> Result<(), Skip> {
        if amount.is_zero() {
            return Err(Skip::ZeroAmount);
        }
        if self.transactions.contains_key(&tx) {
            return Err(Skip::Duplicate);
        }
        let current = self
            .balances
            .get(&client)
            .map(|balance| balance.available)
            .unwrap_or_else(|| SignedAmount::from_minor(0));
        let available = current.checked_add_positive(amount).ok_or(Skip::Overflow)?;
        self.transactions.insert(
            tx,
            AccountTransaction::Deposit {
                client,
                amount,
                state: DepositState::Posted,
            },
        );
        self.balances
            .entry(client)
            .or_insert_with(Balance::zero)
            .available = available;
        Ok(())
    }

    fn withdraw(&mut self, client: ClientId, tx: TxId, amount: PositiveAmount) -> Result<(), Skip> {
        if amount.is_zero() {
            return Err(Skip::ZeroAmount);
        }
        if self.transactions.contains_key(&tx) {
            return Err(Skip::Duplicate);
        }
        let Some(balance) = self.balances.get(&client) else {
            return Err(Skip::InsufficientFunds);
        };
        if balance.available < amount {
            return Err(Skip::InsufficientFunds);
        }
        let available = balance
            .available
            .checked_sub_positive(amount)
            .ok_or(Skip::Overflow)?;
        self.transactions.insert(tx, AccountTransaction::Withdrawal);
        self.balances.get_mut(&client).unwrap().available = available;
        Ok(())
    }

    fn dispute(&mut self, client: ClientId, tx: TxId) -> Result<(), Skip> {
        let amount = self.deposit_amount(client, tx, DepositState::Posted)?;
        let balance = self.balance_for_owner(client);
        let held = balance
            .held
            .checked_add_positive(amount)
            .ok_or(Skip::Overflow)?;
        let available = balance
            .available
            .checked_sub_positive(amount)
            .ok_or(Skip::Overflow)?;
        self.set_deposit_state(tx, DepositState::Disputed);
        let balance = self.balances.get_mut(&client).unwrap();
        balance.held = held;
        balance.available = available;
        Ok(())
    }

    fn chargeback(&mut self, client: ClientId, tx: TxId) -> Result<(), Skip> {
        let amount = self.deposit_amount(client, tx, DepositState::Disputed)?;
        let held = non_negative_held(self.balance_for_owner(client).held, amount)?;
        self.set_deposit_state(tx, DepositState::ChargedBack);
        let balance = self.balances.get_mut(&client).unwrap();
        balance.held = held;
        balance.locked = true;
        Ok(())
    }

    fn resolve(&mut self, client: ClientId, tx: TxId) -> Result<(), Skip> {
        let amount = self.deposit_amount(client, tx, DepositState::Disputed)?;
        let balance = self.balance_for_owner(client);
        let held = non_negative_held(balance.held, amount)?;
        let available = balance
            .available
            .checked_add_positive(amount)
            .ok_or(Skip::Overflow)?;
        self.set_deposit_state(tx, DepositState::Posted);
        let balance = self.balances.get_mut(&client).unwrap();
        balance.held = held;
        balance.available = available;
        Ok(())
    }

    fn deposit_amount(
        &self,
        client: ClientId,
        tx: TxId,
        expected: DepositState,
    ) -> Result<PositiveAmount, Skip> {
        match self.transactions.get(&tx) {
            Some(AccountTransaction::Deposit {
                client: owner,
                amount,
                state,
            }) => {
                if *owner != client {
                    return Err(Skip::WrongClient);
                }
                if *state != expected {
                    return Err(state_skip(expected, *state));
                }
                Ok(*amount)
            }
            Some(AccountTransaction::Withdrawal) => Err(Skip::NotADeposit),
            None => Err(Skip::UnknownTransaction),
        }
    }

    fn balance_for_owner(&self, client: ClientId) -> Balance {
        *self
            .balances
            .get(&client)
            .expect("an accepted deposit has a balance")
    }

    fn set_deposit_state(&mut self, tx: TxId, state: DepositState) {
        if let Some(AccountTransaction::Deposit { state: slot, .. }) =
            self.transactions.get_mut(&tx)
        {
            *slot = state;
        }
    }
}

fn non_negative_held(held: SignedAmount, amount: PositiveAmount) -> Result<SignedAmount, Skip> {
    let next = held.checked_sub_positive(amount).ok_or(Skip::Overflow)?;
    if next.is_negative() {
        Err(Skip::NegativeHeld)
    } else {
        Ok(next)
    }
}

fn describe_skip(accounts: &Accounts, transaction: &Transaction, skip: &Skip) -> String {
    let mut fields = vec![
        format!("client={}", transaction.client()),
        format!("tx={}", transaction.tx()),
        format!("type={}", transaction.kind()),
    ];
    if let Some(amount) = transaction.amount() {
        fields.push(format!("amount={}", format_amount(amount)));
    }
    match accounts.balance(transaction.client()) {
        Some(balance) => {
            fields.push(format!("available={}", format_amount(balance.available)));
            fields.push(format!("held={}", format_amount(balance.held)));
            fields.push(format!("locked={}", balance.locked));
        }
        None => fields.push("no account".to_string()),
    }
    match accounts.transaction(transaction.tx()) {
        Some(AccountTransaction::Deposit {
            client: owner,
            amount,
            state,
        }) => {
            fields.push(format!("owner={owner}"));
            fields.push(format!("deposit={}", format_amount(*amount)));
            fields.push(format!("state={}", deposit_state_name(*state)));
        }
        Some(AccountTransaction::Withdrawal) => fields.push("stored=withdrawal".to_string()),
        None => {}
    }
    format!("{skip}, {}", fields.join(" "))
}

fn deposit_state_name(state: DepositState) -> &'static str {
    match state {
        DepositState::Posted => "posted",
        DepositState::Disputed => "disputed",
        DepositState::ChargedBack => "charged_back",
    }
}

fn state_skip(expected: DepositState, actual: DepositState) -> Skip {
    match (expected, actual) {
        (DepositState::Posted, DepositState::Disputed) => Skip::AlreadyDisputed,
        (DepositState::Posted, DepositState::ChargedBack) => Skip::ChargedBack,
        (DepositState::Disputed, _) => Skip::NotDisputed,
        _ => Skip::NotDisputed,
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
            format_total(balance.available, balance.held),
            balance.locked,
        )?;
    }
    Ok(())
}

pub fn process(input: impl io::Read, output: &mut impl Write) -> io::Result<()> {
    let mut accounts = Accounts::new();
    for row in parse_csv(input) {
        match row {
            Ok(row) => {
                if let Err(skip) = accounts.apply(&row.transaction) {
                    log::debug!(
                        "line {}: {}",
                        row.line,
                        describe_skip(&accounts, &row.transaction, &skip)
                    );
                }
            }
            Err(failure) => log::debug!("{failure}"),
        }
    }
    write_balances(output, &accounts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::amount::{format_amount, parse_amount};

    fn amount(input: &str) -> PositiveAmount {
        parse_amount(input).unwrap()
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

    fn dispute(client: ClientId, tx: TxId) -> Transaction {
        Transaction::Dispute { client, tx }
    }

    fn resolve(client: ClientId, tx: TxId) -> Transaction {
        Transaction::Resolve { client, tx }
    }

    fn chargeback(client: ClientId, tx: TxId) -> Transaction {
        Transaction::Chargeback { client, tx }
    }

    fn apply_all(transactions: Vec<Transaction>) -> Accounts {
        let mut accounts = Accounts::new();
        for transaction in transactions {
            accounts
                .apply(&transaction)
                .expect("this row is an accepted transaction");
        }
        accounts
    }

    fn assert_available(accounts: &Accounts, client: ClientId, expected: &str) {
        assert_eq!(
            format_amount(accounts.balance(client).unwrap().available),
            expected
        );
    }

    fn assert_held(accounts: &Accounts, client: ClientId, expected: &str) {
        assert_eq!(
            format_amount(accounts.balance(client).unwrap().held),
            expected
        );
    }

    #[test]
    fn deposit_increases_available_and_total_matches_parts() {
        let accounts = apply_all(vec![deposit(1, 1, "1.5"), deposit(1, 2, "2.25")]);
        let balance = accounts.balance(1).unwrap();
        assert_available(&accounts, 1, "3.7500");
        assert_held(&accounts, 1, "0.0000");
        assert_eq!(format_amount(balance.available + balance.held), "3.7500");
        assert!(!balance.locked);
    }

    #[test]
    fn exact_withdrawal_is_accepted_and_a_larger_one_does_not_reserve_its_id() {
        let mut accounts = apply_all(vec![deposit(1, 1, "1.5"), withdrawal(1, 2, "1.5")]);
        assert_available(&accounts, 1, "0.0000");
        assert!(matches!(
            accounts.transaction(2),
            Some(AccountTransaction::Withdrawal)
        ));

        let rejected = accounts.apply(&withdrawal(1, 3, "0.0001")).unwrap_err();
        assert_eq!(rejected, Skip::InsufficientFunds);
        assert!(accounts.transaction(3).is_none());
        accounts.apply(&deposit(1, 3, "4.0")).unwrap();
        assert_available(&accounts, 1, "4.0000");
    }

    #[test]
    fn skipped_withdrawal_log_names_the_client_transaction_amount_and_balance() {
        let mut accounts = apply_all(vec![deposit(2, 2, "2.0")]);
        let transaction = withdrawal(2, 5, "3.0");
        let skip = accounts.apply(&transaction).unwrap_err();
        assert_eq!(
            describe_skip(&accounts, &transaction, &skip),
            "insufficient available funds, client=2 tx=5 type=withdrawal amount=3.0000 available=2.0000 held=0.0000 locked=false"
        );
    }

    #[test]
    fn duplicate_id_is_ignored_and_does_not_create_the_other_client() {
        let mut accounts = apply_all(vec![deposit(1, 1, "1.0")]);
        assert_eq!(
            accounts.apply(&deposit(2, 1, "5.0")).unwrap_err(),
            Skip::Duplicate
        );
        assert_available(&accounts, 1, "1.0000");
        assert!(accounts.balance(2).is_none());
    }

    #[test]
    fn dispute_moves_funds_to_held_even_when_available_becomes_negative() {
        let mut accounts = apply_all(vec![deposit(1, 1, "5.0"), withdrawal(1, 2, "1.0")]);
        accounts.apply(&dispute(1, 1)).unwrap();
        let balance = accounts.balance(1).unwrap();
        assert_available(&accounts, 1, "-1.0000");
        assert_held(&accounts, 1, "5.0000");
        assert_eq!(format_amount(balance.available + balance.held), "4.0000");
    }

    #[test]
    fn dispute_of_a_withdrawal_unknown_id_or_other_client_is_ignored() {
        let mut accounts = apply_all(vec![deposit(1, 1, "2.0"), withdrawal(1, 2, "1.0")]);
        assert_eq!(
            accounts.apply(&dispute(1, 2)).unwrap_err(),
            Skip::NotADeposit
        );
        assert_eq!(
            accounts.apply(&dispute(1, 9)).unwrap_err(),
            Skip::UnknownTransaction
        );
        assert_eq!(
            accounts.apply(&dispute(2, 1)).unwrap_err(),
            Skip::WrongClient
        );
        assert_available(&accounts, 1, "1.0000");
        assert_held(&accounts, 1, "0.0000");
        assert!(accounts.balance(2).is_none());
    }

    #[test]
    fn resolve_and_chargeback_apply_only_while_disputed() {
        let mut accounts = apply_all(vec![deposit(1, 1, "4.0")]);
        accounts.apply(&dispute(1, 1)).unwrap();
        assert_eq!(
            accounts.apply(&dispute(1, 1)).unwrap_err(),
            Skip::AlreadyDisputed
        );
        assert_eq!(accounts.apply(&resolve(1, 1)).unwrap(), ());
        assert_eq!(
            accounts.apply(&resolve(1, 1)).unwrap_err(),
            Skip::NotDisputed
        );
        assert_eq!(
            accounts.apply(&chargeback(1, 1)).unwrap_err(),
            Skip::NotDisputed
        );
        assert_available(&accounts, 1, "4.0000");
        assert_held(&accounts, 1, "0.0000");
        assert!(!accounts.balance(1).unwrap().locked);
    }

    #[test]
    fn resolved_deposit_can_be_disputed_again() {
        let mut accounts = apply_all(vec![deposit(1, 1, "3.0")]);
        accounts.apply(&dispute(1, 1)).unwrap();
        accounts.apply(&resolve(1, 1)).unwrap();
        accounts.apply(&dispute(1, 1)).unwrap();
        assert_available(&accounts, 1, "0.0000");
        assert_held(&accounts, 1, "3.0000");
    }

    #[test]
    fn chargeback_locks_and_leaves_another_dispute_held() {
        let mut accounts = apply_all(vec![deposit(1, 1, "10.0"), deposit(1, 2, "6.0")]);
        accounts.apply(&withdrawal(1, 3, "1.0")).unwrap();
        accounts.apply(&dispute(1, 1)).unwrap();
        accounts.apply(&dispute(1, 2)).unwrap();
        accounts.apply(&chargeback(1, 2)).unwrap();
        let balance = accounts.balance(1).unwrap();
        assert_available(&accounts, 1, "-1.0000");
        assert_held(&accounts, 1, "10.0000");
        assert_eq!(format_amount(balance.available + balance.held), "9.0000");
        assert!(balance.locked);
        assert!(matches!(
            accounts.transaction(2),
            Some(AccountTransaction::Deposit {
                state: DepositState::ChargedBack,
                ..
            })
        ));
    }

    #[test]
    fn locked_account_ignores_every_later_row() {
        let mut accounts = apply_all(vec![
            deposit(1, 1, "5.0"),
            deposit(1, 2, "3.0"),
            deposit(2, 3, "4.0"),
        ]);
        accounts.apply(&dispute(1, 1)).unwrap();
        accounts.apply(&chargeback(1, 1)).unwrap();
        let before = *accounts.balance(1).unwrap();

        assert_eq!(
            accounts.apply(&deposit(1, 4, "1.0")).unwrap_err(),
            Skip::Locked
        );
        assert_eq!(
            accounts.apply(&withdrawal(1, 5, "1.0")).unwrap_err(),
            Skip::Locked
        );
        assert_eq!(accounts.apply(&dispute(1, 2)).unwrap_err(), Skip::Locked);
        assert_eq!(accounts.apply(&resolve(1, 2)).unwrap_err(), Skip::Locked);
        assert_eq!(accounts.apply(&chargeback(1, 2)).unwrap_err(), Skip::Locked);
        assert_eq!(accounts.apply(&dispute(1, 99)).unwrap_err(), Skip::Locked);
        assert_eq!(accounts.apply(&dispute(1, 3)).unwrap_err(), Skip::Locked);
        assert_eq!(*accounts.balance(1).unwrap(), before);
        assert!(accounts.transaction(4).is_none());
        assert!(matches!(
            accounts.transaction(2),
            Some(AccountTransaction::Deposit {
                state: DepositState::Posted,
                ..
            })
        ));
    }

    #[test]
    fn charged_back_deposit_is_reported_when_the_account_is_not_locked() {
        let mut accounts = apply_all(vec![deposit(1, 1, "5.0")]);
        accounts.apply(&dispute(1, 1)).unwrap();
        accounts.apply(&chargeback(1, 1)).unwrap();
        accounts.balances.get_mut(&1).unwrap().locked = false;
        assert_eq!(
            accounts.apply(&dispute(1, 1)).unwrap_err(),
            Skip::ChargedBack
        );
        assert_eq!(
            accounts.apply(&resolve(1, 1)).unwrap_err(),
            Skip::NotDisputed
        );
        assert!(!accounts.balance(1).unwrap().locked);
        assert_held(&accounts, 1, "0.0000");
    }

    #[test]
    fn zero_amount_changes_nothing_and_does_not_reserve_its_id() {
        let mut accounts = Accounts::new();
        assert_eq!(
            accounts.apply(&deposit(1, 1, "0")).unwrap_err(),
            Skip::ZeroAmount
        );
        assert!(accounts.balance(1).is_none());
        assert!(accounts.transaction(1).is_none());

        accounts.apply(&deposit(1, 1, "2.0")).unwrap();
        assert_eq!(
            accounts.apply(&deposit(1, 2, "0")).unwrap_err(),
            Skip::ZeroAmount
        );
        assert_eq!(
            accounts.apply(&withdrawal(1, 2, "0")).unwrap_err(),
            Skip::ZeroAmount
        );
        assert!(accounts.transaction(2).is_none());
        accounts.apply(&deposit(1, 2, "3.0")).unwrap();
        assert_available(&accounts, 1, "5.0000");
    }

    #[test]
    fn ignored_rows_do_not_create_a_client() {
        let mut accounts = Accounts::new();
        assert_eq!(
            accounts.apply(&withdrawal(3, 1, "1.0")).unwrap_err(),
            Skip::InsufficientFunds
        );
        assert_eq!(
            accounts.apply(&dispute(4, 99)).unwrap_err(),
            Skip::UnknownTransaction
        );
        assert!(accounts.balance(3).is_none());
        assert!(accounts.balance(4).is_none());
        assert!(accounts.transaction(1).is_none());
    }

    #[test]
    fn overflow_and_negative_held_leave_the_previous_state_unchanged() {
        let mut accounts = apply_all(vec![deposit(1, 1, "5.0"), deposit(1, 2, "1.0")]);
        accounts.balances.get_mut(&1).unwrap().available = SignedAmount::from_minor(i64::MAX);
        let before = *accounts.balance(1).unwrap();
        assert_eq!(
            accounts.apply(&deposit(1, 3, "0.0001")).unwrap_err(),
            Skip::Overflow
        );
        assert_eq!(*accounts.balance(1).unwrap(), before);
        assert!(accounts.transaction(3).is_none());

        accounts.balances.get_mut(&1).unwrap().available = SignedAmount::from_minor(60_000);
        accounts.apply(&dispute(1, 1)).unwrap();
        accounts.balances.get_mut(&1).unwrap().held = SignedAmount::from_minor(0);
        let before = *accounts.balance(1).unwrap();
        assert_eq!(
            accounts.apply(&chargeback(1, 1)).unwrap_err(),
            Skip::NegativeHeld
        );
        assert_eq!(*accounts.balance(1).unwrap(), before);
        assert!(matches!(
            accounts.transaction(1),
            Some(AccountTransaction::Deposit {
                state: DepositState::Disputed,
                ..
            })
        ));
    }

    #[test]
    fn writes_a_total_larger_than_i64() {
        let mut accounts = apply_all(vec![deposit(1, 1, "1.0")]);
        let balance = accounts.balances.get_mut(&1).unwrap();
        balance.available = SignedAmount::from_minor(i64::MAX);
        balance.held = SignedAmount::from_minor(1);
        let mut output = Vec::new();
        write_balances(&mut output, &accounts).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "\
client, available, held, total, locked
1, 922337203685477.5807, 0.0001, 922337203685477.5808, false
"
        );
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
}
