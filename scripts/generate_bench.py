#!/usr/bin/env python3
"""Generate a benchmark input for the payments engine.

    python3 scripts/generate_bench.py

Writes datasets/bench/balanced_10m.csv. The file is deterministic and is not a
correctness fixture: it has no expected-output snapshot.

Shape: 10_000_000 rows interleaved across 18 clients, one client per logical
CPU on the machine this benchmark was sized for. Each client keeps roughly
555_000 rows, in file order. That is the smallest shape where a parallel fold
should separate from one thread.

The mix is deposits, withdrawals, disputes, and resolves. No chargebacks, so a
client is never locked and every row does real work. Amounts stay inside u32
minor units, and held funds stay under that limit too.
"""

from __future__ import annotations

import random
from pathlib import Path

SEED = 20261102
CLIENTS = 18
ROWS = 10_000_000
HELD_LIMIT = 4_000_000_000
ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "datasets" / "bench" / "balanced_10m.csv"


def fmt(minor: int) -> str:
    whole, frac = divmod(minor, 10_000)
    if frac == 0:
        return f"{whole}.0"
    return f"{whole}.{frac:04d}".rstrip("0")


class Client:
    def __init__(self, client_id: int) -> None:
        self.id = client_id
        self.available = 0
        self.held = 0
        self.posted: list[tuple[int, int]] = []
        self.disputed: list[tuple[int, int]] = []

    def emit(self, rng: random.Random, tx: int) -> tuple[str, int]:
        roll = rng.randrange(100)
        if roll < 8 and self.posted:
            disputed = self.dispute(rng)
            if disputed is not None:
                return disputed, tx
        if roll < 10 and self.disputed:
            return self.resolve(rng), tx
        if roll < 30 and self.available > 0:
            return self.withdraw(rng, tx)
        return self.deposit(rng, tx)

    def deposit(self, rng: random.Random, tx: int) -> tuple[str, int]:
        amount = rng.randrange(10_000, 200_001)
        self.available += amount
        self.posted.append((tx, amount))
        return f"deposit, {self.id}, {tx}, {fmt(amount)}\n", tx + 1

    def withdraw(self, rng: random.Random, tx: int) -> tuple[str, int]:
        amount = min(self.available, rng.randrange(10_000, 200_001))
        if amount <= 0:
            return self.deposit(rng, tx)
        self.available -= amount
        return f"withdrawal, {self.id}, {tx}, {fmt(amount)}\n", tx + 1

    def dispute(self, rng: random.Random) -> str | None:
        index = rng.randrange(len(self.posted))
        self.posted[index], self.posted[-1] = self.posted[-1], self.posted[index]
        dep_tx, amount = self.posted.pop()
        if self.held + amount > HELD_LIMIT:
            self.posted.append((dep_tx, amount))
            return None
        self.available -= amount
        self.held += amount
        self.disputed.append((dep_tx, amount))
        return f"dispute, {self.id}, {dep_tx},\n"

    def resolve(self, rng: random.Random) -> str:
        index = rng.randrange(len(self.disputed))
        self.disputed[index], self.disputed[-1] = self.disputed[-1], self.disputed[index]
        dep_tx, amount = self.disputed.pop()
        self.held -= amount
        self.available += amount
        self.posted.append((dep_tx, amount))
        return f"resolve, {self.id}, {dep_tx},\n"


def main() -> None:
    rng = random.Random(SEED)
    clients = [Client(client_id) for client_id in range(1, CLIENTS + 1)]
    OUT.parent.mkdir(parents=True, exist_ok=True)
    tx = 1
    chunk: list[str] = []
    with OUT.open("w", buffering=1024 * 1024) as handle:
        handle.write("type, client, tx, amount\n")
        for index in range(ROWS):
            line, tx = clients[index % CLIENTS].emit(rng, tx)
            chunk.append(line)
            if len(chunk) == 100_000:
                handle.writelines(chunk)
                chunk.clear()
        handle.writelines(chunk)
    print(f"wrote {OUT} ({OUT.stat().st_size} bytes, {ROWS} rows, {CLIENTS} clients)")


if __name__ == "__main__":
    main()
