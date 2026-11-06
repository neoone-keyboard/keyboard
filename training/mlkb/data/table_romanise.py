"""Native script → Latin from a pack's `romanisation.json`, for languages whose romaniser is data.

    uv run python -m mlkb.data.table_romanise --table packs/hi.neopack/romanisation.json \\
        --dakshina data/raw/dakshina/dakshina_dataset_v1.0/hi/lexicons/hi.translit.sampled.train.tsv

`Table.canonical(word)` is the one romanisation the romanised lexicon (`hinglish.fst`) is keyed
by; `Table.skeleton(latin)` collapses the spellings people type inconsistently. lexbuild
(`engine/lexbuild/src/romanisation.rs`) implements both from the same file; the tests check that
the two agree. `variants` samples the spellings people type, for synthetic training pairs.

Malayalam keeps its hand-written romaniser (`romanise.py`, `engine/core/src/romanize.rs`); this
module is for the packs that ship their tables as data (Hindi first).

With `--dakshina`, reports how often the canonical romanisation, and its skeleton, match the
human romanisations of a Dakshina lexicon file (weighted by annotator count).
"""

import argparse
import json
import random
import unicodedata
from collections import Counter
from dataclasses import dataclass, field
from functools import lru_cache
from pathlib import Path
from typing import NamedTuple

ROOT = Path(__file__).resolve().parents[3]

ZWJ, ZWNJ = "‍", "‌"


def _nfc(s: str) -> str:
    return unicodedata.normalize("NFC", s)


class Unit(NamedTuple):
    """One spelled unit of a word.

    kind: "C" a consonant or conjunct piece (its vowel follows as its own unit), "V" a vowel
    (sign or independent), "S" an inherent vowel (schwa) that may be deleted, "M" a modifier
    (anusvara, chandrabindu, visarga), "L" a Latin letter passed through.
    """

    kind: str
    latin: str
    src: str
    single: bool = True  # a C unit: one consonant letter (not a cluster of several)


@dataclass
class Table:
    """A loaded `romanisation.json`."""

    raw: dict
    virama: str = ""
    nukta: str = ""
    inherent: str = "a"
    independent: dict[str, str] = field(default_factory=dict)
    signs: dict[str, str] = field(default_factory=dict)
    nasal_signs: dict[str, str] = field(default_factory=dict)
    consonants: dict[str, str] = field(default_factory=dict)
    clusters: dict[str, str] = field(default_factory=dict)
    modifiers: dict[str, str] = field(default_factory=dict)
    labials: str = ""
    labial_latin: str = "m"
    schwa: dict = field(default_factory=dict)
    skel: dict = field(default_factory=dict)
    block: tuple[str, str] = ("", "")
    not_letters: frozenset[str] = frozenset()

    @classmethod
    def load(cls, path: Path) -> "Table":
        raw = json.loads(Path(path).read_text(encoding="utf-8"))
        n = lambda d: {_nfc(k): v for k, v in d.items()}  # noqa: E731
        consonants = n(raw["consonants"]) | n(raw.get("nuktaConsonants", {}))
        labial = raw.get("anusvaraBeforeLabials", {})
        return cls(
            raw=raw,
            virama=raw["virama"],
            nukta=raw.get("nukta", ""),
            inherent=raw.get("inherentVowel", "a"),
            independent=n(raw["independentVowels"]),
            signs=n(raw["vowelSigns"]),
            nasal_signs=n(raw.get("nasalisedVowelSigns", {})),
            consonants=consonants,
            clusters=n(raw.get("clusters", {})),
            modifiers=n(raw.get("modifiers", {})),
            labials=_nfc(labial.get("letters", "")),
            labial_latin=labial.get("latin", "m"),
            schwa=raw.get("schwa", {}),
            skel=raw.get("skeleton", {}),
            block=tuple(raw["block"]),
            not_letters=frozenset(raw.get("notLetters", [])),
        )

    # --- script -------------------------------------------------------------------------

    def is_native(self, word: str) -> bool:
        """Every character is in the script block (less its punctuation and digits) or a joiner."""
        lo, hi = self.block
        return bool(word) and all(
            (lo <= c <= hi and c not in self.not_letters) or c in (ZWJ, ZWNJ) for c in word
        )

    # --- romanising ---------------------------------------------------------------------

    def _consonant_at(self, chars: list[str], i: int) -> tuple[str, str, int] | None:
        """(letters, latin, length) of the consonant at `i`: a nukta letter first, then a plain one."""
        if i + 1 < len(chars) and chars[i + 1] == self.nukta and chars[i] + self.nukta in self.consonants:
            return chars[i] + self.nukta, self.consonants[chars[i] + self.nukta], 2
        if chars[i] in self.consonants:
            return chars[i], self.consonants[chars[i]], 1
        return None

    def units(self, word: str) -> tuple[Unit, ...]:
        return _units(self, _nfc(word))

    def canonical(self, word: str) -> str:
        """The romanisation `hinglish.fst` is keyed by: भारत → bhaarat, कमरा → kamra."""
        return "".join(u.latin for u in self.units(word))

    def skeleton(self, latin: str) -> str:
        """`latin` collapsed to what people type consistently (see the table's `skeleton`)."""
        return skeleton(self.skel, latin)

    def key(self, word: str) -> str:
        return self.skeleton(self.canonical(word))


