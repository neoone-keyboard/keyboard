#!/usr/bin/env python3
"""Prints `cargo llvm-cov --json --summary-only` as a per-file line-coverage table, the least
covered file first (`make coverage-engine`).

    python3 -I scripts/coverage-table.py build/coverage/engine.json [engine/]

The second argument is the folder the file names are shown relative to. Percentages are cut,
not rounded, to one decimal, so a file never reads as better covered than it is.
"""

import json
import sys
from pathlib import Path


def cut(covered: int, lines: int) -> str:
    """`covered / lines` as a percentage cut to one decimal: 99.99 is 99.9, never 100.0."""
    if lines == 0:
        return "-"
    tenths = covered * 1000 // lines
    return f"{tenths // 10}.{tenths % 10}"


def rows(export: dict, base: Path) -> list[tuple[str, int, int]]:
    """`(file, lines, uncovered)` for each source file of the export."""
    out = []
    for data in export["data"]:
        for file in data["files"]:
            lines = file["summary"]["lines"]
            path = Path(file["filename"])
            name = str(path.relative_to(base)) if path.is_relative_to(base) else str(path)
            out.append((name, lines["count"], lines["count"] - lines["covered"]))
    # Least covered first; among equals, the one with more uncovered lines.
    out.sort(key=lambda r: ((r[1] - r[2]) / r[1] if r[1] else 1.0, -r[2], r[0]))
    return out


def table(export: dict, base: Path) -> str:
    files = rows(export, base)
    width = max([len(name) for name, _, _ in files] + [len("file")])
    lines = [f"{'file':{width}}  {'lines':>6}  {'uncovered':>9}  {'covered':>7}"]
    for name, count, missed in files:
        lines.append(f"{name:{width}}  {count:6}  {missed:9}  {cut(count - missed, count):>6}%")
    total = sum(count for _, count, _ in files)
    missed = sum(missed for _, _, missed in files)
    lines.append(f"{'total':{width}}  {total:6}  {missed:9}  {cut(total - missed, total):>6}%")
    return "\n".join(lines)


def main() -> None:
    export = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    base = Path(sys.argv[2]).resolve() if len(sys.argv) > 2 else Path.cwd()
    print(table(export, base))


if __name__ == "__main__":
    main()
