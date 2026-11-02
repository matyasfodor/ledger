#!/usr/bin/env python3
"""Run the payments engine on input CSVs and compare stdout with the expected accounts.

The engine reads the transactions CSV from a positional filename argument. For an
input `datasets/small_transfers.csv`, the expected output is
`datasets/small_transfers.expected.csv`. With no arguments, every dataset pair
in `datasets/` is checked.

    python3 scripts/check_output.py
    python3 scripts/check_output.py datasets/small_transfers.csv
    python3 scripts/check_output.py datasets/small_transfers.csv datasets/large_transfers.csv

Account rows are ordered by the first field (client id) before comparison, so
output order does not matter. Mismatched lines are printed. The process exits
with status 1 when any line differs, the engine exits with an error, or the
expected file is missing.
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DATASETS = ROOT / "datasets"


def debug_binary() -> Path:
    for line in (ROOT / "Cargo.toml").read_text().splitlines():
        if line.startswith("name = "):
            name = line.split("=", 1)[1].strip().strip('"')
            suffix = ".exe" if sys.platform == "win32" else ""
            return ROOT / "target" / "debug" / f"{name}{suffix}"
    raise SystemExit("Cargo.toml has no package name")


BINARY = debug_binary()


def dataset_inputs(paths: list[str]) -> list[Path]:
    if paths:
        return [Path(path) for path in paths]
    return sorted(
        path
        for path in DATASETS.glob("*.csv")
        if path.name != "manifest.csv" and not path.name.endswith(".expected.csv")
    )


def expected_path(source: Path) -> Path:
    return source.with_suffix(".expected.csv")


def build_engine() -> None:
    subprocess.run(
        ["cargo", "build", "--quiet"],
        cwd=ROOT,
        check=True,
    )


def run_engine(source: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(BINARY), str(source)],
        capture_output=True,
        text=True,
        cwd=ROOT,
    )


def client_id(line: str) -> tuple[int, int | str]:
    field = line.split(",", 1)[0].strip()
    if field.isdigit():
        return (0, int(field))
    return (1, field)


def ordered_account_lines(text: str) -> list[str]:
    lines = text.splitlines()
    if not lines:
        return []
    header, *rows = lines
    rows.sort(key=client_id)
    return [header, *rows]


def mismatched_lines(expected: list[str], actual: list[str]) -> list[tuple[int, str | None, str | None]]:
    mismatches: list[tuple[int, str | None, str | None]] = []
    for index in range(max(len(expected), len(actual))):
        expected_line = expected[index] if index < len(expected) else None
        actual_line = actual[index] if index < len(actual) else None
        if expected_line != actual_line:
            mismatches.append((index + 1, expected_line, actual_line))
    return mismatches


def show_mismatch(source: Path, line_number: int, expected: str | None, actual: str | None) -> None:
    print(f"{source}:{line_number}")
    print(f"  expected: {expected if expected is not None else '<no line>'}")
    print(f"  actual:   {actual if actual is not None else '<no line>'}")


def check_file(source: Path) -> bool:
    expected_file = expected_path(source)
    if not source.is_file():
        print(f"{source}: input file not found", file=sys.stderr)
        return False
    if not expected_file.is_file():
        print(f"{expected_file}: expected output not found", file=sys.stderr)
        return False

    completed = run_engine(source)
    if completed.returncode != 0:
        print(f"{source}: engine exited with status {completed.returncode}", file=sys.stderr)
        if completed.stderr:
            print(completed.stderr, end="" if completed.stderr.endswith("\n") else "\n", file=sys.stderr)
        return False

    mismatches = mismatched_lines(
        ordered_account_lines(expected_file.read_text()),
        ordered_account_lines(completed.stdout),
    )
    for line_number, expected_line, actual_line in mismatches:
        show_mismatch(source, line_number, expected_line, actual_line)
    if mismatches:
        print(f"{source}: {len(mismatches)} mismatched line(s)")
        return False

    print(f"{source}: ok")
    return True


def main(argv: list[str]) -> int:
    sources = dataset_inputs(argv)
    if not sources:
        print(f"no input CSV files found in {DATASETS}", file=sys.stderr)
        return 1
    try:
        build_engine()
    except subprocess.CalledProcessError:
        return 1
    if not BINARY.is_file():
        print(f"built binary not found at {BINARY}", file=sys.stderr)
        return 1

    failed = False
    for source in sources:
        if not check_file(source):
            failed = True
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
