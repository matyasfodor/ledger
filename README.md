# ledger

A payments engine. It reads a transaction CSV and writes one balance row per client with an accepted deposit.

## Run

```sh
cargo run -- transactions.csv
```

`transactions.csv` is a small demo. Client 2's withdrawal is larger than the available balance, so it is ignored:

```text
client, available, held, total, locked
1, 1.5000, 0.0000, 1.5000, false
2, 2.0000, 0.0000, 2.0000, false
```

Stdout is only that report. A row that cannot be parsed or applied is skipped, and the rest of the file is processed. `RUST_LOG=debug` prints each skip on stderr. A missing file, or a failure to write the report, is logged at error and the process exits.

## Test

```sh
cargo test
```

`datasets/` holds input files and matching `.expected.csv` reports. The integration test runs the `ledger` binary on each input. Unit tests cover the ledger rules directly.

## Amounts

Amounts use four decimal places and are stored as `i64` minor units of `1/10_000`, so `1.5000` is `15_000`. A single amount is valid up to `922337203685477.5807`. A larger value, a fifth decimal place, or a negative amount is a bad row and is skipped.

Available and held use the same representation. A dispute can push available below zero. A deposit that would make available or `available + held` overflow `i64`, or a dispute that would make held overflow, is skipped. The previous balances stay as they are.

## Locks and disputes

An accepted deposit can be disputed by the same client. That moves its amount from available to held, and the total stays the same. Resolve moves it back, and the deposit can be disputed again. Chargeback removes it from held, locks the client, and does not change available.

A locked account ignores every later row. Deposits that are still disputed stay held, and they cannot be resolved or charged back while the lock stands. A chargeback cannot be disputed again.

## Choices

Transaction ids are unique across clients. A repeated id is ignored.

A client is written to the report only after a deposit is accepted. A rejected withdrawal, dispute, resolve, chargeback, or duplicate does not open a zero account.

A zero deposit or withdrawal is ignored. It does not reserve the transaction id.

Dispute, resolve, and chargeback do not carry an amount. A blank amount column is fine (`dispute, 1, 1,`). A row that drops the column (`dispute, 1, 1`) is skipped.
