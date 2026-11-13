"""Native word -> Latin with weighted spellings, driven by tables instead of code: the training
side of a pack's romanisation. (The pack's own `romanisation.json` is the format-1 file of
`docs/design/romanisation-format.md`, exported from these tables'
canonical spellings by `mlkb.langs.format1`; that is what the engine and the romanised lookup
keys use.)

    from mlkb.langs.romanisation import Tables
    t = Tables.load("as")            # data/langs/as/romanisation-weights.json
    t.canonical("অসম")               # 'oxom'
    t.variants("অসম", rng)           # ['oxom', 'asom', ...]
    t.skeleton("axom")               # the lookup key both sides collapse to

The same walk as `engine/core/src/romanize.rs` and `mlkb.data.romanise` do for Malayalam,
with the letters in tables (`romanisation.json` holds what `romanize.rs` had as code;
docs/design/romanisation-format.md). A word splits into slots, each with weighted spellings, canonical first:

- a consonant cluster (consonants joined by viramas) becomes one slot per piece: a listed
  conjunct (`clusters`, ক্ষ), a doubled letter (ক্ক: doubled or, `geminateSingle` of the time,
  single), or one letter, from `afterVirama` when it follows a virama (the -ya and -wa
  signs), else from `inCluster` when it is in a cluster (স in স্ত is "s", not "x"), else
  `consonants`;
- then the vowel it carries: a vowel sign; an apostrophe (Assamese ক'ত: `apostropheVowel`);
  a virama at the end of the word (nothing); or the inherent vowel, `inherent.final` at the
  end of the word after a lone consonant, `finalAfterCluster` after a cluster, `only` when
  the word is that one consonant, else `medial`;
- independent vowels, `finals` (anusvara, candrabindu, visarga, khanda ta) and stray signs
  are one slot each; anything else is skipped.

`skeleton` collapses the spellings people don't type consistently (see the `skeleton` table);
the romanised lookup keys every native word by the skeleton of its canonical romanisation.
"""

import json
import random
from dataclasses import dataclass
from functools import cached_property, lru_cache
from pathlib import Path
from typing import NamedTuple

ROOT = Path(__file__).resolve().parents[3]
LANGS = ROOT / "data/langs"

Options = tuple[tuple[str, float], ...]


class Slot(NamedTuple):
    """One unit of a word: its spellings (canonical first) with weights, and where they come from."""

    kind: str  # "C" consonant piece, "V" vowel (sign, independent, inherent), "F" final letter
    options: Options
    table: str  # which table (and context) the options are from, e.g. "consonants", "inherent.medial"
    key: str  # the letters in that table

    @property
    def canonical(self) -> str:
        return self.options[0][0]


def _options(raw) -> Options:
    return tuple((str(s), float(w)) for s, w in raw)


