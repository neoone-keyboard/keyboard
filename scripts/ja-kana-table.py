#!/usr/bin/env python3
"""Writes the Japanese pack's romaji table, `kana.json` (the `kana` role, engine 9), to stdout.

    python3 -I scripts/ja-kana-table.py > data/langs/ja/kana.json

The rules are the usual romaji input conventions of Japanese input methods, written out here
from grids so the table can be read and changed: Hepburn and Kunrei spellings both work
(`shi` and `si`, `tsu` and `tu`, `chi` and `ti`, `fu` and `hu`, `ji` and `zi`), `x` or `l`
makes a small kana (`xa` ぁ, `ltu` っ, `xya` ゃ), a doubled consonant makes っ (`kka` っか),
`nn`, `n'` and `xn` make ん, and an `n` before any other consonant (or at the end) makes ん
too, as the engine's composer reads the table (`convert::romaji`). `-` is the long vowel mark
ー; `,` `.` `[` `]` are Japanese punctuation.

Each rule is [typed, kana] or [typed, kana, left]: after the kana, `left` stays typed, waiting
for the next letter (`kk` gives っ and leaves `k`).
"""

import json
import sys

VOWELS = "aiueo"


def row(prefix: str, kana: str) -> list[list[str]]:
    """`prefix` + each vowel → the five kana of `kana` (a space for a gap)."""
    return [[prefix + v, k] for v, k in zip(VOWELS, kana) if k != " "]


def yoon(prefix: str, base: str) -> list[list[str]]:
    """`prefix` + ya, yi, yu, ye, yo → `base` + small ゃ ぃ ゅ ぇ ょ."""
    return [[prefix + v, base + s] for v, s in zip(VOWELS, "ゃぃゅぇょ")]


def small(prefix: str, base: str) -> list[list[str]]:
    """`prefix` + a, i, u, e, o → `base` + small ぁ ぃ ぅ ぇ ぉ (fa ふぁ)."""
    return [[prefix + v, base + s] for v, s in zip(VOWELS, "ぁぃぅぇぉ")]


