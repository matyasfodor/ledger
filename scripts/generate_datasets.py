#!/usr/bin/env python3
"""Generate payments-engine CSV fixtures.

Run from anywhere:

    python3 scripts/generate_datasets.py

Writes four input files, a reference account snapshot for each, and a row-by-row
manifest under datasets/. The snapshots follow the rules below.

Datasets
--------
small_transfers.csv
    Deposits and withdrawals only. Every withdrawal is within available funds.
large_transfers.csv
    Deposits and withdrawals, including withdrawals that must be ignored because
    available funds are insufficient.
small_disputes.csv
    Deposits, withdrawals, disputes, resolves, and chargebacks. Every dispute
    lifecycle event refers to an accepted deposit that is in the required state.
large_disputes.csv
    The same transaction types, mixing valid sequences with partner errors.

Reference rules
---------------
- Transaction ids are unique for accepted deposits and withdrawals. A repeated id
  is ignored. Dispute, resolve, and chargeback rows reuse the referenced id.
- A well-formed transaction opens the client before it is accepted or ignored.
  A rejected withdrawal, dispute, resolve, chargeback, or duplicate still leaves
  an account row. Ignored rows do not move funds.
- A withdrawal larger than available funds is ignored. Withdrawing the exact
  available balance is valid.
- Only an accepted deposit can be disputed, and only by the same client. The
  deposit must not already be under dispute or already charged back.
- A dispute moves the deposit amount from available to held. Total is unchanged.
  This is applied even when available becomes negative, which happens when the
  deposit was already partly withdrawn. Withdrawals are the only transaction the
  spec rejects for insufficient funds.
- Resolve and chargeback apply only while that deposit is under dispute, for the
  same client. Resolve moves the amount from held back to available. Chargeback
  removes it from held and total, leaves available unchanged, and locks the account.
- Once an account is locked, later deposits and withdrawals are ignored. A lock
  does not release other deposits that are still under dispute.
- Unknown ids, client mismatches, and lifecycle events for a transaction that is
  not currently disputed are ignored.

Amounts use four decimal places. Regeneration is deterministic.
"""

from __future__ import annotations

import csv
import random
from dataclasses import dataclass, field
from decimal import Decimal
from pathlib import Path

Q = Decimal("0.0001")
SEED = 20261102
ROOT = Path(__file__).resolve().parents[1]
OUT_DIR = ROOT / "datasets"


def money(value: Decimal | str | int) -> Decimal:
    return Decimal(str(value)).quantize(Q)


def fmt_amount(amount: Decimal) -> str:
    text = f"{amount:.4f}".rstrip("0").rstrip(".")
    if "." not in text:
        text += ".0"
    return text


def random_amount(rng: random.Random, lo: Decimal, hi: Decimal) -> Decimal:
    lo_units = int(money(lo) * 10000)
    hi_units = int(money(hi) * 10000)
    if hi_units < lo_units:
        lo_units, hi_units = hi_units, lo_units
    return Decimal(rng.randint(lo_units, hi_units)) / Decimal(10000)


@dataclass
class StoredTx:
    tx: int
    client: int
    kind: str
    amount: Decimal
    disputed: bool = False
    chargebacked: bool = False


@dataclass
class Account:
    available: Decimal = Decimal("0.0000")
    held: Decimal = Decimal("0.0000")
    locked: bool = False

    @property
    def total(self) -> Decimal:
        return self.available + self.held


@dataclass
class Row:
    kind: str
    client: int
    tx: int
    amount: Decimal | None


class Reject:
    ACCEPTED = "accepted"
    DUPLICATE = "duplicate transaction id"
    LOCKED = "account is locked"
    INSUFFICIENT = "insufficient available funds"
    MISSING = "referenced transaction does not exist"
    WRONG_CLIENT = "referenced transaction belongs to another client"
    NOT_DEPOSIT = "only deposits can be disputed"
    ALREADY_DISPUTED = "transaction is already under dispute"
    NOT_DISPUTED = "transaction is not under dispute"
    CHARGED_BACK = "transaction was already charged back"


