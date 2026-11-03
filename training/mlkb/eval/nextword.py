"""Next-word accuracy and keystroke savings, measured through the production engine.

A sentence is what is typed, word by word (`typed`: Latin for Manglish rows), and what
should end up in the text (`words`). The text before each word is the intended words,
as the keyboard would have committed them.
"""
from dataclasses import dataclass
from pathlib import Path

PUNCT = ".,!?;:\"()…"


@dataclass(frozen=True)
class Sentence:
    typed: tuple[str, ...]
    words: tuple[str, ...]


@dataclass(frozen=True)
class NextScore:
    top1: float
    top3: float
    n: int


def plain(line: str) -> Sentence:
    """A sentence typed as it is written, punctuation left out."""
    words = tuple(w.strip(PUNCT) for w in line.split() if w.strip(PUNCT))
    return Sentence(words, words)


def load_aligned(path: Path) -> list[Sentence]:
    """`roman<TAB>native` rows; rows whose word counts differ are skipped."""
    out = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        roman, native = line.split("\t")
        typed, words = tuple(roman.split()), tuple(native.split())
        if len(typed) == len(words):
            out.append(Sentence(typed, words))
    return out


def _same(a: str, b: str) -> bool:
    return a.casefold() == b.casefold()


def next_word(engine, sentences: list[Sentence], layout: str) -> NextScore:
    """After each space from the second word on: is the next word first in the bar, or in it."""
    hit1 = hit3 = n = 0
    for s in sentences:
        for i in range(1, len(s.words)):
            context = " ".join(s.words[:i]) + " "
            bar = [t for t, kind, _ in engine.suggest(context, layout, True) if kind == "next_word"][:3]
            n += 1
            hit1 += bool(bar) and _same(bar[0], s.words[i])
            hit3 += any(_same(b, s.words[i]) for b in bar)
    return NextScore(hit1 / n if n else 0.0, hit3 / n if n else 0.0, n)


def keystroke_savings(engine, sentences: list[Sentence], layout: str) -> float:
    """1 − keys used / keys without prediction. A word costs p + 1 for the first prefix p
    (0 = right after the space) whose bar holds it, the tap adding the space; otherwise its
    letters + 1."""
    cost = full = 0
    for s in sentences:
        done: list[str] = []
        for typed, want in zip(s.typed, s.words):
            before = " ".join(done) + (" " if done else "")
            word_cost = len(typed) + 1
            for p in range(0, len(typed) + 1):
                if p == 0 and not done:
                    continue
                bar = [t for t, _, _ in engine.suggest(before + typed[:p], layout, True)][:3]
                if any(_same(b, want) for b in bar):
                    word_cost = p + 1
                    break
            cost += word_cost
            full += len(typed) + 1
            done.append(want)
    return 1 - cost / full if full else 0.0