def _units(t: Table, word: str) -> tuple[Unit, ...]:
    return _units_cached(id(t), t, word)


_CACHE: dict = {}


def _units_cached(tid: int, t: Table, word: str) -> tuple[Unit, ...]:
    key = (tid, word)
    hit = _CACHE.get(key)
    if hit is not None:
        return hit
    out = _walk(t, word)
    if len(_CACHE) > 500_000:
        _CACHE.clear()
    _CACHE[key] = out
    return out


def _walk(t: Table, word: str, delete_schwas: bool = True) -> tuple[Unit, ...]:
    """The units of `word`, with the schwas Hindi drops left out unless `delete_schwas` is off."""
    chars = [c for c in word if c not in (ZWJ, ZWNJ)]
    n = len(chars)
    units: list[Unit] = []
    i = 0
    while i < n:
        c = chars[i]
        # A conjunct spelled as a unit (क्ष ksh, ज्ञ gy): three characters, consonant virama consonant.
        three = "".join(chars[i : i + 3])
        cons = None
        if len(three) == 3 and three in t.clusters:
            cons = (three, t.clusters[three], 3, False)
        elif (found := t._consonant_at(chars, i)) is not None:
            cons = (*found, True)
        if cons is not None:
            letters, latin, length, single = cons
            j = i + length
            units.append(Unit("C", latin, letters, single))
            sign = chars[j] if j < n else ""
            if sign in t.signs:
                if j + 2 == n and _nasal(t, chars[j + 1]):
                    v = t.nasal_signs.get(sign, t.signs[sign])
                else:
                    v = t.signs[sign]
                units.append(Unit("V", v, sign))
                j += 1
            elif sign == t.virama:
                j += 1  # no vowel: the next consonant joins this one
            else:
                units.append(Unit("S", t.inherent, ""))
            i = j
            continue
        if c in t.independent:
            nasal = i + 2 == n and _nasal(t, chars[i + 1])
            units.append(Unit("V", t.nasal_signs.get(c, t.independent[c]) if nasal else t.independent[c], c))
        elif c in t.signs:  # a stray sign: spell it as its vowel
            units.append(Unit("V", t.signs[c], c))
        elif c in t.modifiers:
            latin = t.modifiers[c]
            nxt = t._consonant_at(chars, i + 1) if i + 1 < n else None
            if c == "ं" and nxt and nxt[0][0] in t.labials:
                latin = t.labial_latin
            units.append(Unit("M", latin, c))
        elif c.isascii() and c.isalpha():
            units.append(Unit("L", c.lower(), c))
        i += 1
    return tuple(_delete_schwas(t, units) if delete_schwas else units)


def _nasal(t: Table, c: str) -> bool:
    """Anusvara or chandrabindu: a modifier spelled as a nasal."""
    return c in t.modifiers and t.modifiers[c] in ("n", "m")