@dataclass
class Engine:
    accounts: dict[int, Account] = field(default_factory=dict)
    txs: dict[int, StoredTx] = field(default_factory=dict)

    def apply(self, row: Row) -> str:
        self.accounts.setdefault(row.client, Account())
        if row.kind == "deposit":
            return self._deposit(row)
        if row.kind == "withdrawal":
            return self._withdraw(row)
        if row.kind == "dispute":
            return self._dispute(row)
        if row.kind == "resolve":
            return self._settle(row, chargeback=False)
        if row.kind == "chargeback":
            return self._settle(row, chargeback=True)
        raise ValueError(f"unknown transaction type: {row.kind}")

    def _deposit(self, row: Row) -> str:
        if row.tx in self.txs:
            return Reject.DUPLICATE
        account = self.accounts.get(row.client)
        if account is not None and account.locked:
            return Reject.LOCKED
        if row.amount is None or row.amount <= 0:
            raise ValueError(f"deposit {row.tx} needs a positive amount")
        account = self.accounts.setdefault(row.client, Account())
        account.available = money(account.available + row.amount)
        self.txs[row.tx] = StoredTx(row.tx, row.client, "deposit", row.amount)
        return Reject.ACCEPTED

    def _withdraw(self, row: Row) -> str:
        if row.tx in self.txs:
            return Reject.DUPLICATE
        if row.amount is None or row.amount <= 0:
            raise ValueError(f"withdrawal {row.tx} needs a positive amount")
        account = self.accounts.get(row.client)
        if account is not None and account.locked:
            return Reject.LOCKED
        available = account.available if account is not None else Decimal("0")
        if row.amount > available:
            return Reject.INSUFFICIENT
        assert account is not None
        account.available = money(account.available - row.amount)
        self.txs[row.tx] = StoredTx(row.tx, row.client, "withdrawal", row.amount)
        return Reject.ACCEPTED

    def _dispute(self, row: Row) -> str:
        account = self.accounts.get(row.client)
        if account is not None and account.locked:
            return Reject.LOCKED
        stored = self.txs.get(row.tx)
        if stored is None:
            return Reject.MISSING
        if stored.client != row.client:
            return Reject.WRONG_CLIENT
        if stored.kind != "deposit":
            return Reject.NOT_DEPOSIT
        if stored.chargebacked:
            return Reject.CHARGED_BACK
        if stored.disputed:
            return Reject.ALREADY_DISPUTED
        assert account is not None
        stored.disputed = True
        account.available = money(account.available - stored.amount)
        account.held = money(account.held + stored.amount)
        return Reject.ACCEPTED

    def _settle(self, row: Row, chargeback: bool) -> str:
        account = self.accounts.get(row.client)
        if account is not None and account.locked:
            return Reject.LOCKED
        stored = self.txs.get(row.tx)
        if stored is None:
            return Reject.MISSING
        if stored.client != row.client:
            return Reject.WRONG_CLIENT
        if not stored.disputed:
            return Reject.NOT_DISPUTED
        assert account is not None
        stored.disputed = False
        account.held = money(account.held - stored.amount)
        if chargeback:
            stored.chargebacked = True
            account.locked = True
        else:
            account.available = money(account.available + stored.amount)
        return Reject.ACCEPTED

    def assert_invariants(self) -> None:
        for client, account in self.accounts.items():
            if account.held < 0:
                raise AssertionError(f"client {client} has negative held funds")
            if account.available + account.held != account.total:
                raise AssertionError(f"client {client} total does not match parts")
            held = sum(
                (
                    tx.amount
                    for tx in self.txs.values()
                    if tx.client == client and tx.disputed
                ),
                Decimal("0"),
            )
            if money(held) != account.held:
                raise AssertionError(f"client {client} held does not match disputes")
            if account.locked and not any(
                tx.client == client and tx.chargebacked for tx in self.txs.values()
            ):
                raise AssertionError(f"client {client} is locked without a chargeback")