@dataclass(frozen=True, eq=False)
class Tables:
    """A language's weighted romanisation tables (`romanisation-weights.json` or `.hand.json`)."""

    data: dict
    path: Path | None = None

    @classmethod
    def load(cls, lang_or_path: str | Path) -> "Tables":
        path = Path(lang_or_path)
        if not path.suffix:
            path = LANGS / str(lang_or_path) / "romanisation-weights.json"
        return cls(json.loads(path.read_text(encoding="utf-8")), path)

    def table(self, name: str) -> dict[str, Options]:
        return self._tables[name]

    @cached_property
    def _tables(self) -> dict[str, dict[str, Options]]:
        out = {}
        for name in ("independentVowels", "vowelSigns", "consonants", "inCluster", "afterVirama", "clusters", "finals"):
            out[name] = {k: _options(v) for k, v in self.data.get(name, {}).items()}
        out["inherent"] = {k: _options(v) for k, v in self.data["inherent"].items()}
        return out

    @cached_property
    def virama(self) -> str:
        return self.data["virama"]

    @cached_property
    def apostrophe(self) -> str:
        return self.data.get("apostrophe", "")

    @cached_property
    def _consonant_keys(self) -> list[str]:
        return sorted(self.table("consonants"), key=len, reverse=True)

    @cached_property
    def _cluster_keys(self) -> list[str]:
        return sorted(self.table("clusters"), key=len, reverse=True)

    @cached_property
    def _independent_keys(self) -> list[str]:
        return sorted(self.table("independentVowels"), key=len, reverse=True)

    def _independent_at(self, word: str, i: int) -> str | None:
        """The independent-vowel key at `word[i:]`, longest first."""
        for key in self._independent_keys:
            if word.startswith(key, i):
                return key
        return None

    def _cluster_at(self, word: str, i: int) -> str | None:
        """The `clusters` key at `word[i:]`, longest first (a script without a virama)."""
        for key in self._cluster_keys:
            if word.startswith(key, i):
                return key
        return None

    def _cluster_parts(self, key: str) -> list[str]:
        """A `clusters` key's consonants: split at the virama, or, in a script without one, the
        whole key (its letters stand side by side, matched by `_cluster_at`)."""
        return key.split(self.virama) if self.virama else [key]

    def _consonant_at(self, word: str, i: int) -> str | None:
        """The consonant table key at `word[i:]` (a letter, or a letter + nukta), longest first."""
        for key in self._consonant_keys:
            if word.startswith(key, i):
                return key
        return None

    # The walk ---------------------------------------------------------------------------

    def slots(self, word: str) -> tuple[Slot, ...]:
        return _slots(self, word)

    def _walk(self, word: str) -> tuple[Slot, ...]:
        t = self._tables
        n = len(word)
        out: list[Slot] = []
        i = 0
        while i < n:
            c = word[i]
            if c in t["finals"]:
                out.append(Slot("F", t["finals"][c], "finals", c))
                i += 1
                continue
            # A script without a virama (Urdu's) writes its clusters as letters side by side
            # (بھ, b + do-chashmi he): a `clusters` key is then that text, matched longest first
            # where it stands, as the format-1 walk matches every cluster.
            joined = None if self.virama else self._cluster_at(word, i)
            first = joined or self._consonant_at(word, i)
            if first is not None:
                # The cluster: consonant (virama consonant)*.
                cluster = [first]
                j = i + len(first)
                while self.virama and j + 1 < n and word[j] == self.virama:
                    nxt = self._consonant_at(word, j + 1)
                    if nxt is None:
                        break
                    cluster.append(nxt)
                    j += 1 + len(nxt)
                if joined is not None:
                    out.append(Slot("C", t["clusters"][joined], "clusters", joined))
                else:
                    out.extend(self._pieces(cluster))
                # The vowel it carries.
                if j < n and word[j] in t["vowelSigns"]:
                    out.append(Slot("V", t["vowelSigns"][word[j]], "vowelSigns", word[j]))
                    j += 1
                elif j < n and self.apostrophe and word[j] == self.apostrophe:
                    out.append(Slot("V", _options(self.data["apostropheVowel"]), "apostropheVowel", "'"))
                    j += 1
                elif self.virama and j < n and word[j] == self.virama:
                    j += 1  # a virama with nothing after it: no vowel
                else:
                    at_end = j >= n or all(ch in "‌‍" for ch in word[j:])
                    if at_end and i == 0 and len(cluster) == 1:
                        ctx = "only"
                    elif at_end:
                        ctx = "final" if len(cluster) == 1 else "finalAfterCluster"
                    else:
                        ctx = "medial"
                    out.append(Slot("V", t["inherent"][ctx], f"inherent.{ctx}", ""))
                i = j
                continue
            vowel = self._independent_at(word, i)
            if vowel is not None:
                # A key may be several code points (Bengali অ্যা, the vowel of "and"), as in the
                # format-1 walk: the longest one that matches.
                out.append(Slot("V", t["independentVowels"][vowel], "independentVowels", vowel))
                i += len(vowel)
                continue
            if c in t["vowelSigns"]:  # a stray sign: spelled as its vowel
                out.append(Slot("V", t["vowelSigns"][c], "vowelSigns", c))
            elif c.isascii() and c.isalpha():
                out.append(Slot("F", ((c.lower(), 1.0),), "latin", c.lower()))
            i += 1
        if "schwaDeleted" in t["inherent"]:
            self._delete_schwas(out)
        return tuple(out)

    def _delete_schwas(self, slots: list[Slot]) -> None:
        """Marks the medial inherent vowels that are not pronounced: right to left, one between
        a vowel + lone consonant and a lone consonant + vowel (V C _ C V), as in ৰাজনীতি
        "rajniti", চৰকাৰ "sorkar". Going right to left, a deleted vowel no longer counts as the
        vowel after an earlier one (উন্নয়নমূলক "unnoyonmulok", not "unnoynomulok")."""
        deleted = self._tables["inherent"]["schwaDeleted"]

        def vowel(k: int) -> bool:
            # A deleted vowel is no vowel, whatever its weights say people type.
            s = slots[k]
            return s.kind == "V" and s.canonical != "" and s.table != "inherent.schwaDeleted"

        def lone(k: int) -> bool:
            s = slots[k]
            return s.kind == "C" and s.table in ("consonants", "inCluster", "afterVirama")

        for k in range(len(slots) - 3, 1, -1):
            s = slots[k]
            if s.table != "inherent.medial":
                continue
            # The consonant before must be lone (not the end of a cluster): V C _ .
            if not (lone(k - 1) and vowel(k - 2)) or slots[k - 1].table != "consonants":
                continue
            if lone(k + 1) and slots[k + 1].table == "consonants" and vowel(k + 2):
                slots[k] = Slot("V", deleted, "inherent.schwaDeleted", "")

    def _pieces(self, cluster: list[str]) -> list[Slot]:
        t = self._tables
        out: list[Slot] = []
        k = 0
        in_cluster = len(cluster) > 1
        while k < len(cluster):
            joined = None
            for key in self._cluster_keys:
                parts = self._cluster_parts(key)
                if cluster[k : k + len(parts)] == parts:
                    joined = key
                    break
            if joined is not None:
                out.append(Slot("C", t["clusters"][joined], "clusters", joined))
                k += len(self._cluster_parts(joined))
                continue
            letter = cluster[k]
            after_virama = k > 0
            if k + 1 < len(cluster) and cluster[k + 1] == letter:
                out.append(Slot("C", self._geminate(letter), "geminate", letter))
                k += 2
                continue
            if after_virama and letter in t["afterVirama"]:
                out.append(Slot("C", t["afterVirama"][letter], "afterVirama", letter))
            elif in_cluster and letter in t["inCluster"]:
                out.append(Slot("C", t["inCluster"][letter], "inCluster", letter))
            else:
                out.append(Slot("C", t["consonants"][letter], "consonants", letter))
            k += 1
        return out

    def _geminate(self, letter: str) -> Options:
        g = float(self.data.get("geminateSingle", 0.0))
        out: list[tuple[str, float]] = []
        for latin, w in self.table("consonants")[letter]:
            doubled = latin[0] + latin if latin else latin
            out += [(doubled, w * (1 - g)), (latin, w * g)]
        return tuple(out)

    # Spellings --------------------------------------------------------------------------

    def canonical(self, word: str) -> str:
        """The canonical romanisation: the first spelling of every slot."""
        return "".join(s.canonical for s in self.slots(word))

    def sample(self, slots: tuple[Slot, ...], rng: random.Random) -> str:
        """One random spelling of a word's slots."""
        out = []
        for s in slots:
            if len(s.options) == 1:
                out.append(s.options[0][0])
                continue
            x = rng.random() * sum(w for _, w in s.options)
            for spelling, w in s.options:
                x -= w
                if x < 0:
                    break
            out.append(spelling)
        return "".join(out)

    def variants(self, word: str, rng: random.Random) -> list[str]:
        """1-4 distinct Latin spellings of `word`, drawn with `rng`, weighted by how people type."""
        slots = self.slots(word)
        canon = "".join(s.canonical for s in slots)
        if not canon:
            return []
        want = rng.choices((1, 2, 3, 4), weights=self.data.get("variantCounts", (0.4, 0.3, 0.2, 0.1)))[0]
        out = [canon] if rng.random() < self.data.get("canonicalShare", 0.1) else []
        for _ in range(3 * want):
            if len(out) >= want:
                break
            v = self.sample(slots, rng)
            if v and v not in out:
                out.append(v)
        return out or [canon]

    # The skeleton -----------------------------------------------------------------------

    @cached_property
    def _skeleton(self) -> tuple[list[tuple[str, str]], dict[str, str], str, str]:
        sk = self.data["skeleton"]
        replace = sorted(((a, b) for a, b in sk.get("replace", [])), key=lambda p: len(p[0]), reverse=True)
        return replace, dict(sk.get("letters", {})), sk.get("vowels", "aeiou"), sk.get("dropFinal", "")

    def skeleton(self, latin: str) -> str:
        return _skeleton(self, latin)

    def _skeleton_uncached(self, latin: str) -> str:
        replace, letters, vowels, drop_final = self._skeleton
        sk = self.data["skeleton"]
        s = "".join(c for c in latin.lower() if "a" <= c <= "z")
        # The format's optional steps 2 and 5 (`dropTrailing`, `dropBetweenConsonants`), written
        # in the hand file as the pack's file has them; `format1.export` copies them.
        trail = sk.get("dropTrailing")
        if (
            trail
            and len(s) >= max(trail.get("minLength", 3), 1)
            and s[-1] in trail["letters"]
            and (not trail.get("afterVowel", True) or (len(s) >= 2 and s[-2] in "aeiou"))
        ):
            s = s[:-1]
        out: list[str] = []
        i = 0
        while i < len(s):
            for key, symbols in replace:
                if s.startswith(key, i):
                    i += len(key)
                    break
            else:
                symbols = letters.get(s[i], s[i])
                i += 1
            for sym in symbols:
                if not out or out[-1] != sym:
                    out.append(sym)
        out = _drop_between_consonants(out, sk.get("dropBetweenConsonants", ""))
        if len(out) >= 2 and out[-1] in drop_final and out[-2] not in vowels:
            out.pop()
        return "".join(out)

    def key(self, word: str) -> str:
        """The romanised lookup key of a native word: the skeleton of its canonical romanisation."""
        return self.skeleton(self.canonical(word))


def _drop_between_consonants(out: list[str], letters: str) -> list[str]:
    """The format's skeleton step 5: every letter of `letters` with a consonant on both sides
    goes, all at once (Hindi "a": samajhana, samjhna -> smjna). As `romanisation.rs` does it."""
    if not letters or len(out) < 3:
        return out
    last = len(out) - 1
    return [
        c for k, c in enumerate(out)
        if not (c in letters and 0 < k < last and out[k - 1] not in "aeiou" and out[k + 1] not in "aeiou")
    ]


@lru_cache(maxsize=500_000)
def _slots(tables: Tables, word: str) -> tuple[Slot, ...]:
    return tables._walk(word)


@lru_cache(maxsize=500_000)
def _skeleton(tables: Tables, latin: str) -> str:
    return tables._skeleton_uncached(latin)