def _delete_schwas(t: Table, units: list[Unit]) -> list[Unit]:
    """Drops the schwas Hindi doesn't pronounce (the table's `schwa`)."""
    rules = t.schwa
    if not rules:
        return units
    keep = [True] * len(units)
    vowels = [k for k, u in enumerate(units) if u.kind in "VS"]
    # Final: the last unit is a schwa after a consonant, in a word of two or more syllables.
    # Kept after a conjunct ending in one of `keepFinalAfterClusterEndingIn` (mitra, kaarya).
    if rules.get("deleteFinal") and units and units[-1].kind == "S" and len(vowels) >= 2:
        last = len(units) - 1
        kept_after = rules.get("keepFinalAfterClusterEndingIn", "")
        if not (_cluster_before(units, last) and units[last - 1].src[-1:] in kept_after):
            keep[-1] = False
    if rules.get("deleteMedial"):
        # Right to left: V C _ C V, each C a single consonant, the right V not dropped.
        for k in range(len(units) - 2, 1, -1):
            u = units[k]
            if u.kind != "S" or not keep[k]:
                continue
            if k + 1 < len(units) and units[k + 1].kind == "M":
                continue  # a nasalised schwa stays (sant, ambar)
            if _cluster_before(units, k):
                continue
            # left: units[k-1] is C, units[k-2] a vowel that is kept (or a modifier after one)
            if units[k - 1].kind != "C" or units[k - 2].kind not in "VSM" or not keep[k - 2]:
                continue
            # right: a single consonant, then a kept vowel
            if k + 2 >= len(units) or units[k + 1].kind != "C" or not units[k + 1].single:
                continue
            if units[k + 2].kind not in "VS" or not keep[k + 2]:
                continue
            keep[k] = False
    return [u for u, k in zip(units, keep, strict=True) if k]


def _cluster_before(units: list[Unit], k: int) -> bool:
    """Whether the consonant(s) before the vowel at `k` are a cluster (two C units, or a conjunct)."""
    if k < 1 or units[k - 1].kind != "C":
        return False
    return not units[k - 1].single or (k >= 2 and units[k - 2].kind == "C")


# --- skeleton -----------------------------------------------------------------------------

VOWELS = frozenset("aeiou")


@lru_cache(maxsize=64)
def _rewrites(items: tuple[tuple[str, str], ...]) -> tuple[int, dict[str, str]]:
    table = dict(items)
    return max((len(k) for k in table), default=1), table


def skeleton(rules: dict, latin: str) -> str:
    """See `Table.skeleton`."""
    s = "".join(c for c in latin.lower() if "a" <= c <= "z")
    if rules.get("dropTrailingHAfterVowel") and len(s) >= 3 and s[-1] == "h" and s[-2] in VOWELS:
        s = s[:-1]
    longest, rewrites = _rewrites(tuple(sorted(rules.get("rewrites", {}).items())))
    singles = rules.get("singles", {})
    out: list[str] = []
    i = 0
    while i < len(s):
        for size in range(min(longest, len(s) - i), 1, -1):
            sym = rewrites.get(s[i : i + size])
            if sym is not None:
                i += size
                break
        else:
            sym = singles.get(s[i], s[i])
            i += 1
        for ch in sym:
            if not (rules.get("collapseRuns") and out and out[-1] == ch):
                out.append(ch)
    # A vowel between two consonants that people write or leave out (samajhna, samjhna).
    drop = rules.get("dropBetweenConsonants", "")
    if drop:
        out = [
            ch
            for k, ch in enumerate(out)
            if not (
                ch in drop
                and 0 < k < len(out) - 1
                and out[k - 1] not in VOWELS
                and out[k + 1] not in VOWELS
            )
        ]
    ending = "".join(out)
    finals = sorted(rules.get("finalRewrites", {}).items(), key=lambda kv: (-len(kv[0]), kv[0]))
    for end, sym in finals:
        if len(ending) > len(end) and ending.endswith(end):
            out = list(ending[: -len(end)] + sym)
            break
    strip = rules.get("stripFinal", [])
    if len(out) >= 3 and out[-1] in strip and out[-2] not in VOWELS:
        out.pop()
    return "".join(out)