class DatasetBuilder:
    def __init__(self, name: str) -> None:
        self.name = name
        self.engine = Engine()
        self.rows: list[Row] = []
        self.notes: list[dict[str, str]] = []
        self.next_tx = 1

    def emit(
        self,
        kind: str,
        client: int,
        *,
        amount: Decimal | str | None = None,
        tx: int | None = None,
        coherent: bool,
        detail: str,
        expect: str = Reject.ACCEPTED,
    ) -> int:
        if kind in ("deposit", "withdrawal"):
            if tx is None:
                tx = self.next_tx
                self.next_tx += 1
            else:
                self.next_tx = max(self.next_tx, tx + 1)
            parsed_amount = money(amount) if amount is not None else None
        else:
            if tx is None:
                raise ValueError(f"{kind} requires a transaction id")
            parsed_amount = None

        row = Row(kind, client, tx, parsed_amount)
        result = self.engine.apply(row)
        if result != expect:
            raise AssertionError(
                f"{self.name} {kind} client {client} tx {tx}: "
                f"expected {expect!r}, got {result!r} ({detail})"
            )
        if coherent and result != Reject.ACCEPTED:
            raise AssertionError(f"{self.name} marked an ignored row coherent: {detail}")
        if not coherent and result == Reject.ACCEPTED:
            raise AssertionError(f"{self.name} marked an accepted row incoherent: {detail}")

        self.rows.append(row)
        self.notes.append(
            {
                "dataset": self.name,
                "line": str(len(self.rows) + 1),
                "type": kind,
                "client": str(client),
                "tx": str(tx),
                "amount": "" if parsed_amount is None else fmt_amount(parsed_amount),
                "coherence": "coherent" if coherent else "incoherent",
                "result": result,
                "detail": detail,
            }
        )
        return tx

    def deposit(self, client: int, amount: Decimal | str, detail: str) -> int:
        return self.emit(
            "deposit", client, amount=amount, coherent=True, detail=detail
        )

    def withdraw(
        self,
        client: int,
        amount: Decimal | str,
        *,
        coherent: bool,
        detail: str,
        expect: str,
    ) -> int:
        return self.emit(
            "withdrawal",
            client,
            amount=amount,
            coherent=coherent,
            detail=detail,
            expect=expect,
        )

    def dispute(
        self,
        client: int,
        tx: int,
        *,
        coherent: bool,
        detail: str,
        expect: str = Reject.ACCEPTED,
    ) -> int:
        return self.emit(
            "dispute",
            client,
            tx=tx,
            coherent=coherent,
            detail=detail,
            expect=expect,
        )

    def resolve(
        self,
        client: int,
        tx: int,
        *,
        coherent: bool,
        detail: str,
        expect: str = Reject.ACCEPTED,
    ) -> int:
        return self.emit(
            "resolve",
            client,
            tx=tx,
            coherent=coherent,
            detail=detail,
            expect=expect,
        )

    def chargeback(
        self,
        client: int,
        tx: int,
        *,
        coherent: bool,
        detail: str,
        expect: str = Reject.ACCEPTED,
    ) -> int:
        return self.emit(
            "chargeback",
            client,
            tx=tx,
            coherent=coherent,
            detail=detail,
            expect=expect,
        )

    def finish(self) -> None:
        self.engine.assert_invariants()


def build_small_transfers() -> DatasetBuilder:
    data = DatasetBuilder("small_transfers.csv")
    data.deposit(1, "1.0", "opening deposit")
    data.deposit(2, "2.0", "opening deposit")
    data.deposit(1, "2.0", "second deposit")
    data.withdraw(
        1,
        "1.5",
        coherent=True,
        detail="withdrawal within available funds",
        expect=Reject.ACCEPTED,
    )
    data.withdraw(
        2,
        "1.25",
        coherent=True,
        detail="withdrawal within available funds",
        expect=Reject.ACCEPTED,
    )
    data.deposit(3, "10.5", "opening deposit")
    data.withdraw(
        3,
        "0.5",
        coherent=True,
        detail="withdrawal within available funds",
        expect=Reject.ACCEPTED,
    )
    data.deposit(3, "0.0001", "smallest four-decimal deposit")
    data.withdraw(
        1,
        "1.5",
        coherent=True,
        detail="withdraw the exact remaining balance",
        expect=Reject.ACCEPTED,
    )
    data.finish()
    return data


