"""The pack file `romanisation.json`, format 1 (docs/design/2026-11-06-m6-romanisation-format.md):
the walk and skeleton exactly as the spec says the engine computes them, and the export of a
language's weighted tables (`romanisation.Tables`) to that format.

    uv run python -m mlkb.langs.format1 --lang as      # writes data/langs/as/romanisation.json

The export takes each table's canonical (first, most typed) spelling. What the format has no
field for is folded into `clusters` where it can be:
- a letter listed in the hand file's `inClusterLetters` takes its `inCluster` spelling in a
  cluster: Assamese lists its sibilants, which are "s" there (স্ত "st", স্ব "sw") though a lone one
  is "x", so every two-letter cluster with শ, ষ or স first or second gets its own entry;
- the -ya and -wa signs (C + virama + য / ব / ৱ) are "y" and "w", not the letters' own "j" and
  "b": every such cluster gets an entry.
The rest (medial schwa deletion, the "o" kept after a final cluster, the apostrophe as a vowel)
is for the language's `notes`, which the export copies from the hand file (a string, or a dict of
topic to text); a language without them gets "".

The romanised lookup (`romanised_keys`) keys every word by `Format1.key`, so the build matches
what the engine computes at run time.
"""

import argparse
import json
from dataclasses import dataclass
from functools import cached_property, lru_cache
from pathlib import Path

from mlkb.langs import human
from mlkb.langs.romanisation import LANGS, Tables, _drop_between_consonants

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
            len(s) >= max(trail["minLength"], 1)
            and s[-1] in trail["letters"]
            and (not trail["afterVowel"] or (len(s) >= 2 and s[-2] in VOWELS))
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
        out = _drop_between_consonants(out, sk.get("dropBetweenConsonants", ""))
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

    def keys(self, word: str) -> list[str]:
        """Every key of a native word: `key`, then the skeleton of its walk without a last sign
        that the file's `optionalFinals` says may go unwritten, when that is another key (the
        engine's `Romanisation::word_skeletons`). One key for a file without the field. The
        key `dropTrailing.alsoKept` adds is not written here (`lexbuild-rekey` writes it)."""
        whole = self.key(word)
        if not whole:
            return []
        latin = self.romanize(word)
        for sign in self.data.get("optionalFinals", []):
            end = self.data["finals"].get(sign, "")
            before = word[: -len(sign)]
            vowels = (*self.data["vowelSigns"], *self.data["independentVowels"])
            if (word.endswith(sign) and end and latin.endswith(end)
                    and any(before.endswith(v) for v in vowels)):
                other = self.skeleton(latin[: -len(end)])
                if other and other != whole:
                    return [whole, other]
                break
        return [whole]


@lru_cache(maxsize=500_000)
def _romanize(f: Format1, word: str) -> str:
    return f._walk(word)


@lru_cache(maxsize=500_000)
def _skeleton(f: Format1, latin: str) -> str:
    return f._skeleton(latin)


def _hand(t: Tables) -> dict:
    """The hand-written tables next to `t`'s file (`romanisation.hand.json`), else `t`'s own data.
    The fields the fitting never changes (`notes`, `inClusterLetters`) are read from it, so an edit
    there reaches the export without calibrating again."""
    if t.path is not None:
        hand = t.path.with_name("romanisation.hand.json")
        if hand.exists():
            return json.loads(hand.read_text(encoding="utf-8"))
    return t.data


def _notes(hand: dict) -> str:
    notes = hand.get("notes", "")
    return notes if isinstance(notes, str) else " ".join(f"{k}: {v}" for k, v in notes.items())


def export(t: Tables) -> dict:
    """The format-1 file of weighted tables: canonical spellings, contexts folded into clusters."""
    canon = {name: {k: v[0][0] for k, v in t.table(name).items()} for name in
             ("independentVowels", "vowelSigns", "consonants", "finals", "clusters")}
    in_cluster = {k: v[0][0] for k, v in t.table("inCluster").items()}
    after = {k: v[0][0] for k, v in t.table("afterVirama").items()}
    virama = t.virama
    consonants = canon["consonants"]
    clusters = dict(canon["clusters"])
    hand = _hand(t)
    # Letters whose `inCluster` spelling holds in every two-letter cluster they are in (Assamese's
    # sibilants শ ষ স: x alone, s in a cluster).
    sibilants = [c for c in hand.get("inClusterLetters", "") if c in in_cluster]
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
    # Which typed spellings spell a word out (the engine's `Romanisation::relaxed`). Always
    # written: a file without it is read as the packs published before the field were, with
    # Malayalam's habits. A hand file that says nothing gets none.
    loose = {k: v for k, v in hand.get("looseSpelling", {}).items() if k in ("singles", "pairs", "collapse")}
    # The skeleton's optional steps, copied from the hand file when it has them: a final letter
    # dropped after a vowel (Gujarati's final anusvara, typed n, m or not at all) and a letter
    # dropped between consonants (the medial inherent vowel). A file without them is as before.
    skeleton = {
        "dropTrailing": sk.get("dropTrailing", {"letters": "", "afterVowel": True, "minLength": 3}),
        "pairs": [list(p) for p in pairs],
        "singles": dict(sk["letters"]),
        "collapseDoubles": True,
    }
    if sk.get("dropBetweenConsonants"):
        skeleton["dropBetweenConsonants"] = sk["dropBetweenConsonants"]
    # A letter people write for that dropped vowel (Hindi's u: hum for हम); the engine reads
    # typed letters the second way too. It changes no key, so this skeleton doesn't use it.
    if sk.get("alsoBetweenConsonants"):
        skeleton["alsoBetweenConsonants"] = sk["alsoBetweenConsonants"]
    skeleton["dropFinalVowel"] = {"vowels": sk.get("dropFinal", ""), "afterConsonant": True, "minLength": 3}
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
        "skeleton": skeleton,
        "looseSpelling": loose,
        # Final signs people leave unwritten (`Format1.keys`), when the hand file names any.
        **({"optionalFinals": hand["optionalFinals"]} if hand.get("optionalFinals") else {}),
        "notes": _notes(hand),
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
