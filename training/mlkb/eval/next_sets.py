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

    uv run python -m mlkb.eval.next_sets --lang hi

writes Hindi's Hinglish sets instead, `data/eval/hi/next-hinglish.tsv` (gate) and
`next-hinglish.tune.tsv`, the same way from Dakshina's `hi/romanized/` dev sentences (Hindi
normalised by the engine, `mlengine.lm_normalize`). Hindi's other next-word sets, held-out
Hindi Wikipedia and Wikisource sentences, are written by `mlkb.lm.corpus --lang hi`.

    uv run python -m mlkb.eval.next_sets --lang ta

writes Tamil's Tanglish sets, `data/eval/ta/next-tanglish.tsv` and `next-tanglish.tune.tsv`, the
same way from Dakshina's `ta/romanized/` dev sentences. Eval only: the Tamil pack trains on
nothing of Dakshina's.

    uv run python -m mlkb.eval.next_sets --lang as

Dakshina has no Assamese, so Assamese's romanised sets are synthetic:
`data/eval/as/next-romanised.tsv` and `next-romanised.tune.tsv` are the held-out sentences of
`next-as.heldout.txt` and `next-as.tune.txt` (run `mlkb.lm.corpus --lang as` first), each word
spelled once by the pack's fitted tables (`mlkb.langs.romanisation.Tables.variants`, seed
`SEED`). The LM's romanised training text is spelled by the same tables, so these rows are
kinder to an LM than what people type; they say whether romanised typing gets worse, not how
good it is.
"""
import argparse
import random
import sys
from collections.abc import Callable
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


HINGLISH_HEADER = """# Next-word eval set, Hinglish ({split} split): roman<TAB>native, word-aligned sentences.
# From the Dakshina dataset v1.0 (Google Research, Roark et al., 2020), CC BY-SA 4.0:
# hi/romanized/hi.romanized.rejoined.dev.{{roman,native}}.txt, shuffled with seed {seed} and split into
# disjoint halves (gate from the first, tune from the second). Punctuation dropped, sentences with
# digits skipped, only pairs with the same word count (2 to {max_words}) kept. The test sentences are not used.
# Eval only: these sentences are removed from the LM training text. Built by training/mlkb/eval/next_sets.py --lang hi.
"""


def aligned_pair(roman: str, native: str, norm: Callable[[str], str] = normalize) -> tuple[str, str] | None:
    """The cleaned `(roman, native)` sentence pair, or None if it can't be word-aligned."""
    if any(c.isdigit() for c in roman + native):
        return None
    r, n = _words(roman.lower()), _words(norm(native))
    if not 2 <= len(r) <= MAX_WORDS or len(r) != len(n):
        return None
    return " ".join(r), " ".join(n)


def manglish_rows(roman: list[str], native: list[str], seed: int = SEED,
                  norm: Callable[[str], str] = normalize) -> tuple[list, list]:
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
            if pair := aligned_pair(roman[i], native[i], norm):
                rows.append(pair)
        out.append(rows)
    return out[0], out[1]


TANGLISH_HEADER = HINGLISH_HEADER.replace("Hinglish", "Tanglish").replace("hi/romanized/hi.", "ta/romanized/ta.") \
    .replace("--lang hi", "--lang ta").replace(
        "# Eval only: these sentences are removed from the LM training text.",
        "# Eval only: the Tamil pack trains on nothing of Dakshina's, and these sentences are removed from the\n"
        "# LM training text.")
SYNTHETIC_HEADER = """# Next-word eval set, romanised {name} ({split} split): roman<TAB>native, word-aligned sentences.
# SYNTHETIC: the sentences of {source} (held-out {name} Wikipedia, Wikisource and Wikiquote text,
# CC BY-SA 4.0, by their contributors), each word spelled once by the pack's fitted romanisation tables
# (data/langs/{id}/romanisation-weights.json, seed {seed}); nobody typed these. Punctuation dropped,
# sentences with digits or a word the tables can't spell skipped, 2 to {max_words} words.
# Built by training/mlkb/eval/next_sets.py --lang {id}.
"""