def build_large_transfers(rng: random.Random) -> DatasetBuilder:
    data = DatasetBuilder("large_transfers.csv")
    clients = list(range(1, 11))

    for client in clients:
        for index in range(rng.randint(2, 4)):
            data.deposit(
                client,
                random_amount(rng, Decimal("1"), Decimal("40")),
                "opening deposit" if index == 0 else "additional deposit",
            )
    data.deposit(4, "1.2345", "four-decimal deposit")

    exact = data.engine.accounts[1].available
    data.withdraw(
        1,
        exact,
        coherent=True,
        detail="withdraw the exact available balance",
        expect=Reject.ACCEPTED,
    )

    for client in clients:
        extra = random_amount(rng, Decimal("0.0001"), Decimal("5"))
        available = data.engine.accounts[client].available
        data.withdraw(
            client,
            available + extra,
            coherent=False,
            detail="withdrawal exceeds available funds",
            expect=Reject.INSUFFICIENT,
        )

    for _ in range(80):
        client = rng.choice(clients)
        available = data.engine.accounts[client].available
        roll = rng.random()
        if roll < 0.45:
            data.deposit(
                client,
                random_amount(rng, Decimal("0.0001"), Decimal("25")),
                "deposit",
            )
        elif roll < 0.75 and available > 0:
            data.withdraw(
                client,
                random_amount(rng, Decimal("0.0001"), available),
                coherent=True,
                detail="withdrawal within available funds",
                expect=Reject.ACCEPTED,
            )
        else:
            extra = random_amount(rng, Decimal("0.0001"), Decimal("20"))
            data.withdraw(
                client,
                available + extra,
                coherent=False,
                detail="withdrawal exceeds available funds",
                expect=Reject.INSUFFICIENT,
            )

    data.finish()
    return data


def build_small_disputes() -> DatasetBuilder:
    data = DatasetBuilder("small_disputes.csv")

    first = data.deposit(1, "10.0", "opening deposit")
    data.deposit(1, "5.0", "second deposit")
    data.dispute(1, first, coherent=True, detail="dispute an accepted deposit")
    data.resolve(1, first, coherent=True, detail="release a disputed deposit")
    data.withdraw(
        1,
        "4.0",
        coherent=True,
        detail="withdrawal after the dispute was resolved",
        expect=Reject.ACCEPTED,
    )

    disputed = data.deposit(2, "8.0", "opening deposit")
    data.deposit(2, "2.0", "second deposit")
    data.dispute(2, disputed, coherent=True, detail="dispute an accepted deposit")
    data.chargeback(2, disputed, coherent=True, detail="charge back a disputed deposit and lock the account")

    fractional = data.deposit(3, "3.2515", "four-decimal deposit")
    data.dispute(3, fractional, coherent=True, detail="dispute the full deposit")
    data.resolve(3, fractional, coherent=True, detail="release a disputed deposit")

    held = data.deposit(4, "1.0", "opening deposit")
    data.deposit(4, "1.0", "second deposit stays available while the first is held")
    data.dispute(4, held, coherent=True, detail="hold one deposit")
    data.withdraw(
        4,
        "1.0",
        coherent=True,
        detail="withdraw only the funds that are still available",
        expect=Reject.ACCEPTED,
    )
    data.resolve(4, held, coherent=True, detail="release the held deposit")

    open_dispute = data.deposit(5, "6.0", "opening deposit")
    data.deposit(5, "1.5", "second deposit")
    data.withdraw(
        5,
        "1.5",
        coherent=True,
        detail="withdraw the undisputed deposit",
        expect=Reject.ACCEPTED,
    )
    data.dispute(
        5,
        open_dispute,
        coherent=True,
        detail="leave this dispute open so held funds are non-zero",
    )

    data.finish()
    return data


def _deposits(engine: Engine, client: int, *, disputed: bool) -> list[StoredTx]:
    return [
        tx
        for tx in engine.txs.values()
        if tx.client == client
        and tx.kind == "deposit"
        and tx.disputed is disputed
        and not tx.chargebacked
    ]


