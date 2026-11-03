# Assumptions

These are decisions the input rules do not state as a single sentence. The engine follows them, and skipped rows are debug-logged to stderr. Stdout is only the account report.

- A locked account ignores every later row, including dispute, resolve, and chargeback. Downstream systems handle the locked account. A chargeback does not release other deposits that are still under dispute, and those disputes cannot be resolved while the lock stands.
- The lock is checked before the transaction reference. For a locked client, a dispute of an unknown id or of another client's deposit is logged as locked. The balances would be unchanged either way. The log reason is the part that depends on the order. For example, client 2 is locked and the row is `dispute, 2, 50`, but transaction 50 belongs to client 1. Checking the lock first logs that the account is locked. Checking the deposit first would log that it belongs to another client. Neither changes available, held, or total.
- After a chargeback, a later dispute is ignored because a charged-back deposit must not be disputed, and a later resolve is ignored because resolve applies only while the deposit is under dispute. On a locked account the logged reason is the lock, because the lock is checked first.
- After a resolve, that deposit may be disputed again.
- A client is printed only after an accepted deposit.
- A zero amount is ignored. It does not reserve its transaction id and does not create a client.
- A row that cannot be parsed or applied is skipped. The rest of the file is processed. A missing input file, or a failure to write the report, is still fatal.
- Overflow skips the row. Amounts are not wrapped. Held funds are not allowed to become negative; that row is skipped as well.
- Account-report order does not matter.