def rules() -> list[list[str]]:
    r: list[list[str]] = []
    r += row("", "あいうえお")
    r += row("k", "かきくけこ")
    r += row("g", "がぎぐげご")
    r += row("s", "さしすせそ")
    r += row("z", "ざじずぜぞ")
    r += row("t", "たちつてと")
    r += row("d", "だぢづでど")
    r += row("n", "なにぬねの")
    r += row("h", "はひふへほ")
    r += row("b", "ばびぶべぼ")
    r += row("p", "ぱぴぷぺぽ")
    r += row("m", "まみむめも")
    r += [["ya", "や"], ["yu", "ゆ"], ["yo", "よ"]]
    r += row("r", "らりるれろ")
    r += [["wa", "わ"], ["wi", "うぃ"], ["wu", "う"], ["we", "うぇ"], ["wo", "を"]]
    r += [["ye", "いぇ"]]
    # Hepburn and other spellings of the same kana.
    r += [["shi", "し"], ["chi", "ち"], ["tsu", "つ"], ["fu", "ふ"], ["ji", "じ"]]
    r += [["ca", "か"], ["ci", "し"], ["cu", "く"], ["ce", "せ"], ["co", "こ"]]
    r += [["qa", "くぁ"], ["qi", "くぃ"], ["qu", "く"], ["qe", "くぇ"], ["qo", "くぉ"]]
    # Contracted sounds.
    for prefix, base in [("ky", "き"), ("gy", "ぎ"), ("sy", "し"), ("zy", "じ"), ("ty", "ち"),
                         ("cy", "ち"), ("dy", "ぢ"), ("ny", "に"), ("hy", "ひ"), ("by", "び"),
                         ("py", "ぴ"), ("my", "み"), ("ry", "り"), ("jy", "じ"), ("fy", "ふ"),
                         ("vy", "ゔ")]:
        r += yoon(prefix, base)
    for prefix, base in [("sh", "し"), ("ch", "ち"), ("j", "じ")]:
        r += [[prefix + "a", base + "ゃ"], [prefix + "u", base + "ゅ"], [prefix + "e", base + "ぇ"],
              [prefix + "o", base + "ょ"]]
    # Sounds written with a small vowel.
    r += [["fa", "ふぁ"], ["fi", "ふぃ"], ["fe", "ふぇ"], ["fo", "ふぉ"]]
    r += [["va", "ゔぁ"], ["vi", "ゔぃ"], ["vu", "ゔ"], ["ve", "ゔぇ"], ["vo", "ゔぉ"]]
    r += [["tsa", "つぁ"], ["tsi", "つぃ"], ["tse", "つぇ"], ["tso", "つぉ"]]
    r += [["tha", "てゃ"], ["thi", "てぃ"], ["thu", "てゅ"], ["the", "てぇ"], ["tho", "てょ"]]
    r += [["dha", "でゃ"], ["dhi", "でぃ"], ["dhu", "でゅ"], ["dhe", "でぇ"], ["dho", "でょ"]]
    r += [["twu", "とぅ"], ["dwu", "どぅ"], ["t'i", "てぃ"], ["d'i", "でぃ"], ["t'u", "とぅ"], ["d'u", "どぅ"]]
    for prefix, base in [("kw", "く"), ("gw", "ぐ"), ("sw", "す"), ("zw", "ず"), ("tw", "と"),
                         ("dw", "ど"), ("hw", "ふ"), ("wh", "う")]:
        r += [rule for rule in small(prefix, base) if rule[0] not in ("twu", "dwu")]
    r += [["whu", "う"], ["kwa", "くぁ"]]
    # Small kana, with x or l.
    for lead in "xl":
        r += row(lead, "ぁぃぅぇぉ")
        r += [[lead + "ya", "ゃ"], [lead + "yu", "ゅ"], [lead + "yo", "ょ"], [lead + "yi", "ぃ"],
              [lead + "ye", "ぇ"], [lead + "tu", "っ"], [lead + "tsu", "っ"], [lead + "wa", "ゎ"],
              [lead + "ka", "ヵ"], [lead + "ke", "ヶ"]]
    r += [["wyi", "ゐ"], ["wye", "ゑ"]]
    # ん: nn, n', xn; a lone n before another consonant is the composer's (it reads "n" → ん
    # when nothing longer can follow).
    r += [["nn", "ん"], ["n'", "ん"], ["xn", "ん"], ["n", "ん"]]
    # A doubled consonant is っ, the second letter left to start the next kana.
    for c in "kgsztdhbpmyrwfjvcqxl":
        r += [[c + c, "っ", c]]
    r += [["tch", "っ", "ch"]]
    # Marks.
    r += [["-", "ー"], [",", "、"], [".", "。"], ["[", "「"], ["]", "」"], ["~", "〜"],
          ["/", "・"], ["?", "？"], ["!", "！"]]
    # Later rules for the same keys replace earlier ones, as a dict keeps the last value; keep
    # the first spelling given instead, so the grids above win.
    seen: dict[str, list[str]] = {}
    for rule in r:
        seen.setdefault(rule[0], rule)
    return sorted(seen.values())


def main() -> int:
    table = {
        "format": 1,
        "notes": "Romaji to kana for the Japanese pack, written by scripts/ja-kana-table.py. Each "
                 "rule is [typed, kana] or [typed, kana, left], left staying typed for the next "
                 "letter.",
        "rules": rules(),
    }
    # One rule a line, so a change to the table reads as a change to its rules.
    lines = [json.dumps(rule, ensure_ascii=False) for rule in table["rules"]]
    sys.stdout.write("{\n")
    sys.stdout.write(f'  "format": {table["format"]},\n')
    sys.stdout.write(f'  "notes": {json.dumps(table["notes"], ensure_ascii=False)},\n')
    sys.stdout.write('  "rules": [\n    ' + ",\n    ".join(lines) + "\n  ]\n}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
