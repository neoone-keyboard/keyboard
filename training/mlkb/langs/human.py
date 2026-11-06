"""A pack language's config (`data/langs/<id>/lang.json`), its normal form, and its human pairs.

`Lang.normalize` is `engine/lexbuild/src/native.rs` `Lang::normalize` in Python: the config's
`replace` pairs, then the engine's normal form (NFC, `’` to `'`, joiners dropped unless a
virama + joiner + letter of the block: none outside Malayalam), then `replace` again. Training
pairs are normalised with it, so they spell words as the lexicon does.

Aksharantar (`data/raw/aksharantar-<code>/extracted/<code>_{train,valid,test}.json`, see
`data/manifests/aksharantar-asm.json`) is filtered as for Malayalam (`mlkb.data.human`): rows
whose source is Dakshina or Existing are dropped, mined IndicCorp rows below the mining score
cut too.
"""

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
        a, b = self.data["block"]
        return _cp(a), _cp(b)

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
        return _cp(self.data["virama"])

    def in_block(self, c: str) -> bool:
        return self.block[0] <= c <= self.block[1]

    def is_word_char(self, c: str) -> bool:
        return self.in_block(c) and not any(a <= c <= b for a, b in self._not_word)

    def _replace(self, text: str) -> str:
        for a, b in self.data.get("replace", []):
            text = text.replace(a, b)
        return text

    def normalize(self, text: str) -> str:
        before = self._replace(_engine_normalize(self._replace(text)))
        after = self._replace(_engine_normalize(before))
        return before if after == before else after

    def is_word(self, word: str) -> bool:
        """`native.rs` `Lang::is_word`."""
        apostrophe = bool(self.data.get("apostrophe"))
        ok = all(self.is_word_char(c) or c in (ZWJ, ZWNJ) or (c == "'" and apostrophe) for c in word)
        starts = bool(word) and any(a <= word[0] <= b for a, b in self._first)
        lone_virama = len(word) <= 2 and word.endswith(self.virama)
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


def _engine_normalize(text: str) -> str:
    """`engine/core/src/normalize.rs` outside Malayalam: NFC, `’` to `'`, joiners dropped."""
    from mlkb.data.human import normalize as malayalam_normalize

    return malayalam_normalize(text)


@dataclass(frozen=True)
class HumanPair:
    latin: str
    native: str
    source: str


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
    """Aksharantar rows of `split` whose source the licence audit allows, cleaned."""
    lang = load(lang_id)
    stats = stats if stats is not None else Counter()
    out = []
    path = aksharantar_dir(lang) / f"{lang.data['aksharantar']}_{split}.json"
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
