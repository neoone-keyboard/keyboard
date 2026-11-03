"""Human romanisation pairs from Dakshina and Aksharantar, and the human eval set.

    uv run python -m mlkb.data.human   # writes data/eval/translit-human.tsv

Sources and the rules for using them are in `docs/design/2026-11-03-m3-licence-audit.md`:
- **Dakshina** (`ml/lexicons/ml.translit.sampled.{train,dev,test}.tsv`: native word, romanisation,
  annotator count). Train trains; dev validates; test is the human eval set. Neither dev nor test
  words may appear in any training pair.
- **Aksharantar** `mal_train.json`, filtered by the `source` field: IndicCorp, Samanantar and
  Wikidata rows are kept, `Dakshina` and `Existing` rows dropped (CC BY-SA and unlicensed
  BrahmiNet rows). The train file has no manual (AK-*) rows; those are only in the test split,
  which is held out and not used.

Malayalam is normalised the way the engine does (`engine/core/src/normalize.rs`): NFC, old-style
chillus (consonant + virama + ZWJ) made atomic, stray joiners dropped. Latin is lowercased; rows
with anything but a–z are dropped.
"""

import argparse
import json
import random
import unicodedata
from collections import Counter
from collections.abc import Iterator
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
RAW = ROOT / "data/raw"
DAKSHINA = RAW / "dakshina/dakshina_dataset_v1.0/ml/lexicons"
AKSHARANTAR = RAW / "aksharantar/extracted"
HUMAN_EVAL = ROOT / "data/eval/translit-human.tsv"

# Aksharantar `source` values the licence audit allows; everything else is dropped.
AKSHARANTAR_KEEP = frozenset({"IndicCorp", "Samanantar", "Wikidata", "AK-NEI", "AK-NEF", "AK-Uni", "AK-Freq"})
# IndicCorp rows are mined with a model and carry its log score; the lowest fifth is mostly noise.
INDICCORP_MIN_SCORE = -0.2
# Longest Latin or Malayalam string kept, in characters: the model's max_len is 32 with bos/eos.
MAX_CHARS = 30

HUMAN_EVAL_ROWS = 2000
HUMAN_EVAL_SEED = 20261103

VIRAMA, ZWJ, ZWNJ = "്", "‍", "‌"
CHILLUS = {"ണ": "ൺ", "ന": "ൻ", "ര": "ർ", "ല": "ൽ", "ള": "ൾ", "ക": "ൿ"}


def normalize(text: str) -> str:
    """The engine's normal form: NFC, atomic chillus, joiners only after a virama inside a word.

    Like the engine, the pass repeats until nothing changes: a dropped joiner can leave text
    that one more pass would change (vowel sign halves to compose, an old-style chillu).
    """
    out = _pass(text)
    while (nxt := _pass(out)) != out:
        out = nxt
    return out


def _pass(text: str) -> str:
    nfc = unicodedata.normalize("NFC", text).replace("’", "'")
    out: list[str] = []
    i = 0
    while i < len(nfc):
        c = nfc[i]
        if nfc[i + 1 : i + 3] == VIRAMA + ZWJ and c in CHILLUS:
            out.append(CHILLUS[c])
            i += 3
            continue
        if c in (ZWJ, ZWNJ):
            nxt = nfc[i + 1] if i + 1 < len(nfc) else ""
            if not (out and out[-1] == VIRAMA and "ഀ" <= nxt <= "ൿ"):
                i += 1
                continue
        out.append(c)
        i += 1
    return "".join(out)


def _is_malayalam(word: str) -> bool:
    return bool(word) and all("ഀ" <= c <= "ൿ" or c in (ZWJ, ZWNJ) for c in word)


def clean(latin: str, native: str) -> tuple[str, str] | None:
    """The normalised (latin, malayalam) pair, or None when it can't be a training row."""
    latin = latin.strip().lower()
    native = normalize(native.strip())
    if not latin or not latin.isascii() or not latin.isalpha() or not _is_malayalam(native):
        return None
    if len(latin) > MAX_CHARS or len(native) > MAX_CHARS:
        return None
    return latin, native


