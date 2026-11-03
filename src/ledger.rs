use std::collections::HashMap;
use std::fmt;
use std::io::{self, Write};

use serde::Deserialize;

use crate::money::Money;

pub type ClientId = u16;
pub type TxId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DepositState {
    Posted,
    Disputed,
    ChargedBack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StoredTx {
    Deposit {
        client: ClientId,
        amount: Money,
        state: DepositState,
    },
    Withdrawal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Balance {
    available: Money,
    held: Money,
    locked: bool,
}

impl Balance {
    fn zero() -> Self {
        Self {
            available: Money::ZERO,
            held: Money::ZERO,
            locked: false,
        }
    }
}

#[derive(Debug)]
pub enum Transaction {
    Deposit {
        client: ClientId,
        tx: TxId,
        amount: Money,
    },
    Withdrawal {
        client: ClientId,
        tx: TxId,
        amount: Money,
    },
    Dispute {
        client: ClientId,
        tx: TxId,
    },
    Resolve {
        client: ClientId,
        tx: TxId,
    },
    Chargeback {
        client: ClientId,
        tx: TxId,
    },
}

impl Transaction {
    fn client(&self) -> ClientId {
        match self {
            Transaction::Deposit { client, .. }
            | Transaction::Withdrawal { client, .. }
            | Transaction::Dispute { client, .. }
            | Transaction::Resolve { client, .. }
            | Transaction::Chargeback { client, .. } => *client,
        }
    }

    fn tx(&self) -> TxId {
        match self {
            Transaction::Deposit { tx, .. }
            | Transaction::Withdrawal { tx, .. }
            | Transaction::Dispute { tx, .. }
            | Transaction::Resolve { tx, .. }
            | Transaction::Chargeback { tx, .. } => *tx,
        }
    }
}

/// Why one input row was not applied.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Skip {
    UnknownType,
    MissingAmount,
    InvalidClient,
    InvalidTx,
    InvalidAmount { input: String },
    BrokenRecord { message: String },
    Duplicate,
    Locked,
    InsufficientFunds,
    UnknownTransaction,
    WrongClient,
    NotADeposit,
    AlreadyDisputed,
    NotDisputed,
    ChargedBack,
    ZeroAmount,
    Overflow,
}

impl fmt::Display for Skip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Skip::UnknownType => write!(f, "unknown transaction type"),
            Skip::MissingAmount => write!(f, "amount is required"),
            Skip::InvalidClient => write!(f, "invalid client id"),
            Skip::InvalidTx => write!(f, "invalid transaction id"),
            Skip::InvalidAmount { input } => write!(f, "invalid amount '{input}'"),
            Skip::BrokenRecord { message } => write!(f, "broken record: {message}"),
            Skip::Duplicate => write!(f, "duplicate transaction id"),
            Skip::Locked => write!(f, "account is locked"),
            Skip::InsufficientFunds => write!(f, "insufficient available funds"),
            Skip::UnknownTransaction => write!(f, "referenced transaction does not exist"),
            Skip::WrongClient => write!(f, "referenced transaction belongs to another client"),
            Skip::NotADeposit => write!(f, "only deposits can be disputed"),
            Skip::AlreadyDisputed => write!(f, "transaction is already under dispute"),
            Skip::NotDisputed => write!(f, "transaction is not under dispute"),
            Skip::ChargedBack => write!(f, "transaction was already charged back"),
            Skip::ZeroAmount => write!(f, "amount must be positive"),
            Skip::Overflow => write!(f, "amount overflow"),
        }
    }
}

struct RowFailure {
    line: u64,
    skip: Skip,
    detail: String,
}

impl fmt::Display for RowFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.detail.is_empty() {
            write!(f, "line {}: {}", self.line, self.skip)
        } else {
            write!(f, "line {}: {}, {}", self.line, self.skip, self.detail)
        }
    }
}

/// Client balances and the transactions that have already been accepted.
#[derive(Debug, Default)]
struct Accounts {
    balances: HashMap<ClientId, Balance>,
    transactions: HashMap<TxId, StoredTx>,
}

impl Accounts {
    fn new() -> Self {
        Self::default()
    }

