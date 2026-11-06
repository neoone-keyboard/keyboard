"""The pack file `romanisation.json`, format 1 (docs/design/2026-11-06-m6-romanisation-format.md):
the walk and skeleton exactly as the spec says the engine computes them, and the export of a
language's weighted tables (`romanisation.Tables`) to that format.

    uv run python -m mlkb.langs.format1 --lang as      # writes data/langs/as/romanisation.json

The export takes each table's canonical (first, most typed) spelling. What the format has no
field for is folded into `clusters` where it can be:
- a sibilant in a cluster is "s" (স্ত "st", স্ব "sw"), though a lone one is "x": every two-letter
  cluster with শ, ষ or স first or second gets its own entry;
- the -ya and -wa signs (C + virama + য / ব / ৱ) are "y" and "w", not the letters' own "j" and
  "b": every such cluster gets an entry.
The rest (medial schwa deletion, the "o" kept after a final cluster, the apostrophe as a vowel)
is described under `notes`, for the transliterator model or a later format.

The romanised lookup (`romanised_keys`) keys every word by `Format1.key`, so the build matches
what the engine computes at run time.
"""

import argparse
import json
from dataclasses import dataclass
from functools import cached_property, lru_cache
from pathlib import Path

from mlkb.langs import human
from mlkb.langs.romanisation import LANGS, Tables

VOWELS = "aeiou"


def _longest(table: dict[str, str], word: str, i: int) -> str | None:
    best = None
    for key in table:
        if word.startswith(key, i) and (best is None or len(key) > len(best)):
            best = key
    return best


@dataclass(frozen=True, eq=False)
class Format1:
    """A parsed format-1 `romanisation.json`."""

    data: dict

    @classmethod
    def load(cls, lang_or_path: str | Path) -> "Format1":
        path = Path(lang_or_path)
        if not path.suffix:
            path = LANGS / str(lang_or_path) / "romanisation.json"
        return cls(json.loads(path.read_text(encoding="utf-8")))

    @cached_property
    def _keys(self) -> dict[str, list[str]]:
        names = ("clusters", "consonants", "vowelSigns", "independentVowels", "finals")
        return {n: sorted(self.data[n], key=len, reverse=True) for n in names}

    def _match(self, name: str, word: str, i: int) -> str | None:
        for key in self._keys[name]:
            if word.startswith(key, i):
                return key
        return None

    def romanize(self, word: str) -> str:
        return _romanize(self, word)

    def _walk(self, word: str) -> str:
        d = self.data
        virama, end = d["virama"], d["atWordEnd"]
        out: list[str] = []
        i, n = 0, len(word)
        while i < n:
            key = self._match("clusters", word, i)
            table = "clusters"
            if key is None:
                key, table = self._match("consonants", word, i), "consonants"
            if key is not None:
                out.append(d[table][key])
                i += len(key)
                sign = self._match("vowelSigns", word, i)
                if sign is not None:
                    out.append(d["vowelSigns"][sign])
                    i += len(sign)
                elif word.startswith(virama, i):
                    i += len(virama)
                    if i >= n:
                        out.append(end["virama"])
                else:
                    out.append(end["inherent"] if i >= n else d["inherent"])
                continue
            other = self._match("independentVowels", word, i)
            if other is not None:
                out.append(d["independentVowels"][other])
                i += len(other)
                continue
            other = self._match("finals", word, i)
            if other is not None:
                out.append(d["finals"][other])
                i += len(other)
                continue
            if word[i].isascii() and word[i].isalpha():
                out.append(word[i].lower())
            i += 1
        return "".join(out)

    def skeleton(self, latin: str) -> str:
        return _skeleton(self, latin)

    def _skeleton(self, latin: str) -> str:
        sk = self.data["skeleton"]
        s = [c for c in latin.lower() if "a" <= c <= "z"]
        trail = sk["dropTrailing"]
        if (
            len(s) >= trail["minLength"]
            and s[-1] in trail["letters"]
            and (not trail["afterVowel"] or s[-2] in VOWELS)
        ):
            s.pop()
        text = "".join(s)
        out: list[str] = []
        i = 0
        while i < len(text):
            for pattern, symbols in sk["pairs"]:
                if text.startswith(pattern, i):
                    i += len(pattern)
                    break
            else:
                symbols = sk["singles"].get(text[i], text[i])
                i += 1
            for sym in symbols:
                if not (sk["collapseDoubles"] and out and out[-1] == sym):
                    out.append(sym)
        final = sk["dropFinalVowel"]
        # As romanize.rs: the length before the vowel is dropped is at least minLength.
        if (
            len(out) >= final["minLength"]
            and out[-1] in final["vowels"]
            and (not final["afterConsonant"] or out[-2] not in VOWELS)
        ):
            out.pop()
        return "".join(out)

    def key(self, word: str) -> str:
        """The romanised lookup key of a native word: the skeleton of its walk."""
        return self.skeleton(self.romanize(word))


