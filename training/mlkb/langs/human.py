"""A pack language's config (`data/langs/<id>/lang.json`), its normal form, and its human pairs.

`Lang.normalize` is `engine/lexbuild/src/native.rs` `Lang::normalize` in Python: the config's
`replace` pairs, then the engine's normal form (NFC, `’` to `'`, joiners dropped unless a
virama + joiner + letter of the block: none outside Malayalam), then `replace` again. Training
pairs are normalised with it, so they spell words as the lexicon does.

Aksharantar (`data/raw/aksharantar-<code>/extracted/<code>_{train,valid,test}.json`, see
`data/manifests/aksharantar-asm.json`) is filtered as for Malayalam (`mlkb.data.human`): rows
whose source is Dakshina or Existing are dropped, mined IndicCorp rows below the mining score
cut too.

A language may also have human pairs from files of its own, named in `data/langs/<id>/pairs.json`
(`PairSource`): Dakshina's typed lexicon for Punjabi and Sinhala, or any list of words as people
typed them. Each file is a split, read as Malayalam reads Dakshina's: the train split trains the
transliterator and calibrates the tables, the dev split validates, and the test split is held out
(the held-out eval set itself when the language has no Aksharantar data). No dev or test word is in
any training pair. The files are TSV, one pair per line, as Dakshina's lexicons are:

    native word<TAB>romanisation<TAB>count

`count` is how many people typed that romanisation for the word (Dakshina's annotator count);
it may be left out (then 1). Blank lines and lines starting with `#` are skipped. Rows are cleaned
as Aksharantar's are (`clean`): lower-cased, `a`-`z` only, the native word in the pack's normal
form and a word of its script.
"""

import hashlib
import json
import unicodedata
from collections import Counter
from dataclasses import dataclass
from functools import cache, cached_property
from pathlib import Path

from mlkb.data.human import AKSHARANTAR_KEEP, INDICCORP_MIN_SCORE, MAX_CHARS, aksharantar_rows

ROOT = Path(__file__).resolve().parents[3]
LANGS = ROOT / "data/langs"
RAW = ROOT / "data/raw"
ZWJ, ZWNJ = "‍", "‌"


def _cp(h: str) -> str:
    return chr(int(h, 16))


@dataclass(frozen=True, eq=False)
class Lang:
    data: dict

    @property
    def id(self) -> str:
        return self.data["id"]

    @cached_property
    def block(self) -> tuple[str, str]:
        """The span of the script's characters, first to last (`ScriptDef::span`)."""
        ranges = self._ranges
        return min(a for a, _ in ranges), max(b for _, b in ranges)

    @cached_property
    def _ranges(self) -> list[tuple[str, str]]:
        """The script's characters: `block`, then `letters` (a script not in one block, or one
        that lists only its letters: Urdu's)."""
        pairs = ([self.data["block"]] if self.data.get("block") else []) + list(self.data.get("letters", []))
        return [(_cp(a), _cp(b)) for a, b in pairs]

    @cached_property
    def _not_word(self) -> list[tuple[str, str]]:
        return [(_cp(a), _cp(b)) for a, b in self.data.get("notWord", [])]

    @cached_property
    def _first(self) -> list[tuple[str, str]]:
        return [(_cp(a), _cp(b)) for a, b in self.data["firstLetter"]]

    @cached_property
    def _signs(self) -> list[tuple[str, str]]:
        return [(_cp(a), _cp(b)) for a, b in self.data.get("signs", [])]

    @cached_property
    def _bases(self) -> list[tuple[str, str]]:
        return [(_cp(a), _cp(b)) for a, b in self.data.get("bases", [])]

    def _signs_follow_bases(self, word: str) -> bool:
        def within(ranges, c):
            return any(a <= c <= b for a, b in ranges)

        return all(
            not within(self._signs, c) or (i > 0 and within(self._bases, word[i - 1])) for i, c in enumerate(word)
        )

    @cached_property
    def virama(self) -> str:
        """The virama, or "" for a script without one (Urdu's): `virama` is optional in
        lang.json, as in the engine's `ScriptDef`."""
        return _cp(self.data["virama"]) if self.data.get("virama") else ""

    def in_block(self, c: str) -> bool:
        return any(a <= c <= b for a, b in self._ranges)

    def is_word_char(self, c: str) -> bool:
        return self.in_block(c) and not any(a <= c <= b for a, b in self._not_word)

    def _replace(self, text: str) -> str:
        for a, b in self.data.get("replace", []):
            text = text.replace(a, b)
        return text

    @cached_property
    def _joiners(self) -> tuple:
        """The joiner rules `ScriptDef::engine_normalize` uses: this file's for its script
        (`joiners`, `zwjForms`), and Malayalam's, which an engine with no other pack has."""
        from mlkb.data.human import MALAYALAM, Joiners

        own = Joiners.of(self.data)
        if self.data.get("script") == "mlym":
            says = "joiners" in self.data or "zwjForms" in self.data
            return ((own,) if own else ()) if says else (MALAYALAM,)
        return (MALAYALAM,) + ((own,) if own else ())

    def normalize(self, text: str) -> str:
        before = self._replace(_engine_normalize(self._replace(text), self._joiners))
        after = self._replace(_engine_normalize(before, self._joiners))
        return before if after == before else after

    def is_word(self, word: str) -> bool:
        """`native.rs` `Lang::is_word`."""
        apostrophe = bool(self.data.get("apostrophe"))
        ok = all(self.is_word_char(c) or c in (ZWJ, ZWNJ) or (c == "'" and apostrophe) for c in word)
        starts = bool(word) and any(a <= word[0] <= b for a, b in self._first)
        lone_virama = bool(self.virama) and len(word) <= 2 and word.endswith(self.virama)
        return (
            ok
            and word.count("'") <= 1
            and starts
            and self._signs_follow_bases(word)
            and not lone_virama
            and len(word) <= self.data["maxChars"]
            and not word.endswith("'")
        )


