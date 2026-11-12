"""Cleans extracted Urdu text (one paragraph per line) for the lexicon builder.

    python urdu_text.py IN.txt --lang data/langs/ur/lang.json --out OUT.txt [--ratio 10] [--min-count 50]

Two things the builder's normal form (the pack's `lang.json`, which the engine applies to
typed text too) can't do:

1. Presentation forms. Text copied from old typesetting carries the Arabic presentation forms
   (U+FB50-U+FDFF, U+FE70-U+FEFF: a letter's initial, medial or final shape as its own code
   point, ﻛ for ک). NFKC folds each to its letter. Only those characters are folded, so NFKC
   changes nothing else (digits, symbols, ligatures outside the ranges).

2. Words run together. Urdu letters that never join the next one (ا د ڈ ذ ر ڑ ز ژ و ے ء ں and
   their kin) leave a visible gap after them, so people often type no space there: کےساتھ for
   کے ساتھ. Left as they are, such tokens fill the lexicon with false words. A token is cut
   after a non-joining letter when both halves are words of the text at least `ratio` times
   as frequent as the token, and at least `min-count` times; the most frequent cut first, then
   each half again. Two passes: the first counts tokens, the second writes the text with the
   cuts made. A real word is rarely that much rarer than both its halves (دوران, during, is
   not cut into دو and ران).

The counts are of tokens as the builder will see them: the `replace` pairs of the lang.json
applied (Arabic ي ك ه to Urdu ی ک ہ, short vowels dropped), and only the runs of its letters.
"""

import argparse
import json
import sys
import unicodedata
from collections import Counter
from collections.abc import Iterable, Iterator
from pathlib import Path

PRESENTATION = ((0xFB50, 0xFDFF), (0xFE70, 0xFEFF))
# Letters that never join the letter after them (Urdu's right-joining letters).
NON_JOINING = set("اآأإدڈذرڑزژوؤےۓءںۃۂ")


def fold_presentation_forms(text: str) -> str:
    """Every presentation form, NFKC-folded to its letter; nothing else changes."""
    if not any(a <= ord(c) <= b for c in text for a, b in PRESENTATION):
        return text
    return "".join(
        unicodedata.normalize("NFKC", c) if any(a <= ord(c) <= b for a, b in PRESENTATION) else c
        for c in text
    )


class Urdu:
    """The lang.json's letters and replacements."""

    def __init__(self, lang: dict):
        self.letters = {
            chr(cp) for a, b in lang["letters"] for cp in range(int(a, 16), int(b, 16) + 1)
        }
        self.replace = [(a, b) for a, b in lang.get("replace", [])]

    def clean(self, line: str) -> str:
        line = unicodedata.normalize("NFC", fold_presentation_forms(line))
        for a, b in self.replace:
            if a in line:
                line = line.replace(a, b)
        return line

    def tokens(self, line: str) -> Iterator[tuple[int, int]]:
        """The (start, end) of every run of letters in `line`."""
        start = None
        for i, c in enumerate(line):
            if c in self.letters:
                if start is None:
                    start = i
            elif start is not None:
                yield start, i
                start = None
        if start is not None:
            yield start, len(line)


def count(lines: Iterable[str], urdu: Urdu) -> Counter:
    counts: Counter = Counter()
    for line in lines:
        line = urdu.clean(line)
        counts.update(line[a:b] for a, b in urdu.tokens(line))
    return counts


def cut(token: str, counts: Counter, ratio: float, min_count: int) -> list[str]:
    """`token` as the words it runs together, or `[token]`: the cut whose rarest word is the
    most frequent, each word at least `min_count` times in the text and `ratio` times as often
    as `token`."""
    floor = max(min_count, ratio * max(counts.get(token, 0), 1))
    memo: dict[str, tuple[int, list[str]] | None] = {}

    def words(text: str) -> tuple[int, list[str]] | None:
        """The best reading of `text` as words above `floor`: its rarest word's count, the words."""
        if text in memo:
            return memo[text]
        whole = counts.get(text, 0)
        best = (whole, [text]) if whole >= floor and text != token else None
        for i in range(1, len(text)):
            if text[i - 1] not in NON_JOINING:
                continue
            left, right = words(text[:i]), words(text[i:])
            if left and right and (best is None or min(left[0], right[0]) > best[0]):
                best = (min(left[0], right[0]), left[1] + right[1])
        memo[text] = best
        return best

    found = words(token)
    return found[1] if found else [token]


def rewrite(
    lines: Iterable[str], urdu: Urdu, counts: Counter, ratio: float, min_count: int, cache: dict[str, str] | None = None
) -> Iterator[str]:
    """The cleaned lines, with every run-together token cut into its words. `cache` keeps each
    token's text as written out."""
    cache = {} if cache is None else cache
    for line in lines:
        line = urdu.clean(line.rstrip("\n"))
        out, last = [], 0
        for a, b in urdu.tokens(line):
            token = line[a:b]
            if token not in cache:
                cache[token] = " ".join(cut(token, counts, ratio, min_count))
            out.append(line[last:a])
            out.append(cache[token])
            last = b
        out.append(line[last:])
        yield "".join(out) + "\n"


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("text")
    p.add_argument("--lang", required=True, help="the pack's lang.json")
    p.add_argument("--out", required=True)
    p.add_argument("--ratio", type=float, default=10.0, help="each half this many times as frequent as the token")
    p.add_argument("--min-count", type=int, default=50, help="each half at least this frequent")
    args = p.parse_args(argv)
    urdu = Urdu(json.loads(Path(args.lang).read_text(encoding="utf-8")))
    with open(args.text, encoding="utf-8") as f:
        counts = count(f, urdu)
    cache: dict[str, str] = {}
    with open(args.text, encoding="utf-8") as f, open(args.out, "w", encoding="utf-8") as out:
        for line in rewrite(f, urdu, counts, args.ratio, args.min_count, cache):
            out.write(line)
    cuts = sum(counts[token] for token, text in cache.items() if text != token)
    print(f"urdu_text: {len(counts)} distinct tokens; {cuts} run-together tokens cut", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