# --- sampled spellings -------------------------------------------------------------------


@dataclass(frozen=True)
class Variation:
    """How often each spelling change fires when sampling what people type. Set by hand from the
    Dakshina train split (see `--dakshina`)."""

    # Canonical vowels are short. A long one written long: ा aa, ी ee, ू oo (and the independent forms).
    long_a_long: float = 0.3
    long_i_long: float = 0.3
    long_u_long: float = 0.3
    i_as_ee: float = 0.03  # short ि written ee
    u_as_oo: float = 0.02
    keep_schwa: float = 0.15  # a deleted schwa written anyway (samajhana)
    final_schwa: float = 0.05  # a final deleted schwa written (bhaarata)
    aspiration_drop: float = 0.08
    sh_as_s: float = 0.15
    v_as_w: float = 0.25
    z_as_j: float = 0.35
    f_as_ph: float = 0.2
    q_as_k: float = 0.6
    nasal_drop: float = 0.25  # anusvara / chandrabindu not written
    nasal_final_drop: float = 0.4
    ri_as_ru: float = 0.05
    e_as_ai: float = 0.05
    ai_as_e: float = 0.12
    au_as_o: float = 0.1
    ksh_as_x: float = 0.06
    gy_as_gn: float = 0.15
    geminate_single: float = 0.15
    ch_as_c: float = 0.04
    chh_as_ch: float = 0.4
    y_vowel_e: float = 0.1  # final -ye written -e (gaye → gae)
    variant_counts: tuple[float, float, float, float] = (0.4, 0.3, 0.2, 0.1)
    canonical_share: float = 0.15


VARIATION = Variation()

_ASPIRATES = {"kh": "k", "gh": "g", "chh": "ch", "jh": "j", "th": "t", "dh": "d", "ph": "p", "bh": "b"}


def _unit_options(t: Table, units: tuple[Unit, ...], k: int, v: Variation) -> list[tuple[str, float]]:
    """Weighted spellings of unit `k`, canonical first."""
    u = units[k]
    opts: list[tuple[str, float]] = [(u.latin, 1.0)]

    def alt(spelling: str, p: float) -> None:
        if p > 0 and spelling != u.latin:
            opts[0] = (opts[0][0], opts[0][1] - p)
            opts.append((spelling, p))

    if u.kind == "V":
        match u.src:
            case "ा" | "आ":
                alt("aa", v.long_a_long)
            case "ी" | "ई":
                alt("ee", v.long_i_long)
            case "ू" | "ऊ":
                alt("oo", v.long_u_long)
            case "ि" | "इ":
                alt("ee", v.i_as_ee)
            case "ु" | "उ":
                alt("oo", v.u_as_oo)
        match u.latin:
            case "ri":
                alt("ru", v.ri_as_ru)
            case "e":
                alt("ai", v.e_as_ai)
            case "ai":
                alt("e", v.ai_as_e)
            case "au":
                alt("o", v.au_as_o)
    elif u.kind == "C":
        latin = u.latin
        # A doubled consonant (क्क, त्त): written once.
        prev_same = k >= 1 and units[k - 1].kind == "C" and units[k - 1].latin == latin
        if prev_same:
            alt("", v.geminate_single)
        if latin in _ASPIRATES:
            alt(_ASPIRATES[latin], v.chh_as_ch if latin == "chh" else v.aspiration_drop)
        match latin:
            case "sh":
                alt("s", v.sh_as_s)
            case "v":
                alt("w", v.v_as_w)
            case "z":
                alt("j", v.z_as_j)
            case "f":
                alt("ph", v.f_as_ph)
            case "q":
                alt("k", v.q_as_k)
            case "ksh":
                alt("x", v.ksh_as_x)
            case "gy":
                alt("gn", v.gy_as_gn)
            case "ch":
                alt("c", v.ch_as_c)
    elif u.kind == "M" and u.latin in ("n", "m"):
        alt("", v.nasal_final_drop if k == len(units) - 1 else v.nasal_drop)
    return opts