@cache
def load(lang: str) -> Lang:
    return Lang(json.loads((LANGS / lang / "lang.json").read_text(encoding="utf-8")))


def _engine_normalize(text: str, rules: tuple) -> str:
    """`engine/core/src/normalize.rs` `normalize_with`: NFC, `’` to `'`, and `rules`' joiners."""
    from mlkb.data.human import normalize as engine_normalize

    return engine_normalize(text, rules)


@dataclass(frozen=True)
class HumanPair:
    latin: str
    native: str
    source: str
    count: int = 1  # how many annotators typed it (Dakshina), else 1


def clean(lang: Lang, latin: str, native: str) -> tuple[str, str] | None:
    """The normalised (latin, native) pair, or None when it can't be a training row."""
    latin = latin.strip().lower()
    native = lang.normalize(unicodedata.normalize("NFC", native.strip()))
    if not latin or not latin.isascii() or not latin.isalpha() or not lang.is_word(native):
        return None
    if len(latin) > MAX_CHARS or len(native) > MAX_CHARS:
        return None
    return latin, native


def aksharantar_dir(lang: Lang) -> Path:
    code = lang.data["aksharantar"]
    return RAW / f"aksharantar-{code}" / "extracted"


def aksharantar(lang_id: str, split: str = "train", stats: Counter | None = None) -> list[HumanPair]:
    """Aksharantar rows of `split` whose source the licence audit allows, cleaned; [] for a
    language whose lang.json names no Aksharantar code."""
    lang = load(lang_id)
    if not lang.data.get("aksharantar"):
        return []  # a language with no Aksharantar data: its pairs are its own (`pairs.json`)
    stats = stats if stats is not None else Counter()
    out = []
    path = aksharantar_dir(lang) / f"{lang.data['aksharantar']}_{split}.json"
    if not path.exists():
        raise SystemExit(
            f"{path} is missing. Unzip Aksharantar first: run ./data/build-{lang_id}-text.sh "
            "(docs/make-a-language.md step 3)."
        )
    for row in aksharantar_rows(path):
        source = row.get("source") or "missing"
        if source not in AKSHARANTAR_KEEP:
            stats[f"dropped {source} (licence)"] += 1
            continue
        score = row.get("score")
        if source == "IndicCorp" and score is not None and score < INDICCORP_MIN_SCORE:
            stats["dropped IndicCorp (low mining score)"] += 1
            continue
        pair = clean(lang, row["english word"], row["native word"])
        if not pair:
            stats[f"dropped {source} (not a-z / not a word / too long)"] += 1
            continue
        stats[f"kept {source}"] += 1
        out.append(HumanPair(pair[0], pair[1], source))
    return out


