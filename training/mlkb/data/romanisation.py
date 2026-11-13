"""A pack's `romanisation.json` (format 1), exactly as the engine reads it.

    uv run python -m mlkb.data.romanisation --lang hi [--misses 20] [words…]

The format and its walk and skeleton are specified in
`docs/design/romanisation-format.md`. `Romanisation.key(word)` is what
the romanised lexicon (`hinglish.fst`) is keyed by: the skeleton of the walk. lexbuild
(`engine/lexbuild/src/romanisation.rs`) implements the same; the fixture written by
`mlkb.data.romanisation_fixture` keeps the two in step, and phase 2's engine computes the same
skeleton of what was typed.

Prints how often the human romanisations of the Dakshina lexicon meet the walk's skeleton.
"""

import argparse
import json
import random
import unicodedata
from pathlib import Path

from mlkb import langs
from mlkb.data.table_romanise import dakshina_rows, match_rates

ROOT = Path(__file__).resolve().parents[3]
VOWELS = frozenset("aeiou")
ZWJ, ZWNJ = "‍", "‌"


def _nfc(s: str) -> str:
    return unicodedata.normalize("NFC", s)


class Romanisation:
    def __init__(self, raw: dict):
        n = lambda d: {_nfc(k): v for k, v in d.items()}  # noqa: E731
        self.raw = raw
        self.script = raw["script"]
        self.virama = _nfc(raw["virama"])
        self.inherent = raw["inherent"]
        self.end_virama = raw["atWordEnd"]["virama"]
        self.end_inherent = raw["atWordEnd"]["inherent"]
        self.independent = n(raw["independentVowels"])
        self.signs = n(raw["vowelSigns"])
        self.consonants = n(raw["consonants"])
        self.finals = n(raw["finals"])
        self.clusters = n(raw["clusters"])
        # Signs of `finals` people leave unwritten at a word's end (Hindi's anusvara and
        # chandrabindu: नहीं is nahin and nahi).
        self.optional_finals = [_nfc(s) for s in raw.get("optionalFinals", [])]
        self.skel = raw["skeleton"]
        self.pairs = [(a, b) for a, b in self.skel["pairs"]]

    @classmethod
    def load(cls, path: Path) -> "Romanisation":
        return cls(json.loads(Path(path).read_text(encoding="utf-8")))

    @staticmethod
    def _longest(table: dict[str, str], chars: str, i: int) -> tuple[str, str] | None:
        """The longest key of `table` that starts at `chars[i]`, and its Latin."""
        for size in range(min(4, len(chars) - i), 0, -1):
            key = chars[i : i + size]
            if key in table:
                return key, table[key]
        return None

    def canonical(self, word: str) -> str:
        """The walk (format spec, "The walk")."""
        chars = "".join(c for c in _nfc(word) if c not in (ZWJ, ZWNJ))
        n = len(chars)
        out: list[str] = []
        i = 0
        while i < n:
            found = self._longest(self.clusters, chars, i) or self._longest(self.consonants, chars, i)
            if found:
                key, latin = found
                out.append(latin)
                i += len(key)
                sign = self._longest(self.signs, chars, i) if i < n else None
                if sign:
                    out.append(sign[1])
                    i += len(sign[0])
                elif chars.startswith(self.virama, i):
                    i += len(self.virama)
                    if i >= n:
                        out.append(self.end_virama)
                else:
                    out.append(self.end_inherent if i >= n else self.inherent)
                continue
            found = self._longest(self.independent, chars, i) or self._longest(self.finals, chars, i)
            if found:
                out.append(found[1])
                i += len(found[0])
                continue
            c = chars[i]
            if c.isascii() and c.isalpha():
                out.append(c.lower())
            i += 1
        return "".join(out)

    def skeleton(self, latin: str, drop_trailing: bool = True) -> str:
        """The skeleton (format spec, "The skeleton"); `drop_trailing` False leaves out its
        dropTrailing step (for `keys`)."""
        s = "".join(c for c in latin.lower() if c.isascii() and c.isalpha())
        drop = self.skel["dropTrailing"]
        if (
            drop_trailing
            and len(s) >= drop["minLength"]
            and s
            and s[-1] in drop["letters"]
            and (not drop["afterVowel"] or (len(s) >= 2 and s[-2] in VOWELS))
        ):
            s = s[:-1]
        out: list[str] = []
        singles = self.skel["singles"]
        i = 0
        while i < len(s):
            for pat, sym in self.pairs:
                if s.startswith(pat, i):
                    i += len(pat)
                    break
            else:
                sym = singles.get(s[i], s[i])
                i += 1
            for ch in sym:
                if not (self.skel["collapseDoubles"] and out and out[-1] == ch):
                    out.append(ch)
        # A vowel people write or leave out between consonants (samajhana, samjhna): all at once.
        between = self.skel.get("dropBetweenConsonants", "")
        if between and len(out) >= 3:
            out = [
                ch
                for k, ch in enumerate(out)
                if not (ch in between and 0 < k < len(out) - 1 and out[k - 1] not in VOWELS and out[k + 1] not in VOWELS)
            ]
        final = self.skel["dropFinalVowel"]
        # `minLength` is the length before the vowel is dropped (ente -> ent with 3).
        if (
            len(out) >= max(final["minLength"], 2)
            and out[-1] in final["vowels"]
            and (not final["afterConsonant"] or out[-2] not in VOWELS)
        ):
            out.pop()
        return "".join(out)

    def key(self, word: str) -> str:
        return self.skeleton(self.canonical(word))

    def canonical_short(self, word: str) -> str | None:
        """The walk without the word's last sign, when `optionalFinals` names it (नहीं: nahi);
        None for any other word. As the engine's `Romanisation::romanize_short`."""
        chars = "".join(c for c in _nfc(word) if c not in (ZWJ, ZWNJ))
        sign = next((s for s in self.optional_finals if chars.endswith(s)), None)
        # Only after a written vowel: after a bare consonant the sign is always written.
        before = chars[: -len(sign)] if sign else ""
        if not any(before.endswith(v) for v in (*self.signs, *self.independent)):
            return None
        latin = self.finals.get(sign) if sign else None
        whole = self.canonical(word)
        if not latin or not whole.endswith(latin):
            return None
        return whole[: -len(latin)] or None

    def keys(self, word: str) -> list[str]:
        """Every skeleton the romanised lexicon keys the word by: `key`, then that of its
        shorter spelling when it has one and it is another (नहीं: nhin, nhi). Empty for a word
        without a skeleton. As the engine's `Romanisation::word_skeletons`."""
        whole = self.key(word)
        if not whole:
            return []
        short = self.canonical_short(word)
        other = self.skeleton(short) if short else ""
        keys = [whole, other] if other and other != whole else [whole]
        # dropTrailing.alsoKept: जगह (jagah) is jga, and jgh with its h kept.
        if self.skel["dropTrailing"].get("alsoKept") and self.skel.get("dropBetweenConsonants"):
            kept = self.skeleton(self.canonical(word), drop_trailing=False)
            if kept != whole and 3 <= len(kept) <= len(whole):
                keys.append(kept)
        return keys


def main(argv: list[str] | None = None) -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--lang", default="hi")
    p.add_argument("--misses", type=int, default=0)
    p.add_argument("words", nargs="*")
    args = p.parse_args(argv)
    lang = langs.load(args.lang)
    r = Romanisation.load(lang.pack / "romanisation.json")
    for w in args.words:
        print(w, r.canonical(w), r.key(w))
    for split in ("train", "test"):
        rows = dakshina_rows(lang.dakshina_lexicons / f"{lang.dakshina}.translit.sampled.{split}.tsv")
        rates = match_rates(r, rows)
        print(split, {k: round(v, 4) if isinstance(v, float) else v for k, v in rates.items()})
        if args.misses and split == "test":
            misses = [(n, la, r.canonical(n)) for n, la, _ in rows if r.skeleton(la) != r.key(n)]
            for n, la, c in random.Random(0).sample(misses, min(args.misses, len(misses))):
                print(f"  {n}\t{la}\t{c}\t{r.skeleton(la)} != {r.skeleton(c)}")


if __name__ == "__main__":
    main()