    /// Apply one transaction. A client is opened only when a deposit is accepted.
    fn apply(&mut self, transaction: &Transaction) -> Result<(), Skip> {
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
            Transaction::Resolve { tx, .. } => self.resolve(client, *tx),
            Transaction::Chargeback { tx, .. } => self.chargeback(client, *tx),
        }
    }

    fn deposit(&mut self, client: ClientId, tx: TxId, amount: Money) -> Result<(), Skip> {
        if amount.is_zero() {
            return Err(Skip::ZeroAmount);
        }
        if self.transactions.contains_key(&tx) {
            return Err(Skip::Duplicate);
        }
        let mut balance = self
            .balances
            .get(&client)
            .copied()
            .unwrap_or_else(Balance::zero);
        let available = balance
            .available
            .checked_add(amount)
            .ok_or(Skip::Overflow)?;
        available.checked_add(balance.held).ok_or(Skip::Overflow)?;
        balance.available = available;
        self.balances.insert(client, balance);
        self.transactions.insert(
            tx,
            StoredTx::Deposit {
                client,
                amount,
                state: DepositState::Posted,
            },
        );
        Ok(())
    }

    fn withdraw(&mut self, client: ClientId, tx: TxId, amount: Money) -> Result<(), Skip> {
        if amount.is_zero() {
            return Err(Skip::ZeroAmount);
        }
        if self.transactions.contains_key(&tx) {
            return Err(Skip::Duplicate);
        }
        let Some(mut balance) = self.balances.get(&client).copied() else {
            return Err(Skip::InsufficientFunds);
        };
        if balance.available < amount {
            return Err(Skip::InsufficientFunds);
        }
        balance.available = balance.available.sub(amount);
        self.balances.insert(client, balance);
        self.transactions.insert(tx, StoredTx::Withdrawal);
        Ok(())
    }

    fn dispute(&mut self, client: ClientId, tx: TxId) -> Result<(), Skip> {
        let amount = self.deposit_amount(client, tx, DepositState::Posted)?;
        let mut balance = self.balances[&client];
        balance.held = balance.held.checked_add(amount).ok_or(Skip::Overflow)?;
        balance.available = balance.available.sub(amount);
        self.balances.insert(client, balance);
        self.set_deposit_state(tx, DepositState::Disputed);
        Ok(())
    }

    fn resolve(&mut self, client: ClientId, tx: TxId) -> Result<(), Skip> {
        let amount = self.deposit_amount(client, tx, DepositState::Disputed)?;
        let mut balance = self.balances[&client];
        balance.held = balance.held.sub(amount);
        balance.available = balance.available.add(amount);
        self.balances.insert(client, balance);
        self.set_deposit_state(tx, DepositState::Posted);
        Ok(())
    }

    fn chargeback(&mut self, client: ClientId, tx: TxId) -> Result<(), Skip> {
        let amount = self.deposit_amount(client, tx, DepositState::Disputed)?;
        let mut balance = self.balances[&client];
        balance.held = balance.held.sub(amount);
        balance.locked = true;
        self.balances.insert(client, balance);
        self.set_deposit_state(tx, DepositState::ChargedBack);
        Ok(())
    }

    fn deposit_amount(
        &self,
        client: ClientId,
        tx: TxId,
        expected: DepositState,
    ) -> Result<Money, Skip> {
        match self.transactions.get(&tx) {
            Some(StoredTx::Deposit {
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
            Some(StoredTx::Withdrawal) => Err(Skip::NotADeposit),
            None => Err(Skip::UnknownTransaction),
        }
    }

    fn set_deposit_state(&mut self, tx: TxId, state: DepositState) {
        if let Some(StoredTx::Deposit { state: slot, .. }) = self.transactions.get_mut(&tx) {
            *slot = state;
        }
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

fn write_balances(out: &mut impl Write, accounts: &Accounts) -> io::Result<()> {
    writeln!(out, "client, available, held, total, locked")?;
    let mut rows: Vec<_> = accounts.balances.iter().collect();
    rows.sort_by_key(|(client, _)| *client);
    for (client, balance) in rows {
        let total = balance.available.add(balance.held);
        writeln!(
            out,
            "{}, {}, {}, {}, {}",
            client,
            balance.available.format(),
            balance.held.format(),
            total.format(),
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
                        "line {}: {}, client={}, tx={}",
                        row.line,
                        skip,
                        row.transaction.client(),
                        row.transaction.tx(),
                    );
                }
            }
            Err(failure) => log::debug!("{failure}"),
        }
    }
    write_balances(output, &accounts)
}