# The file that names a language's own human pairs (`PairSource`), beside its lang.json. Not
# shipped in the pack: only the builder reads it.
PAIRS_FILE = "pairs.json"
SPLITS = ("train", "dev", "test")


@dataclass(frozen=True)
class PairSource:
    """A language's own human pairs, from `data/langs/<id>/pairs.json`:

        {"source": "Dakshina",
         "credit": "Dakshina dataset v1.0 (Google Research), CC BY-SA 4.0",
         "files": {"train": "data/raw/…/xx.train.tsv", "dev": "…", "test": "…"},
         "missing": "how to get the files when one is missing"}

    `source` names the rows (`HumanPair.source`, and the eval set's section); `credit` heads the
    eval set written from the test split; `files` maps a split to a TSV file (see the module's
    docstring), a path from the repository root. Every split is optional: without `dev`,
    validation uses the calibration pairs; without `test`, the held-out set is Aksharantar's."""

    name: str
    credit: str
    files: dict[str, str]
    missing: str = ""

    def path(self, split: str) -> Path:
        return ROOT / self.files[split]


def pair_source(lang_id: str) -> PairSource | None:
    """The language's `pairs.json`, or None when it has none."""
    path = LANGS / lang_id / PAIRS_FILE
    if not path.exists():
        return None
    data = json.loads(path.read_text(encoding="utf-8"))
    files = data.get("files", {})
    unknown = set(files) - set(SPLITS)
    if unknown or not files:
        raise SystemExit(f"{path}: \"files\" maps {', '.join(SPLITS)} to TSV files; got {sorted(files) or 'none'}")
    return PairSource(data["source"], data.get("credit", data["source"]), dict(files), data.get("missing", ""))


def read_pairs_file(path: Path) -> list[tuple[str, str, int]]:
    """(native, latin, count) for every row of a pairs TSV (module docstring), uncleaned."""
    out = []
    for n, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip() or line.startswith("#"):
            continue
        fields = line.split("\t")
        if len(fields) not in (2, 3):
            raise SystemExit(f"{path}:{n}: want native<TAB>romanisation[<TAB>count], got {len(fields)} fields")
        out.append((fields[0], fields[1], int(fields[2]) if len(fields) == 3 else 1))
    return out


def file_pairs(lang_id: str, split: str, stats: Counter | None = None) -> list[HumanPair]:
    """The rows of the language's own pairs file for `split` (train, dev or test), cleaned, each
    with its count; [] for a language without `pairs.json` or without that split."""
    src = pair_source(lang_id)
    if src is None or split not in src.files:
        return []
    lang = load(lang_id)
    stats = stats if stats is not None else Counter()
    path = src.path(split)
    if not path.exists():
        hint = f" {src.missing}" if src.missing else ""
        raise SystemExit(f"{path} is missing (data/langs/{lang_id}/{PAIRS_FILE}).{hint}")
    out = []
    for native, latin, count in read_pairs_file(path):
        pair = clean(lang, latin, native)
        if not pair:
            stats[f"dropped {src.name} {split} (not a-z / not a word / too long)"] += 1
            continue
        stats[f"kept {src.name} {split}"] += 1
        out.append(HumanPair(pair[0], pair[1], src.name, count))
    return out


def has_aksharantar(lang_id: str) -> bool:
    return bool(load(lang_id).data.get("aksharantar"))


def held_out_pairs(lang_id: str, stats: Counter | None = None) -> list[HumanPair]:
    """The held-out eval pairs: Aksharantar's manual test rows, or, for a language without
    Aksharantar data, the test split of its own pairs file."""
    if has_aksharantar(lang_id):
        return manual(aksharantar(lang_id, "test", stats))
    rows = file_pairs(lang_id, "test", stats)
    if not rows:
        raise SystemExit(
            f"{lang_id} has no held-out pairs: lang.json names no Aksharantar code and "
            f"data/langs/{lang_id}/{PAIRS_FILE} no test split. Hold out some typed pairs as \"test\"."
        )
    return rows


def second_test_pairs(lang_id: str) -> list[HumanPair]:
    """The test split of the language's own pairs when the main held-out set is Aksharantar's
    (a second measurement, as Punjabi's Dakshina test); [] otherwise."""
    return file_pairs(lang_id, "test") if has_aksharantar(lang_id) else []


