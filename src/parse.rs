use std::io;

use serde::Deserialize;

use crate::amount::parse_amount;
use crate::id::{ClientId, TxId};
use crate::skip::{RowFailure, Skip};
use crate::transaction::Transaction;

#[derive(Debug, Deserialize)]
struct Record {
    #[serde(rename = "type")]
    tx_type: String,
    client: String,
    tx: String,
    amount: Option<String>,
}

pub struct ParsedRow {
    pub line: u64,
    pub transaction: Transaction,
}

pub struct TransactionReader<R> {
    reader: csv::Reader<R>,
    headers: csv::StringRecord,
    pending: Option<RowFailure>,
    stopped: bool,
}

pub fn parse_csv<R: io::Read>(reader: R) -> TransactionReader<R> {
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
        Some(text) => Some(parse_amount(text).map_err(|_| Skip::InvalidAmount {
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
    use crate::accounts::Accounts;
    use crate::amount::{PositiveAmount, format_amount};

    fn rows(csv: &str) -> Vec<Result<ParsedRow, RowFailure>> {
        parse_csv(csv.as_bytes()).collect()
    }

    fn amount_of(transaction: &Transaction) -> PositiveAmount {
        match transaction {
            Transaction::Deposit { amount, .. } | Transaction::Withdrawal { amount, .. } => *amount,
            _ => panic!("expected an amount"),
        }
    }

    #[test]
    fn accepts_up_to_four_fractional_digits() {
        let parsed = rows(
            "\
type, client, tx, amount
deposit, 1, 1, 1
deposit, 1, 2, 1.5
deposit, 1, 3, 1.25
deposit, 1, 4, 1.255
deposit, 1, 5, 1.2555
",
        );
        let formatted: Vec<String> = parsed
            .into_iter()
            .map(|row| format_amount(amount_of(&row.unwrap().transaction)))
            .collect();
        assert_eq!(
            formatted,
            vec!["1.0000", "1.5000", "1.2500", "1.2550", "1.2555"]
        );
    }

    #[test]
    fn invalid_rows_are_skipped_and_a_later_row_still_applies() {
        let csv = "\
type, client, tx, amount
deposit, 1, 1, 1.23456
deposit, 1, 2, -1.0
deposit, 1, 3, no
bogus, 1, 4, 1.0
deposit, 1, 5,
deposit, 65536, 6, 1.0
deposit, 1, 4294967296, 1.0
deposit, 1, 7, 2.0
";
        let mut accounts = Accounts::new();
        let mut skipped = Vec::new();
        for row in parse_csv(csv.as_bytes()) {
            match row {
                Ok(row) => accounts.apply(&row.transaction).unwrap(),
                Err(failure) => skipped.push(failure),
            }
        }
        assert_eq!(
            skipped
                .iter()
                .map(|failure| failure.line)
                .collect::<Vec<_>>(),
            vec![2, 3, 4, 5, 6, 7, 8]
        );
        assert!(matches!(skipped[0].skip, Skip::InvalidAmount { .. }));
        assert!(matches!(skipped[1].skip, Skip::InvalidAmount { .. }));
        assert!(matches!(skipped[2].skip, Skip::InvalidAmount { .. }));
        assert_eq!(skipped[3].skip, Skip::UnknownType);
        assert_eq!(skipped[4].skip, Skip::MissingAmount);
        assert_eq!(skipped[5].skip, Skip::InvalidClient);
        assert_eq!(skipped[6].skip, Skip::InvalidTx);
        assert_eq!(
            skipped[3].to_string(),
            "line 5: unknown transaction type, type=bogus client=1 tx=4 amount=1.0"
        );
        assert_eq!(
            skipped[5].to_string(),
            "line 7: invalid client id, type=deposit client=65536 tx=6 amount=1.0"
        );
        assert_eq!(
            format_amount(accounts.balance(1).unwrap().available),
            "2.0000"
        );
    }

    #[test]
    fn dispute_with_an_amount_keeps_the_reference_only() {
        let mut parsed = rows(
            "\
type, client, tx, amount
deposit, 1, 1, 1.5
dispute, 1, 1, 9.0
",
        );
        let dispute = parsed.pop().unwrap().unwrap().transaction;
        assert!(matches!(dispute, Transaction::Dispute { client: 1, tx: 1 }));
    }
}
