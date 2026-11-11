"""Drops paragraphs that are mostly repeated text, from one-paragraph-per-line plain text.

Some wikis hold thousands of articles written by a bot from one template: Telugu Wikipedia has
about 30,000 village articles whose paragraphs differ only in the village's name and its census
numbers ("గ్రామంలో కుళాయిల ద్వారా రక్షిత మంచినీటి సరఫరా జరుగుతోంది", "బంజరు భూమి: 259 హెక్టార్లు").
Left in, the template's words lead the lexicon (గ్రామం, దూరంలో, హెక్టార్లు in the 20 most frequent)
and its sentences fill the next-word pairs.

    python drop_repeats.py IN.txt --out OUT.txt [--n 4] [--min-count 50] [--share 0.5]

Two passes. The first counts every run of `n` words (digits read as 0, so numbers don't make a
sentence new). The second drops a paragraph when at least `share` of its runs occur `min-count`
times or more in the whole text; a paragraph shorter than `n` words counts as one run. Ordinary
prose repeats a phrase here and there, never half of itself, so it stays. The counts live in a
table of one-byte counters indexed by a hash of the run: two runs can share a counter, which
can only make a paragraph look more repeated, and with the default 2^27 counters it is rare.
"""

import argparse
import re
import sys
from collections.abc import Iterable, Iterator

DIGITS = re.compile(r"\d+")
TABLE_BITS = 27


def runs(line: str, n: int) -> list[str]:
    """The paragraph's runs of `n` words, digits read as 0; the whole of a shorter paragraph."""
    words = DIGITS.sub("0", line).split()
    if len(words) <= n:
        return [" ".join(words)] if words else []
    return [" ".join(words[i : i + n]) for i in range(len(words) - n + 1)]


def count(lines: Iterable[str], n: int, bits: int = TABLE_BITS) -> bytearray:
    """How often each run occurs, saturating at 255, by hash."""
    table = bytearray(1 << bits)
    mask = (1 << bits) - 1
    for line in lines:
        for run in runs(line, n):
            k = hash(run) & mask
            if table[k] < 255:
                table[k] += 1
    return table


def kept(lines: Iterable[str], table: bytearray, n: int, min_count: int, share: float) -> Iterator[str]:
    """The lines whose runs are not mostly ones that occur `min_count` times or more."""
    mask = len(table) - 1
    for line in lines:
        grams = runs(line, n)
        frequent = sum(table[hash(g) & mask] >= min_count for g in grams)
        if not grams or frequent < share * len(grams):
            yield line


def main(argv: list[str] | None = None) -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("text")
    p.add_argument("--out", required=True)
    p.add_argument("--n", type=int, default=4, help="words in a run")
    p.add_argument("--min-count", type=int, default=50, help="a run this frequent is repeated text (at most 255)")
    p.add_argument("--share", type=float, default=0.5, help="drop a paragraph with this share of repeated runs")
    args = p.parse_args(argv)
    if not 1 <= args.min_count <= 255:
        p.error("--min-count must be 1 to 255")
    with open(args.text, encoding="utf-8") as f:
        table = count(f, args.n)
    total = written = words_in = words_out = 0
    with open(args.text, encoding="utf-8") as f, open(args.out, "w", encoding="utf-8") as out:
        def read() -> Iterator[str]:
            nonlocal total, words_in
            for line in f:
                total += 1
                words_in += len(line.split())
                yield line

        for line in kept(read(), table, args.n, args.min_count, args.share):
            out.write(line)
            written += 1
            words_out += len(line.split())
    print(
        f"{args.text}: dropped {total - written:,} of {total:,} paragraphs as repeated text "
        f"({words_in - words_out:,} of {words_in:,} words)",
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()