def held_out_words(lang_id: str, test: list[HumanPair]) -> set[str]:
    """Words no training pair may have: the held-out `test` rows' and every word of the
    language's own dev and test splits."""
    return {h.native for h in test} | {h.native for split in ("dev", "test") for h in file_pairs(lang_id, split)}


# Calibration pairs: what `calibrate` fits the romanisation weights to and `pairs` writes as
# `valid-human.tsv`. Aksharantar's manual rows (typed by people, sources `AK-*`) of the valid
# split, when it has any. Some languages' valid splits have none (Marathi's manual rows are all
# in `mar_train.json`); then a slice of the train split's manual rows is held out instead, chosen
# by a hash of the native word (so it is the same in both tools and every run), never a test word.
# A language with no manual rows in either (Telugu's are all in `tel_test.json`) calibrates on the
# valid split's mined IndicCorp rows.
CALIBRATION_SHARE = 0.25
# The mined source that calibrates when a language has no manual rows outside its test split.
MINED_CALIBRATION = "IndicCorp"


def manual(rows: list[HumanPair]) -> list[HumanPair]:
    return [h for h in rows if h.source.startswith("AK-")]


def in_calibration_slice(word: str, share: float = CALIBRATION_SHARE) -> bool:
    h = hashlib.sha256(word.encode("utf-8")).digest()
    return int.from_bytes(h[:4], "big") / 2**32 < share


def calibration_split(
    valid: list[HumanPair], train: list[HumanPair], test_words: set[str], code: str
) -> tuple[list[HumanPair], str]:
    """The calibration pairs and a phrase saying where they came from. Stops when there are none."""
    rows = manual(valid)
    if rows:
        return rows, f"the manual rows of {code}_valid.json"
    rows = [h for h in manual(train) if h.native not in test_words and in_calibration_slice(h.native)]
    if rows:
        return rows, (
            f"a held-out {CALIBRATION_SHARE:.0%} (by word) of the manual rows of {code}_train.json, "
            f"because {code}_valid.json has none"
        )
    # No manual rows outside the test split (Telugu): the valid split's mined IndicCorp rows.
    # They are spellings a mining model matched in text, not ones a person typed for the word, so
    # they say less about habits; the other mined sources are mostly English spellings of names.
    rows = [h for h in valid if h.source == MINED_CALIBRATION and h.native not in test_words]
    if not rows:
        raise SystemExit(
            f"no calibration pairs: Aksharantar's {code}_valid.json and {code}_train.json have no manual (AK-) "
            f"rows left after the licence filter, and {code}_valid.json no mined {MINED_CALIBRATION} rows. Check "
            "data/raw/aksharantar-*/extracted/. A language without human pairs can't be calibrated: copy "
            "romanisation.hand.json to romanisation-weights.json (docs/make-a-language.md step 4)."
        )
    return rows, (
        f"the mined {MINED_CALIBRATION} rows of {code}_valid.json, because neither {code}_valid.json nor "
        f"{code}_train.json has manual rows. They were matched by a model, not typed by people: read the "
        "numbers as rough"
    )


def calibration(
    lang_id: str, test_words: set[str], train: list[HumanPair] | None = None
) -> tuple[list[HumanPair], str]:
    """`calibration_split` for a language's Aksharantar files (`train` if already read); for a
    language with its own train split (`pairs.json`), that split instead (no word of `test_words`)."""
    src = pair_source(lang_id)
    if src is not None and "train" in src.files:
        rows = [h for h in file_pairs(lang_id, "train") if h.native not in test_words]
        return rows, f"{src.name}'s train split ({src.path('train').name}, typed by people)"
    if not has_aksharantar(lang_id):
        raise SystemExit(
            f"no calibration pairs: {lang_id}'s lang.json names no Aksharantar code and "
            f"data/langs/{lang_id}/{PAIRS_FILE} no train split. A language without human pairs can't be "
            "calibrated: copy romanisation.hand.json to romanisation-weights.json (docs/make-a-language.md step 4)."
        )
    valid = aksharantar(lang_id, "valid")
    if not manual(valid) and train is None:
        train = aksharantar(lang_id, "train")
    return calibration_split(valid, train or [], test_words, load(lang_id).data["aksharantar"])