def build_large_disputes(rng: random.Random) -> DatasetBuilder:
    data = DatasetBuilder("large_disputes.csv")
    clients = list(range(1, 9))

    for client in clients:
        for _ in range(3):
            data.deposit(
                client,
                random_amount(rng, Decimal("2"), Decimal("30")),
                "opening deposit",
            )

    chargebacks_used: set[int] = set()
    for _ in range(90):
        unlocked = [
            client
            for client in clients
            if client in data.engine.accounts and not data.engine.accounts[client].locked
        ]
        if not unlocked:
            break
        client = rng.choice(unlocked)
        account = data.engine.accounts[client]
        free = _deposits(data.engine, client, disputed=False)
        held = _deposits(data.engine, client, disputed=True)
        roll = rng.random()

        if roll < 0.30 or (not free and not held):
            data.deposit(
                client,
                random_amount(rng, Decimal("0.5"), Decimal("20")),
                "deposit",
            )
            continue
        if roll < 0.48:
            if account.available > 0:
                data.withdraw(
                    client,
                    random_amount(rng, Decimal("0.0001"), account.available),
                    coherent=True,
                    detail="withdrawal within available funds",
                    expect=Reject.ACCEPTED,
                )
            else:
                data.deposit(client, random_amount(rng, Decimal("0.5"), Decimal("10")), "deposit while funds are held")
            continue
        if roll < 0.72 and free:
            fitting = [tx for tx in free if tx.amount <= account.available]
            pool = fitting if fitting and rng.random() < 0.7 else free
            target = rng.choice(pool)
            data.dispute(client, target.tx, coherent=True, detail="dispute an accepted deposit")
            continue
        if roll < 0.88 and held:
            target = rng.choice(held)
            data.resolve(client, target.tx, coherent=True, detail="release a disputed deposit")
            continue
        if held and client in (2, 3) and client not in chargebacks_used:
            target = rng.choice(held)
            data.chargeback(
                client,
                target.tx,
                coherent=True,
                detail="charge back one disputed deposit and lock the account",
            )
            chargebacks_used.add(client)
            continue
        if account.available > 0:
            data.withdraw(
                client,
                random_amount(rng, Decimal("0.0001"), account.available),
                coherent=True,
                detail="withdrawal within available funds",
                expect=Reject.ACCEPTED,
            )
        else:
            data.deposit(client, random_amount(rng, Decimal("1"), Decimal("8")), "deposit")

    open_disputes = [tx for tx in data.engine.txs.values() if tx.disputed]
    while len(open_disputes) < 2:
        candidates = [
            client
            for client in clients
            if client in data.engine.accounts
            and not data.engine.accounts[client].locked
            and _deposits(data.engine, client, disputed=False)
        ]
        if not candidates:
            break
        client = candidates[0]
        target = _deposits(data.engine, client, disputed=False)[0]
        data.dispute(client, target.tx, coherent=True, detail="leave a dispute open")
        open_disputes = [tx for tx in data.engine.txs.values() if tx.disputed]

    _script_locked_account(data)
    _script_negative_available(data)
    _script_repeated_lifecycle(data)
    _script_partner_errors(data)
    _script_chargeback_after_spend(data)
    _script_two_disputes(data)

    data.finish()
    return data


def _script_locked_account(data: DatasetBuilder) -> None:
    client = 9
    disputed = data.deposit(client, "20.0", "deposit later charged back")
    data.deposit(client, "5.0", "deposit that remains available")
    data.withdraw(
        client,
        "100.0",
        coherent=False,
        detail="withdrawal exceeds available funds",
        expect=Reject.INSUFFICIENT,
    )
    data.dispute(client, disputed, coherent=True, detail="dispute the larger deposit")
    data.chargeback(client, disputed, coherent=True, detail="chargeback locks the account")
    data.emit(
        "deposit",
        client,
        amount="3.0",
        coherent=False,
        detail="deposit after the account was locked",
        expect=Reject.LOCKED,
    )
    data.withdraw(
        client,
        "1.0",
        coherent=False,
        detail="withdrawal after the account was locked",
        expect=Reject.LOCKED,
    )


def _script_negative_available(data: DatasetBuilder) -> None:
    client = 10
    spent = data.deposit(client, "8.0", "deposit that will be disputed after a withdrawal")
    data.withdraw(
        client,
        "3.0",
        coherent=True,
        detail="spend part of the deposit before it is disputed",
        expect=Reject.ACCEPTED,
    )
    data.dispute(
        client,
        spent,
        coherent=True,
        detail="dispute a deposit already partly withdrawn; available becomes negative and total stays the same",
    )