@dataclass(frozen=True)
class HumanPair:
    """One human romanisation: how often annotators gave it (Dakshina), or 1."""

    latin: str
    malayalam: str
    count: int
    source: str


def dakshina(split: str, root: Path = DAKSHINA) -> list[HumanPair]:
    """Dakshina lexicon rows for `split` (train, dev or test), cleaned."""
    out = []
    path = root / f"ml.translit.sampled.{split}.tsv"
    for line in path.read_text(encoding="utf-8").splitlines():
        native, latin, count = line.split("\t")
        pair = clean(latin, native)
        if pair:
            out.append(HumanPair(pair[0], pair[1], int(count), "Dakshina"))
    return out


def aksharantar_rows(path: Path) -> Iterator[dict]:
    """The JSON-lines rows of one Aksharantar file."""
    with path.open(encoding="utf-8") as f:
        for line in f:
            if line.strip():
                yield json.loads(line)


def aksharantar(split: str = "train", root: Path = AKSHARANTAR, stats: Counter | None = None) -> list[HumanPair]:
    """Aksharantar rows whose source the audit allows, cleaned; `stats` counts kept and dropped."""
    stats = stats if stats is not None else Counter()
    out = []
    for row in aksharantar_rows(root / f"mal_{split}.json"):
        source = row.get("source") or "missing"
        if source not in AKSHARANTAR_KEEP:
            stats[f"dropped {source} (licence)"] += 1
            continue
        score = row.get("score")
        if source == "IndicCorp" and score is not None and score < INDICCORP_MIN_SCORE:
            stats["dropped IndicCorp (low mining score)"] += 1
            continue
        pair = clean(row["english word"], row["native word"])
        if not pair:
            stats[f"dropped {source} (not a-z / too long)"] += 1
            continue
        stats[f"kept {source}"] += 1
        out.append(HumanPair(pair[0], pair[1], 1, source))
    return out


def held_out_words(root: Path = DAKSHINA) -> set[str]:
    """Every Malayalam word in the Dakshina dev and test splits: never in a training pair."""
    return {p.malayalam for split in ("dev", "test") for p in dakshina(split, root)}


def eval_rows(test: list[HumanPair], n: int = HUMAN_EVAL_ROWS, seed: int = HUMAN_EVAL_SEED) -> list[HumanPair]:
    """`n` distinct test pairs sampled with `seed`; a romanisation that maps to two words is skipped."""
    targets: dict[str, set[str]] = {}
    for p in test:
        targets.setdefault(p.latin, set()).add(p.malayalam)
    pairs = sorted({(p.latin, p.malayalam) for p in test if len(targets[p.latin]) == 1})
    picked = random.Random(seed).sample(pairs, min(n, len(pairs)))
    return [HumanPair(latin, ml, 1, "Dakshina") for latin, ml in sorted(picked, key=lambda x: (x[1], x[0]))]


def write_eval(rows: list[HumanPair], path: Path = HUMAN_EVAL) -> None:
    """Writes `context<TAB>latin<TAB>malayalam` with an empty context."""
    header = (
        "# Human transliteration eval set: context<TAB>latin<TAB>malayalam (context empty).\n"
        f"# {len(rows)} pairs sampled (seed {HUMAN_EVAL_SEED}) from the Dakshina v1.0 Malayalam lexicon\n"
        "# test split (ml.translit.sampled.test.tsv), Google Research, CC BY-SA 4.0. Built by\n"
        "# `python -m mlkb.data.human`. Never train on these words.\n"
    )
    body = "".join(f"\t{p.latin}\t{p.malayalam}\n" for p in rows)
    path.write_text(header + body, encoding="utf-8")


def main(argv: list[str] | None = None) -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--out", type=Path, default=HUMAN_EVAL)
    args = p.parse_args(argv)
    test = dakshina("test")
    rows = eval_rows(test)
    write_eval(rows, args.out)
    print(f"dakshina test {len(test)} pairs, {len({t.malayalam for t in test})} words; wrote {len(rows)} to {args.out}")


if __name__ == "__main__":
    main()