@lru_cache(maxsize=500_000)
def _romanize(f: Format1, word: str) -> str:
    return f._walk(word)


@lru_cache(maxsize=500_000)
def _skeleton(f: Format1, latin: str) -> str:
    return f._skeleton(latin)


NOTES = {
    "inherent": "The inherent vowel is typed 'o' (কৰিব korib, অসম oxom); at the end of a word it is dropped (মানুহ manuh, ভাল bhal), as atWordEnd says. Two things the format can't say: after a final cluster it is usually kept (পুত্ৰ putro, মন্ত্ৰ montro), and in the middle of a word it is dropped between a vowel + lone consonant and a lone consonant + vowel, scanning right to left (ৰাজনীতি rajniti, চৰকাৰ sorkar, উন্নয়নমূলক unnoyonmulok). On Aksharantar's manual pairs that medial vowel is still typed about 60% of the time, so the walk keeps it; the transliterator learns both.",
    "clusters": "A sibilant (শ ষ স) is 'x' alone but 's' in a cluster (স্ত st, শ্ৰ sr), and the -ya / -wa signs after a consonant are 'y' / 'w' (ধন্য dhony, স্ব sw), not the letters' own 'j' / 'b'. The format has no per-context spellings, so every such two-letter cluster is listed in clusters. A rule in the plug-in (a consonant's spelling after a virama, and in a cluster) would replace about 250 entries.",
    "apostrophe": "Assamese marks the vowel o with an apostrophe after a consonant (হ'ল hol, ক'ত kot). The walk skips it, so the consonant before it gets the inherent 'o', which is what it means; the lexicon keeps the apostrophe in the word (packed as 0x27).",
    "letters": "ৰ (U+09F0) is r and ৱ (U+09F1) is w; the Bengali র never reaches the walk (lang.json folds it to ৰ). য় ড় ঢ় are base + nukta after NFC (U+09DF etc. are composition exclusions), so their keys are two code points. ৎ, ং, ঁ and ঃ are finals.",
    "skeleton": "x, c (চ is s), sh and ch fold to s; z to j; v and w to b (ৱ is typed w or b, ভ bh or v); f to p; o to a, so the inherent vowel typed o or a gives one key (অসম oxom / asom); y to i (আশ্ৰয় ashroy / ashroi). A final a after a consonant is dropped, which covers the final inherent vowel typed or not (ভাল bhal / bhalo).",
    "dropFinalVowel": "minLength is read as in romanize.rs: the length before the vowel is dropped (ente -> ent with 3). The spec's wording says the result's length; one of the two should change.",
}