#[derive(Debug, Deserialize)]
struct Record {
    #[serde(rename = "type")]
    tx_type: String,
    client: String,
    tx: String,
    amount: Option<String>,
}

struct ParsedRow {
    line: u64,
    transaction: Transaction,
}

struct TransactionReader<R> {
    reader: csv::Reader<R>,
    headers: csv::StringRecord,
    pending: Option<RowFailure>,
    stopped: bool,
}

fn parse_csv<R: io::Read>(reader: R) -> TransactionReader<R> {
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(reader);
    match reader.headers() {
        Ok(headers) => TransactionReader {
            headers: headers.clone(),
            reader,
            pending: None,
            stopped: false,
        },
        Err(err) => TransactionReader {
            headers: csv::StringRecord::new(),
            reader,
            pending: Some(failure(1, broken(err), String::new())),
            stopped: true,
        },
    }
}

impl<R: io::Read> Iterator for TransactionReader<R> {
    type Item = Result<ParsedRow, RowFailure>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(failure) = self.pending.take() {
            return Some(Err(failure));
        }
        if self.stopped {
            return None;
        }
        let mut record = csv::StringRecord::new();
        match self.reader.read_record(&mut record) {
            Ok(false) => None,
            Ok(true) => {
                let line = record
                    .position()
                    .map(|position| position.line())
                    .unwrap_or(0);
                Some(self.parse_record(line, &record))
            }
            Err(err) => {
                let line = self.reader.position().line();
                Some(Err(failure(line, broken(err), String::new())))
            }
        }
    }
}

impl<R: io::Read> TransactionReader<R> {
    fn parse_record(&self, line: u64, record: &csv::StringRecord) -> Result<ParsedRow, RowFailure> {
        let parsed: Record = record.deserialize(Some(&self.headers)).map_err(|err| {
            let row = record.iter().collect::<Vec<_>>().join(",");
            failure(line, broken(err), format!("row={row}"))
        })?;
        let detail = format!(
            "type={} client={} tx={} amount={}",
            parsed.tx_type,
            parsed.client,
            parsed.tx,
            parsed.amount.as_deref().unwrap_or("")
        );
        interpret(parsed)
            .map(|transaction| ParsedRow { line, transaction })
            .map_err(|skip| failure(line, skip, detail))
    }
}

fn interpret(record: Record) -> Result<Transaction, Skip> {
    let client: ClientId = record.client.parse().map_err(|_| Skip::InvalidClient)?;
    let tx: TxId = record.tx.parse().map_err(|_| Skip::InvalidTx)?;
    let amount = match record.amount.as_deref().filter(|text| !text.is_empty()) {
        None => None,
        Some(text) => Some(Money::parse(text).ok_or_else(|| Skip::InvalidAmount {
            input: text.to_owned(),
        })?),
    };
    match record.tx_type.to_ascii_lowercase().as_str() {
        "deposit" => Ok(Transaction::Deposit {
            client,
            tx,
            amount: amount.ok_or(Skip::MissingAmount)?,
        }),
        "withdrawal" => Ok(Transaction::Withdrawal {
            client,
            tx,
            amount: amount.ok_or(Skip::MissingAmount)?,
        }),
        "dispute" => Ok(Transaction::Dispute { client, tx }),
        "resolve" => Ok(Transaction::Resolve { client, tx }),
        "chargeback" => Ok(Transaction::Chargeback { client, tx }),
        _ => Err(Skip::UnknownType),
    }
}

fn broken(err: impl ToString) -> Skip {
    Skip::BrokenRecord {
        message: err.to_string(),
    }
}