def _pick(opts: list[tuple[str, float]], rng: random.Random) -> str:
    x = rng.random()
    acc = 0.0
    for spelling, w in opts:
        acc += w
        if x < acc:
            return spelling
    return opts[-1][0]


def variants(t: Table, word: str, rng: random.Random, v: Variation = VARIATION) -> list[str]:
    """1–4 distinct Latin spellings of `word` the way people type it; [] when nothing to romanise."""
    word = _nfc(word)
    units = t.units(word)
    # The medial schwas written in full (samajhana), the final one as in the canonical spelling.
    full = _walk(t, word, delete_schwas=False)
    if full and full[-1].kind == "S" and units and units[-1].kind != "S":
        full = full[:-1]
    canon = "".join(u.latin for u in units)
    if not canon:
        return []
    want = rng.choices((1, 2, 3, 4), weights=v.variant_counts)[0]
    out = [canon] if rng.random() < v.canonical_share else []
    for _ in range(3 * want):
        if len(out) >= want:
            break
        # Schwa: usually as deleted, sometimes written in full.
        base = full if rng.random() < v.keep_schwa else units
        picked = [_pick(_unit_options(t, base, k, v), rng) for k in range(len(base))]
        s = "".join(picked)
        if s.endswith("ye") and rng.random() < v.y_vowel_e:
            s = s[:-2] + "e"
        elif units and units[-1].kind == "C" and rng.random() < v.final_schwa:
            s += "a"  # a dropped final schwa written (bharata)
        if s and s.isascii() and s.isalpha() and s not in out:
            out.append(s)
    return out or [canon]


# --- Dakshina check ----------------------------------------------------------------------


def dakshina_rows(path: Path) -> list[tuple[str, str, int]]:
    out = []
    for line in Path(path).read_text(encoding="utf-8").splitlines():
        native, latin, count = line.split("\t")
        out.append((_nfc(native), latin.lower(), int(count)))
    return out


def match_rates(t: Table, rows: list[tuple[str, str, int]]) -> dict[str, float]:
    """Weighted share of human romanisations equal to the canonical one, with the same skeleton,
    and whose skeleton starts the word's skeleton or the other way round (a prefix lookup finds it),
    plus the share of words whose most common romanisation is the canonical one."""
    total = exact = same = prefix = 0
    best: dict[str, Counter] = {}
    for native, latin, count in rows:
        canon = t.canonical(native)
        a, b = t.skeleton(latin), t.skeleton(canon)
        total += count
        exact += count * (latin == canon)
        same += count * (a == b)
        prefix += count * (bool(a) and (b.startswith(a) or a.startswith(b)))
        best.setdefault(native, Counter())[latin] += count
    top = sum(t.canonical(w) == c.most_common(1)[0][0] for w, c in best.items())
    return {
        "pairs": len(rows),
        "exact": exact / total,
        "skeleton": same / total,
        "skeleton prefix": prefix / total,
        "words": len(best),
        "top spelling is canonical": top / len(best),
    }


def main(argv: list[str] | None = None) -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--table", type=Path, default=ROOT / "packs/hi.neopack/romanisation.json")
    p.add_argument("--dakshina", type=Path, action="append", default=[])
    p.add_argument("--misses", type=int, default=0, help="print this many skeleton misses")
    p.add_argument("words", nargs="*")
    args = p.parse_args(argv)
    t = Table.load(args.table)
    for w in args.words:
        print(w, t.canonical(w), t.key(w), variants(t, w, random.Random(0)))
    for path in args.dakshina:
        rows = dakshina_rows(path)
        print(path.name, {k: round(v, 4) if isinstance(v, float) else v for k, v in match_rates(t, rows).items()})
        if args.misses:
            rng = random.Random(0)
            misses = [(n, l, t.canonical(n)) for n, l, _ in rows if t.skeleton(l) != t.key(n)]
            for n, l, c in rng.sample(misses, min(args.misses, len(misses))):
                print(f"  {n}\t{l}\t{c}\t{t.skeleton(l)} != {t.skeleton(c)}")


if __name__ == "__main__":
    main()