def _script_repeated_lifecycle(data: DatasetBuilder) -> None:
    client = 11
    first = data.deposit(client, "12.0", "deposit used for repeated dispute lifecycle events")
    second = data.deposit(client, "3.0", "deposit resolved once")
    data.withdraw(
        client,
        "1.0",
        coherent=True,
        detail="withdrawal within available funds",
        expect=Reject.ACCEPTED,
    )
    data.dispute(client, first, coherent=True, detail="first dispute")
    data.dispute(
        client,
        first,
        coherent=False,
        detail="dispute a deposit that is already under dispute",
        expect=Reject.ALREADY_DISPUTED,
    )
    data.resolve(client, first, coherent=True, detail="resolve the dispute")
    data.resolve(
        client,
        first,
        coherent=False,
        detail="resolve a deposit that is no longer under dispute",
        expect=Reject.NOT_DISPUTED,
    )
    data.chargeback(
        client,
        first,
        coherent=False,
        detail="chargeback a deposit that is no longer under dispute",
        expect=Reject.NOT_DISPUTED,
    )
    data.dispute(client, second, coherent=True, detail="dispute the second deposit")
    data.resolve(client, second, coherent=True, detail="resolve the second deposit")
    available = data.engine.accounts[client].available
    data.withdraw(
        client,
        available + Decimal("0.5000"),
        coherent=False,
        detail="withdrawal exceeds available funds",
        expect=Reject.INSUFFICIENT,
    )


def _script_partner_errors(data: DatasetBuilder) -> None:
    client = 12
    deposit = data.deposit(client, "7.5", "deposit referenced by partner errors")
    other = data.deposit(client, "2.5", "undisputed deposit")
    withdrawal = data.withdraw(
        client,
        "2.0",
        coherent=True,
        detail="accepted withdrawal, later targeted by a dispute",
        expect=Reject.ACCEPTED,
    )
    failed = data.withdraw(
        client,
        "50.0",
        coherent=False,
        detail="withdrawal exceeds available funds and is not recorded",
        expect=Reject.INSUFFICIENT,
    )
    data.dispute(
        client,
        withdrawal,
        coherent=False,
        detail="dispute a withdrawal",
        expect=Reject.NOT_DEPOSIT,
    )
    data.dispute(
        client,
        failed,
        coherent=False,
        detail="dispute the id of a withdrawal that was never accepted",
        expect=Reject.MISSING,
    )
    data.dispute(
        client,
        9_000_001,
        coherent=False,
        detail="dispute an unknown transaction id",
        expect=Reject.MISSING,
    )
    data.dispute(
        1,
        deposit,
        coherent=False,
        detail="dispute names a different client than the deposit",
        expect=Reject.WRONG_CLIENT,
    )
    data.resolve(
        client,
        other,
        coherent=False,
        detail="resolve a deposit that was never disputed",
        expect=Reject.NOT_DISPUTED,
    )
    data.chargeback(
        client,
        other,
        coherent=False,
        detail="chargeback a deposit that was never disputed",
        expect=Reject.NOT_DISPUTED,
    )
    data.resolve(
        client,
        9_000_002,
        coherent=False,
        detail="resolve an unknown transaction id",
        expect=Reject.MISSING,
    )
    data.chargeback(
        client,
        9_000_003,
        coherent=False,
        detail="chargeback an unknown transaction id",
        expect=Reject.MISSING,
    )
    data.emit(
        "deposit",
        client,
        amount="99.0",
        tx=deposit,
        coherent=False,
        detail="deposit reuses an existing transaction id",
        expect=Reject.DUPLICATE,
    )
    data.dispute(
        client,
        deposit,
        coherent=True,
        detail="valid dispute left open",
    )


def _script_chargeback_after_spend(data: DatasetBuilder) -> None:
    client = 13
    deposit = data.deposit(client, "5.0", "deposit spent before chargeback")
    data.withdraw(
        client,
        "4.0",
        coherent=True,
        detail="withdraw most of the deposit",
        expect=Reject.ACCEPTED,
    )
    data.dispute(
        client,
        deposit,
        coherent=True,
        detail="dispute the spent deposit; available becomes negative",
    )
    data.chargeback(
        client,
        deposit,
        coherent=True,
        detail="chargeback removes the held amount from total and leaves available unchanged",
    )


def _script_two_disputes(data: DatasetBuilder) -> None:
    client = 14
    first = data.deposit(client, "10.0", "first of two disputed deposits")
    second = data.deposit(client, "6.0", "second of two disputed deposits")
    data.dispute(client, first, coherent=True, detail="dispute the first deposit")
    data.dispute(client, second, coherent=True, detail="dispute the second deposit")
    data.resolve(client, first, coherent=True, detail="release the first dispute")
    data.chargeback(
        client,
        second,
        coherent=True,
        detail="charge back the remaining dispute and lock the account",
    )