fn failure(line: u64, skip: Skip, detail: impl Into<String>) -> RowFailure {
    RowFailure {
        line,
        skip,
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CEILING: &str = "922337203685477.5807";

    fn money(input: &str) -> Money {
        Money::parse(input).unwrap()
    }

    fn deposit(client: ClientId, tx: TxId, value: &str) -> Transaction {
        Transaction::Deposit {
            client,
            tx,
            amount: money(value),
        }
    }

    fn withdrawal(client: ClientId, tx: TxId, value: &str) -> Transaction {
        Transaction::Withdrawal {
            client,
            tx,
            amount: money(value),
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

    fn apply_ok(accounts: &mut Accounts, transaction: Transaction) {
        accounts
            .apply(&transaction)
            .expect("this row is an accepted transaction");
    }

    fn available(accounts: &Accounts, client: ClientId) -> String {
        accounts.balances[&client].available.format()
    }

    fn held(accounts: &Accounts, client: ClientId) -> String {
        accounts.balances[&client].held.format()
    }

    #[test]
    fn deposit_and_exact_withdrawal_are_accepted() {
        let mut accounts = Accounts::new();
        apply_ok(&mut accounts, deposit(1, 1, "1.5"));
        apply_ok(&mut accounts, withdrawal(1, 2, "1.5"));
        assert_eq!(available(&accounts, 1), "0.0000");
        assert!(matches!(
            accounts.transactions.get(&2),
            Some(StoredTx::Withdrawal)
        ));

        assert_eq!(
            accounts.apply(&withdrawal(1, 3, "0.0001")).unwrap_err(),
            Skip::InsufficientFunds
        );
        assert!(!accounts.transactions.contains_key(&3));
        apply_ok(&mut accounts, deposit(1, 3, "4.0"));
        assert_eq!(available(&accounts, 1), "4.0000");

        assert_eq!(
            accounts.apply(&withdrawal(9, 4, "1.0")).unwrap_err(),
            Skip::InsufficientFunds
        );
        assert!(!accounts.balances.contains_key(&9));
    }

    #[test]
    fn duplicate_id_does_not_move_funds_or_open_the_other_client() {
        let mut accounts = Accounts::new();
        apply_ok(&mut accounts, deposit(1, 1, "1.0"));
        assert_eq!(
            accounts.apply(&deposit(2, 1, "5.0")).unwrap_err(),
            Skip::Duplicate
        );
        assert_eq!(available(&accounts, 1), "1.0000");
        assert!(!accounts.balances.contains_key(&2));
    }

    #[test]
    fn dispute_moves_funds_and_a_rejected_dispute_opens_no_client() {
        let mut accounts = Accounts::new();
        apply_ok(&mut accounts, deposit(1, 1, "5.0"));
        apply_ok(&mut accounts, withdrawal(1, 2, "1.0"));
        apply_ok(&mut accounts, dispute(1, 1));
        assert_eq!(available(&accounts, 1), "-1.0000");
        assert_eq!(held(&accounts, 1), "5.0000");

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
        assert_eq!(available(&accounts, 1), "-1.0000");
        assert_eq!(held(&accounts, 1), "5.0000");
        assert!(!accounts.balances.contains_key(&2));
    }

    #[test]
    fn resolve_and_chargeback_apply_only_while_disputed() {
        let mut accounts = Accounts::new();
        apply_ok(&mut accounts, deposit(1, 1, "4.0"));
        apply_ok(&mut accounts, dispute(1, 1));
        assert_eq!(
            accounts.apply(&dispute(1, 1)).unwrap_err(),
            Skip::AlreadyDisputed
        );
        apply_ok(&mut accounts, resolve(1, 1));
        assert_eq!(
            accounts.apply(&resolve(1, 1)).unwrap_err(),
            Skip::NotDisputed
        );
        assert_eq!(
            accounts.apply(&chargeback(1, 1)).unwrap_err(),
            Skip::NotDisputed
        );
        assert_eq!(available(&accounts, 1), "4.0000");
        assert_eq!(held(&accounts, 1), "0.0000");
        assert!(!accounts.balances[&1].locked);

        apply_ok(&mut accounts, dispute(1, 1));
        assert_eq!(available(&accounts, 1), "0.0000");
        assert_eq!(held(&accounts, 1), "4.0000");
    }

    #[test]
    fn chargeback_locks_the_client_and_leaves_another_dispute_held() {
        let mut accounts = Accounts::new();
        apply_ok(&mut accounts, deposit(1, 1, "10.0"));
        apply_ok(&mut accounts, deposit(1, 2, "6.0"));
        apply_ok(&mut accounts, withdrawal(1, 3, "1.0"));
        apply_ok(&mut accounts, dispute(1, 1));
        apply_ok(&mut accounts, dispute(1, 2));
        apply_ok(&mut accounts, chargeback(1, 2));
        assert_eq!(available(&accounts, 1), "-1.0000");
        assert_eq!(held(&accounts, 1), "10.0000");
        assert!(accounts.balances[&1].locked);

        let before = accounts.balances[&1];
        for transaction in [
            deposit(1, 4, "1.0"),
            withdrawal(1, 5, "1.0"),
            dispute(1, 1),
            resolve(1, 1),
            chargeback(1, 1),
        ] {
            assert_eq!(accounts.apply(&transaction).unwrap_err(), Skip::Locked);
        }
        assert_eq!(accounts.balances[&1], before);
        assert!(!accounts.transactions.contains_key(&4));
    }

    #[test]
    fn zero_amount_does_not_reserve_its_id_or_open_a_client() {
        let mut accounts = Accounts::new();
        assert_eq!(
            accounts.apply(&deposit(1, 1, "0")).unwrap_err(),
            Skip::ZeroAmount
        );
        assert!(!accounts.balances.contains_key(&1));
        assert!(!accounts.transactions.contains_key(&1));

        apply_ok(&mut accounts, deposit(1, 1, "2.0"));
        assert_eq!(
            accounts.apply(&withdrawal(1, 2, "0")).unwrap_err(),
            Skip::ZeroAmount
        );
        assert!(!accounts.transactions.contains_key(&2));
        apply_ok(&mut accounts, deposit(1, 2, "3.0"));
        assert_eq!(available(&accounts, 1), "5.0000");
    }

    #[test]
    fn deposit_past_the_i64_ceiling_is_ignored() {
        let mut accounts = Accounts::new();
        apply_ok(&mut accounts, deposit(1, 1, CEILING));
        let before = accounts.balances[&1];
        assert_eq!(
            accounts.apply(&deposit(1, 2, "0.0001")).unwrap_err(),
            Skip::Overflow
        );
        assert_eq!(accounts.balances[&1], before);
        assert!(!accounts.transactions.contains_key(&2));
    }

    #[test]
    fn dispute_that_would_overflow_held_is_ignored() {
        let mut accounts = Accounts::new();
        apply_ok(&mut accounts, deposit(1, 1, CEILING));
        apply_ok(&mut accounts, withdrawal(1, 2, CEILING));
        apply_ok(&mut accounts, dispute(1, 1));
        apply_ok(&mut accounts, deposit(1, 3, "0.0001"));
        let before = accounts.balances[&1];
        assert_eq!(accounts.apply(&dispute(1, 3)).unwrap_err(), Skip::Overflow);
        assert_eq!(accounts.balances[&1], before);
        assert!(matches!(
            accounts.transactions.get(&3),
            Some(StoredTx::Deposit {
                state: DepositState::Posted,
                ..
            })
        ));
    }

    #[test]
    fn malformed_rows_are_skipped_and_a_later_deposit_applies() {
        let csv = "\
type, client, tx, amount
deposit, 1, 1, 1.23456
deposit, 1, 2, -1.0
bogus, 1, 3, 1.0
deposit, 1, 4,
deposit, 65536, 5, 1.0
deposit, 1, 4294967296, 1.0
dispute, 1, 6
deposit, 1, 6, 1000000.0
dispute, 1, 6,
deposit, 2, 7, 1.0
";
        let mut accounts = Accounts::new();
        let mut skipped = Vec::new();
        for row in parse_csv(csv.as_bytes()) {
            match row {
                Ok(row) => accounts.apply(&row.transaction).unwrap(),
                Err(failure) => skipped.push(failure),
            }
        }
        assert!(matches!(skipped[0].skip, Skip::InvalidAmount { .. }));
        assert!(matches!(skipped[1].skip, Skip::InvalidAmount { .. }));
        assert_eq!(skipped[2].skip, Skip::UnknownType);
        assert_eq!(skipped[3].skip, Skip::MissingAmount);
        assert_eq!(skipped[4].skip, Skip::InvalidClient);
        assert_eq!(skipped[5].skip, Skip::InvalidTx);
        assert!(matches!(skipped[6].skip, Skip::BrokenRecord { .. }));
        assert_eq!(skipped.len(), 7);
        assert_eq!(available(&accounts, 1), "0.0000");
        assert_eq!(held(&accounts, 1), "1000000.0000");
        assert_eq!(available(&accounts, 2), "1.0000");
        assert_eq!(available(&accounts, 1), "0.0000");
        assert_eq!(held(&accounts, 1), "1000000.0000");
        assert_eq!(available(&accounts, 2), "1.0000");
    }

    #[test]
    fn writes_sorted_balances() {
        let mut accounts = Accounts::new();
        apply_ok(&mut accounts, deposit(2, 1, "0.5"));
        apply_ok(&mut accounts, deposit(1, 2, "1.5"));
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
