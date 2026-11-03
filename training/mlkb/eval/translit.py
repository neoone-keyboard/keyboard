"""Transliteration accuracy measured through the production engine."""

from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class Row:
    """One eval case: the text before the word, the word as typed, and the intended Malayalam."""

    context: str
    latin: str
    malayalam: str


@dataclass(frozen=True)
class Score:
    """Top-1 and top-3 accuracy over `n` rows, and the rows whose middle slot was wrong."""

    top1: float
    top3: float
    n: int
    misses: tuple[Row, ...]


def load(path: Path) -> list[Row]:
    """Reads a `context<TAB>latin<TAB>malayalam` file; blank lines and `#` lines are skipped."""
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        context, latin, malayalam = line.split("\t")
        rows.append(Row(context, latin, malayalam))
    return rows


def query(row: Row) -> str:
    """The text the engine sees when the user has just typed `row.latin`."""
    return f"{row.context} {row.latin}" if row.context else row.latin


def score(engine, rows: list[Row]) -> Score:
    """Top-1 means the middle slot (what space commits); top-3 means anywhere in the bar."""
    hit1 = hit3 = 0
    misses = []
    for row in rows:
        bar = [text for text, _, _ in engine.suggest(query(row), "latin", True)][:3]
        hit1 += bool(bar) and bar[0] == row.malayalam
        hit3 += row.malayalam in bar
        if not bar or bar[0] != row.malayalam:
            misses.append(row)
    n = len(rows)
    return Score(hit1 / n, hit3 / n, n, tuple(misses))