def write_input(path: Path, rows: list[Row]) -> None:
    lines = ["type, client, tx, amount"]
    for row in rows:
        if row.amount is None:
            lines.append(f"{row.kind}, {row.client}, {row.tx},")
        else:
            lines.append(f"{row.kind}, {row.client}, {row.tx}, {fmt_amount(row.amount)}")
    path.write_text("\n".join(lines) + "\n")


def write_expected(path: Path, engine: Engine) -> None:
    lines = ["client, available, held, total, locked"]
    for client in sorted(engine.accounts):
        account = engine.accounts[client]
        locked = "true" if account.locked else "false"
        lines.append(
            f"{client}, {account.available:.4f}, {account.held:.4f}, "
            f"{account.total:.4f}, {locked}"
        )
    path.write_text("\n".join(lines) + "\n")


def write_manifest(path: Path, datasets: list[DatasetBuilder]) -> None:
    fields = [
        "dataset",
        "line",
        "type",
        "client",
        "tx",
        "amount",
        "coherence",
        "result",
        "detail",
    ]
    with path.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        for dataset in datasets:
            writer.writerows(dataset.notes)


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def validate(datasets: list[DatasetBuilder]) -> None:
    by_name = {dataset.name: dataset for dataset in datasets}

    transfers = by_name["small_transfers.csv"]
    require(
        {row.kind for row in transfers.rows} == {"deposit", "withdrawal"},
        "small transfers should only contain deposits and withdrawals",
    )
    require(
        all(note["coherence"] == "coherent" for note in transfers.notes),
        "small transfers should be entirely coherent",
    )

    large_transfers = by_name["large_transfers.csv"]
    require(
        {row.kind for row in large_transfers.rows} == {"deposit", "withdrawal"},
        "large transfers should only contain deposits and withdrawals",
    )
    require(
        any(note["coherence"] == "incoherent" for note in large_transfers.notes),
        "large transfers should include invalid withdrawals",
    )
    require(
        all(
            note["type"] == "withdrawal"
            for note in large_transfers.notes
            if note["coherence"] == "incoherent"
        ),
        "invalid rows in large transfers should only be withdrawals",
    )

    disputes = by_name["small_disputes.csv"]
    require(
        {"dispute", "resolve", "chargeback"} <= {row.kind for row in disputes.rows},
        "small disputes should include dispute, resolve, and chargeback",
    )
    require(
        all(note["coherence"] == "coherent" for note in disputes.notes),
        "small disputes should be entirely coherent",
    )

    large_disputes = by_name["large_disputes.csv"]
    require(
        any(note["coherence"] == "coherent" for note in large_disputes.notes),
        "large disputes should include coherent rows",
    )
    require(
        any(note["coherence"] == "incoherent" for note in large_disputes.notes),
        "large disputes should include incoherent rows",
    )
    require(
        len(large_transfers.rows) > len(transfers.rows),
        "large transfers should be bigger than small transfers",
    )
    require(
        len(large_disputes.rows) > len(disputes.rows),
        "large disputes should be bigger than small disputes",
    )


def main() -> None:
    rng = random.Random(SEED)
    datasets = [
        build_small_transfers(),
        build_large_transfers(rng),
        build_small_disputes(),
        build_large_disputes(rng),
    ]
    validate(datasets)
    OUT_DIR.mkdir(exist_ok=True)
    for dataset in datasets:
        write_input(OUT_DIR / dataset.name, dataset.rows)
        expected_name = dataset.name.replace(".csv", ".expected.csv")
        write_expected(OUT_DIR / expected_name, dataset.engine)
        coherent = sum(note["coherence"] == "coherent" for note in dataset.notes)
        incoherent = len(dataset.notes) - coherent
        print(
            f"{dataset.name}: {len(dataset.rows)} rows, "
            f"{coherent} coherent, {incoherent} incoherent, "
            f"{len(dataset.engine.accounts)} clients"
        )
    write_manifest(OUT_DIR / "manifest.csv", datasets)
    print(f"wrote fixtures to {OUT_DIR}")


if __name__ == "__main__":
    main()