def dakshina_sets(lang: str, romanised: str, header: str) -> int:
    """`data/eval/<lang>/next-<romanised>.tsv` and its tune split, from Dakshina's `<lang>/romanized/`
    dev sentences; the native side normalised by the engine (`mlengine.lm_normalize`)."""
    import mlengine

    dakshina = RAW / f"dakshina/dakshina_dataset_v1.0/{lang}/romanized"
    roman = (dakshina / f"{lang}.romanized.rejoined.dev.roman.txt").read_text(encoding="utf-8").splitlines()
    native = (dakshina / f"{lang}.romanized.rejoined.dev.native.txt").read_text(encoding="utf-8").splitlines()
    gate, tune = manglish_rows(roman, native, norm=mlengine.lm_normalize)
    (EVAL / lang).mkdir(exist_ok=True)
    for name, split, rows in ((f"next-{romanised}.tsv", "gate", gate), (f"next-{romanised}.tune.tsv", "tune", tune)):
        body = "".join(f"{r}\t{n}\n" for r, n in rows)
        (EVAL / lang / name).write_text(header.format(split=split, seed=SEED, max_words=MAX_WORDS) + body,
                                        encoding="utf-8")
    print(f"next-{romanised} gate {len(gate)}, tune {len(tune)}", file=sys.stderr)
    return 0


def hindi() -> int:
    return dakshina_sets("hi", "hinglish", HINGLISH_HEADER)


def synthetic_rows(sentences: list[str], spell: Callable[[str, random.Random], list[str]], rng: random.Random,
                   want: int) -> list[tuple[str, str]]:
    """Up to `want` `(roman, native)` rows: each sentence's words (punctuation dropped) with one
    spelling of each drawn from `spell`; a sentence with a digit, a word `spell` can't spell or a
    spelling that isn't a-z is skipped."""
    rows = []
    for sentence in sentences:
        if len(rows) == want:
            break
        words = _words(sentence)
        if not 2 <= len(words) <= MAX_WORDS or any(c.isdigit() for c in sentence):
            continue
        typed = []
        for w in words:
            spellings = spell(w, rng)
            typed.append(rng.choice(spellings) if spellings else "")
        if all(t.isascii() and t.isalpha() for t in typed):
            rows.append((" ".join(typed), " ".join(words)))
    return rows


def synthetic_sets(lang: str, name: str, romanised: str = "romanised") -> int:
    """`data/eval/<lang>/next-<romanised>.tsv` and its tune split from the language's held-out
    native sentences, spelled by its fitted tables."""
    from mlkb.langs.romanisation import Tables

    tables = Tables.load(lang)
    rng = random.Random(SEED)
    for source, out, split, want in ((f"next-{lang}.heldout.txt", f"next-{romanised}.tsv", "gate", GATE_ROWS),
                                     (f"next-{lang}.tune.txt", f"next-{romanised}.tune.tsv", "tune", TUNE_ROWS)):
        lines = (EVAL / lang / source).read_text(encoding="utf-8").splitlines()
        rows = synthetic_rows([line for line in lines if line.strip() and not line.startswith("#")],
                              tables.variants, rng, want)
        header = SYNTHETIC_HEADER.format(name=name, split=split, source=f"data/eval/{lang}/{source}", id=lang,
                                         seed=SEED, max_words=MAX_WORDS)
        (EVAL / lang / out).write_text(header + "".join(f"{r}\t{n}\n" for r, n in rows), encoding="utf-8")
        print(f"{out}: {len(rows)} rows", file=sys.stderr)
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--lang", default="ml", choices=["ml", "hi", "as", "ta"])
    lang = parser.parse_args(argv).lang
    if lang == "hi":
        return hindi()
    if lang == "ta":
        return dakshina_sets("ta", "tanglish", TANGLISH_HEADER)
    if lang == "as":
        return synthetic_sets("as", "Assamese")
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
