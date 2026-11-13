"""Human romanisation pairs from Dakshina and Aksharantar, and the human eval set.

    uv run python -m mlkb.data.human   # writes data/eval/translit-human.tsv
    uv run python -m mlkb.data.human --lang hi   # writes data/eval/hi/translit-human.tsv

Sources and the rules for using them are in `docs/design/licence-audit.md`:
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

Every function takes `lang` (a file in `mlkb/langs`, Malayalam by default). For another
language the Dakshina and Aksharantar files are that language's, the native side is NFC with
joiners dropped, and a native word is one in the language's script block.
"""

import argparse
import json
import random
import unicodedata
from collections import Counter
from collections.abc import Iterator
from dataclasses import dataclass
from pathlib import Path

from mlkb import langs

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


@dataclass(frozen=True)
class Joiners:
    """What one script does with ZWJ and ZWNJ (`engine/core/src/normalize.rs` `Joiners`; a
    lang.json's `virama`, `joiners` and `zwjForms`)."""

    virama: str
    ranges: tuple[tuple[str, str], ...]
    keep: bool
    forms: tuple[tuple[str, str], ...] = ()

    def has(self, c: str) -> bool:
        return any(a <= c <= b for a, b in self.ranges)

    @classmethod
    def of(cls, lang: dict) -> "Joiners | None":
        """The rules a lang.json states; None when it keeps no joiner and has no forms."""
        keep = lang.get("joiners") == "afterVirama"
        forms = tuple((chr(int(a, 16)), chr(int(b, 16))) for a, b in lang.get("zwjForms", []))
        if not keep and not forms:
            return None
        pairs = ([lang["block"]] if lang.get("block") else []) + list(lang.get("letters", []))
        ranges = tuple((chr(int(a, 16)), chr(int(b, 16))) for a, b in pairs)
        return cls(chr(int(lang["virama"], 16)), ranges, keep, forms)


# Malayalam's rules, as data/langs/ml/lang.json states them and the engine embeds them.
MALAYALAM = Joiners(VIRAMA, (("ഀ", "ൿ"),), True, tuple(CHILLUS.items()))


def normalize(text: str, rules: tuple[Joiners, ...] = (MALAYALAM,)) -> str:
    """The engine's normal form: NFC, `’` to `'`, and the joiner rules of the scripts in use
    (Malayalam's by default, as in an engine that has opened no other pack): a consonant +
    virama + ZWJ with an atomic form becomes it, and a joiner stays only between a virama and a
    character of a script that keeps joiners there.

    Like the engine, the pass repeats until nothing changes: a dropped joiner can leave text
    that one more pass would change (vowel sign halves to compose, an old-style chillu).
    """
    out = _pass(text, rules)
    while (nxt := _pass(out, rules)) != out:
        out = nxt
    return out


def _pass(text: str, rules: tuple[Joiners, ...]) -> str:
    nfc = unicodedata.normalize("NFC", text).replace("’", "'")
    out: list[str] = []
    i = 0
    while i < len(nfc):
        c = nfc[i]
        if nfc[i + 2 : i + 3] == ZWJ:
            form = next((f for r in rules if nfc[i + 1] == r.virama for a, f in r.forms if a == c), None)
            if form is not None:
                out.append(form)
                i += 3
                continue
        if c in (ZWJ, ZWNJ):
            nxt = nfc[i + 1] if i + 1 < len(nfc) else ""
            if not any(r.keep and out and out[-1] == r.virama and nxt and r.has(nxt) for r in rules):
                i += 1
                continue
        out.append(c)
        i += 1
    return "".join(out)


def _is_malayalam(word: str) -> bool:
    return bool(word) and all("ഀ" <= c <= "ൿ" or c in (ZWJ, ZWNJ) for c in word)


def _normalizer(lang: str):
    """The native side's normal form for `lang`: `normalize` for Malayalam, else NFC without joiners."""
    if lang == "ml":
        return normalize
    return lambda text: "".join(c for c in unicodedata.normalize("NFC", text) if c not in (ZWJ, ZWNJ))


def _is_native(lang: str):
    if lang == "ml":
        return _is_malayalam
    spec = langs.load(lang)
    # Letters and signs of the block: not its punctuation (danda) or digits.
    return lambda word: bool(word) and all(
        spec.in_block(c) and unicodedata.category(c)[0] in "LM" for c in word
    )


def clean(latin: str, native: str, lang: str = "ml") -> tuple[str, str] | None:
    """The normalised (latin, native) pair, or None when it can't be a training row."""
    latin = latin.strip().lower()
    native = _normalizer(lang)(native.strip())
    if not latin or not latin.isascii() or not latin.isalpha() or not _is_native(lang)(native):
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


