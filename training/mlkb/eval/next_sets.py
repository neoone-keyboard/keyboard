"""Builds the next-word eval sets taken from fetched sources (needs data/raw, see data/manifests):

    uv run python -m mlkb.eval.next_sets

- `data/eval/next-ml.txt`: the 203 SMC conversation sentences (eval only). The next_ml gate
  also reads `next-ml.heldout.txt`, held-out Wikipedia and Wikisource sentences that
  `mlkb.lm.corpus` writes.
- `data/eval/next-manglish.tsv` (1,000 rows) and `next-manglish.tune.tsv` (500 rows):
  `roman<TAB>native` from the Dakshina romanised dev sentences (`rejoined` files). The dev
  sentences are shuffled with `SEED` and split into disjoint halves, the gate set taken from the
  first and the tune set from the second. Punctuation splits words and is dropped, sentences with
  digits are skipped, and only pairs with the same number of words, from 2 to `MAX_WORDS`, are
  kept (a one-word sentence has no next word; long ones only slow the harness). The test
  sentences stay unused.

Malayalam is normalised as the engine does (`mlkb.data.human.normalize`).
"""
import random
import sys
from pathlib import Path

from mlkb.data.human import normalize
from mlkb.eval.nextword import PUNCT

ROOT = Path(__file__).resolve().parents[3]
RAW = ROOT / "data/raw"
EVAL = ROOT / "data/eval"
SMC = RAW / "smc-msc-conversations/conversations.txt"
DAKSHINA = RAW / "dakshina/dakshina_dataset_v1.0/ml/romanized"
SEED = 20261103
GATE_ROWS, TUNE_ROWS = 1000, 500
MAX_WORDS = 20
# Punctuation that separates words: the keyboard types it apart from the word.
_SEPARATORS = str.maketrans({c: " " for c in PUNCT + "'‘’-/"})

ML_HEADER = """# Next-word eval set, Malayalam (gate split): one sentence per line, typed on the Malayalam layout.
# The 203 conversation sentences of the SMC Malayalam Speech Corpus (text/conversations.txt at commit
# f959908), by Swathanthra Malayalam Computing, CC BY-SA 4.0. Eval only: never used for training.
# Normalised as the engine does. Built by training/mlkb/eval/next_sets.py.
"""
MANGLISH_HEADER = """# Next-word eval set, Manglish ({split} split): roman<TAB>native, word-aligned sentences.
# From the Dakshina dataset v1.0 (Google Research, Roark et al., 2020), CC BY-SA 4.0:
# ml/romanized/ml.romanized.rejoined.dev.{{roman,native}}.txt, shuffled with seed {seed} and split into
# disjoint halves (gate from the first, tune from the second). Punctuation dropped, sentences with
# digits skipped, only pairs with the same word count (2 to {max_words}) kept. The test sentences are not used.
# Eval only: these sentences are removed from the LM training text. Built by training/mlkb/eval/next_sets.py.
"""


def _words(line: str) -> list[str]:
    return line.translate(_SEPARATORS).split()


def aligned_pair(roman: str, native: str) -> tuple[str, str] | None:
    """The cleaned `(roman, native)` sentence pair, or None if it can't be word-aligned."""
    if any(c.isdigit() for c in roman + native):
        return None
    r, n = _words(roman.lower()), _words(normalize(native))
    if not 2 <= len(r) <= MAX_WORDS or len(r) != len(n):
        return None
    return " ".join(r), " ".join(n)


def manglish_rows(roman: list[str], native: list[str], seed: int = SEED) -> tuple[list, list]:
    """Gate and tune rows from disjoint halves of the shuffled dev sentences."""
    order = list(range(len(roman)))
    random.Random(seed).shuffle(order)
    half = len(order) // 2
    out = []
    for part, want in ((order[:half], GATE_ROWS), (order[half:], TUNE_ROWS)):
        rows = []
        for i in part:
            if len(rows) == want:
                break
            if pair := aligned_pair(roman[i], native[i]):
                rows.append(pair)
        out.append(rows)
    return out[0], out[1]


def main() -> int:
    smc = [normalize(l.strip()) for l in SMC.read_text(encoding="utf-8").splitlines() if l.strip()]
    (EVAL / "next-ml.txt").write_text(ML_HEADER + "\n".join(smc) + "\n", encoding="utf-8")
    roman = (DAKSHINA / "ml.romanized.rejoined.dev.roman.txt").read_text(encoding="utf-8").splitlines()
    native = (DAKSHINA / "ml.romanized.rejoined.dev.native.txt").read_text(encoding="utf-8").splitlines()
    gate, tune = manglish_rows(roman, native)
    for name, split, rows in (("next-manglish.tsv", "gate", gate), ("next-manglish.tune.tsv", "tune", tune)):
        body = "".join(f"{r}\t{n}\n" for r, n in rows)
        (EVAL / name).write_text(MANGLISH_HEADER.format(split=split, seed=SEED, max_words=MAX_WORDS) + body, encoding="utf-8")
    print(f"next-ml {len(smc)}, next-manglish gate {len(gate)}, tune {len(tune)}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