def export(t: Tables) -> dict:
    """The format-1 file of weighted tables: canonical spellings, contexts folded into clusters."""
    canon = {name: {k: v[0][0] for k, v in t.table(name).items()} for name in
             ("independentVowels", "vowelSigns", "consonants", "finals", "clusters")}
    in_cluster = {k: v[0][0] for k, v in t.table("inCluster").items()}
    after = {k: v[0][0] for k, v in t.table("afterVirama").items()}
    virama = t.virama
    consonants = canon["consonants"]
    clusters = dict(canon["clusters"])
    sibilants = [c for c in "শষস" if c in in_cluster]
    for first, first_latin in consonants.items():
        for second, second_latin in consonants.items():
            key = first + virama + second
            if key in clusters:
                continue
            if second in after:  # the -ya / -wa signs
                head = in_cluster.get(first, first_latin)
                clusters[key] = head + after[second]
            elif first in sibilants or second in sibilants:
                clusters[key] = in_cluster.get(first, first_latin) + in_cluster.get(second, second_latin)
    # Only entries that differ from what the walk would spell anyway.
    clusters = {k: v for k, v in clusters.items()
                if k in canon["clusters"] or v != _plain(k, consonants, virama)}
    inherent = t.table("inherent")
    sk = t.data["skeleton"]
    pairs = sorted(sk["replace"], key=lambda p: -len(p[0]))
    return {
        "format": 1,
        "script": t.data["script"],
        "virama": virama,
        "inherent": inherent["medial"][0][0],
        "atWordEnd": {"virama": "", "inherent": inherent["final"][0][0]},
        "independentVowels": canon["independentVowels"],
        "vowelSigns": canon["vowelSigns"],
        "consonants": consonants,
        "finals": canon["finals"],
        "clusters": dict(sorted(clusters.items())),
        "skeleton": {
            "dropTrailing": {"letters": "", "afterVowel": True, "minLength": 3},
            "pairs": [list(p) for p in pairs],
            "singles": dict(sk["letters"]),
            "collapseDoubles": True,
            "dropFinalVowel": {"vowels": sk.get("dropFinal", ""), "afterConsonant": True, "minLength": 3},
        },
        "notes": " ".join(f"{k}: {v}" for k, v in NOTES.items()),
    }


def _plain(key: str, consonants: dict[str, str], virama: str) -> str:
    return "".join(consonants.get(part, "?") for part in key.split(virama))


def dumps(data: dict) -> str:
    """JSON with one table entry per line."""
    def one(v, indent):
        pad = " " * indent
        if isinstance(v, dict) and v:
            items = [f'{pad}  {json.dumps(k, ensure_ascii=False)}: {one(x, indent + 2)}' for k, x in v.items()]
            return "{\n" + ",\n".join(items) + "\n" + pad + "}"
        return json.dumps(v, ensure_ascii=False)
    return one(data, 0) + "\n"


def main(argv: list[str] | None = None) -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--lang", default="as")
    p.add_argument("--out", type=Path)
    args = p.parse_args(argv)
    data = export(Tables.load(args.lang))
    out = args.out or LANGS / args.lang / "romanisation.json"
    out.write_text(dumps(data), encoding="utf-8")
    print(f"wrote {out}: {len(data['consonants'])} consonants, {len(data['clusters'])} clusters")
    f = Format1(data)
    for source in ("AK-Freq", "AK-NEI", "AK-NEF", None):
        pairs = [(h.native, h.latin) for h in human.aksharantar(args.lang, "test")
                 if h.source.startswith("AK-") and (source is None or h.source == source)]
        _report(f"test {source or 'all manual'}", f, pairs)


def _report(label: str, f: Format1, pairs: list[tuple[str, str]]) -> None:
    n = max(len(pairs), 1)
    canonical = sum(f.romanize(w) == latin for w, latin in pairs)
    exact = prefix = 0
    for w, latin in pairs:
        key, typed = f.key(w), f.skeleton(latin)
        exact += typed == key
        prefix += bool(typed) and key.startswith(typed)
    print(f"{label:22} {len(pairs):6,} pairs  walk = typed {canonical / n:.1%}  "
          f"skeleton {exact / n:.1%}  skeleton or prefix {prefix / n:.1%}")


if __name__ == "__main__":
    main()