def dakshina(split: str, root: Path | None = None, lang: str = "ml") -> list[HumanPair]:
    """Dakshina lexicon rows for `split` (train, dev or test), cleaned."""
    out = []
    spec = langs.load(lang)
    root = root or (DAKSHINA if lang == "ml" else spec.dakshina_lexicons)
    path = root / f"{spec.dakshina}.translit.sampled.{split}.tsv"
    for line in path.read_text(encoding="utf-8").splitlines():
        native, latin, count = line.split("\t")
        pair = clean(latin, native, lang)
        if pair:
            out.append(HumanPair(pair[0], pair[1], int(count), "Dakshina"))
    return out


def aksharantar_rows(path: Path) -> Iterator[dict]:
    """The JSON-lines rows of one Aksharantar file."""
    with path.open(encoding="utf-8") as f:
        for line in f:
            if line.strip():
                yield json.loads(line)


def aksharantar(
    split: str = "train", root: Path | None = None, stats: Counter | None = None, lang: str = "ml"
) -> list[HumanPair]:
    """Aksharantar rows whose source the audit allows, cleaned; `stats` counts kept and dropped."""
    stats = stats if stats is not None else Counter()
    spec = langs.load(lang)
    root = root or (AKSHARANTAR if lang == "ml" else spec.aksharantar_dir)
    out = []
    for row in aksharantar_rows(root / f"{spec.aksharantar_prefix}_{split}.json"):
        source = row.get("source") or "missing"
        if source not in AKSHARANTAR_KEEP:
            stats[f"dropped {source} (licence)"] += 1
            continue
        score = row.get("score")
        if source == "IndicCorp" and score is not None and score < INDICCORP_MIN_SCORE:
            stats["dropped IndicCorp (low mining score)"] += 1
            continue
        pair = clean(row["english word"], row["native word"], lang)
        if not pair:
            stats[f"dropped {source} (not a-z / too long)"] += 1
            continue
        stats[f"kept {source}"] += 1
        out.append(HumanPair(pair[0], pair[1], 1, source))
    return out


def held_out_words(root: Path | None = None, lang: str = "ml") -> set[str]:
    """Every native word in the Dakshina dev and test splits: never in a training pair."""
    return {p.malayalam for split in ("dev", "test") for p in dakshina(split, root, lang)}


def eval_rows(test: list[HumanPair], n: int = HUMAN_EVAL_ROWS, seed: int = HUMAN_EVAL_SEED) -> list[HumanPair]:
    """`n` distinct test pairs sampled with `seed`; a romanisation that maps to two words is skipped."""
    targets: dict[str, set[str]] = {}
    for p in test:
        targets.setdefault(p.latin, set()).add(p.malayalam)
    pairs = sorted({(p.latin, p.malayalam) for p in test if len(targets[p.latin]) == 1})
    picked = random.Random(seed).sample(pairs, min(n, len(pairs)))
    return [HumanPair(latin, ml, 1, "Dakshina") for latin, ml in sorted(picked, key=lambda x: (x[1], x[0]))]


def write_eval(rows: list[HumanPair], path: Path = HUMAN_EVAL, lang: str = "ml") -> None:
    """Writes `context<TAB>latin<TAB>native` with an empty context."""
    spec = langs.load(lang)
    header = (
        f"# Human transliteration eval set: context<TAB>latin<TAB>{spec.name.lower()} (context empty).\n"
        f"# {len(rows)} pairs sampled (seed {HUMAN_EVAL_SEED}) from the Dakshina v1.0 {spec.name} lexicon\n"
        f"# test split ({spec.dakshina}.translit.sampled.test.tsv), Google Research, CC BY-SA 4.0. Built by\n"
        f"# `python -m mlkb.data.human{'' if lang == 'ml' else ' --lang ' + lang}`. Never train on these words.\n"
    )
    body = "".join(f"\t{p.latin}\t{p.malayalam}\n" for p in rows)
    path.write_text(header + body, encoding="utf-8")


def main(argv: list[str] | None = None) -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--out", type=Path)
    p.add_argument("--lang", default="ml", help="a file in mlkb/langs")
    args = p.parse_args(argv)
    out = args.out or (HUMAN_EVAL if args.lang == "ml" else langs.load(args.lang).eval_dir / "translit-human.tsv")
    out.parent.mkdir(parents=True, exist_ok=True)
    test = dakshina("test", lang=args.lang)
    rows = eval_rows(test)
    write_eval(rows, out, args.lang)
    print(f"dakshina test {len(test)} pairs, {len({t.malayalam for t in test})} words; wrote {len(rows)} to {out}")


if __name__ == "__main__":
    main()
