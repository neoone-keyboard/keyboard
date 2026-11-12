#!/usr/bin/env python3
"""Writes the Japanese conversion eval set from Tatoeba's exports: sentences whose reading a
person reviewed, as `id<TAB>reading<TAB>sentence`, the reading in hiragana as a typist's
composer gives it (katakana words typed in hiragana, then converted).

    python3 -I scripts/ja-eval-set.py transcriptions.csv jpn_sentences.tsv > data/eval/ja/tatoeba-convert.tsv

The inputs are https://downloads.tatoeba.org/exports/transcriptions.tar.bz2 (unpacked) and
https://downloads.tatoeba.org/exports/per_language/jpn/jpn_sentences.tsv.bz2 (unpacked).
Tatoeba's sentences are CC BY 2.0 FR (https://tatoeba.org/en/downloads); the set is only read
by the eval (engine/core/tests/ja_eval.rs), never shipped.

Kept: Japanese transcriptions with a reviewer (the export's username column), whose sentence
matches the transcription's text, of 4 to 40 characters, written only in kana, kanji, ー, 々
and the marks the composer types (、。！？「」・〜). Sentences with digits or Latin letters are
left out: how a number is typed and converted is a choice, not a reading. A fixed sample of
SAMPLE sentences, chosen by a hash of the id, so a newer export changes few lines.
"""

import hashlib
import re
import sys

SAMPLE = 2000

# [漢字|かん|じ]: the text, then one reading per kanji (or one for the whole group).
GROUP = re.compile(r"\[([^|\]]+)\|([^\]]+)\]")
KANA = r"ぁ-ゖァ-ヺー"
ALLOWED = re.compile(rf"^[{KANA}一-鿿㐀-䶿々〆ヶ、。！？「」・〜]+$")
READING = re.compile(rf"^[{KANA}、。！？「」・〜]+$")


def hiragana(text: str) -> str:
    return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in text)


def parse(transcription: str) -> tuple[str, str]:
    """(text, reading) of a furigana transcription."""
    text = GROUP.sub(lambda m: m.group(1), transcription)
    reading = GROUP.sub(lambda m: "".join(m.group(2).split("|")), transcription)
    return text, reading


def main() -> int:
    transcriptions, sentences = sys.argv[1], sys.argv[2]
    texts = {}
    with open(sentences, encoding="utf-8") as f:
        for line in f:
            parts = line.rstrip("\n").split("\t")
            if len(parts) >= 3:
                texts[parts[0]] = parts[2]
    rows = {}
    with open(transcriptions, encoding="utf-8") as f:
        for line in f:
            parts = line.rstrip("\n").split("\t")
            if len(parts) != 5 or parts[1] != "jpn" or not parts[3]:
                continue
            sid, transcription = parts[0], parts[4]
            text, reading = parse(transcription)
            reading = hiragana(reading)
            if texts.get(sid) != text or not 4 <= len(text) <= 40:
                continue
            if not ALLOWED.match(text) or not READING.match(reading):
                continue
            rows[sid] = (reading, text)
    chosen = sorted(rows, key=lambda sid: hashlib.sha256(sid.encode()).hexdigest())[:SAMPLE]
    out = sys.stdout
    out.write("# Japanese conversion eval: Tatoeba sentences (https://tatoeba.org) with readings a\n")
    out.write("# person reviewed, CC BY 2.0 FR, by Tatoeba's contributors. Written by\n")
    out.write("# scripts/ja-eval-set.py; read by engine/core/tests/ja_eval.rs. id, reading, sentence.\n")
    for sid in sorted(chosen, key=int):
        reading, text = rows[sid]
        out.write(f"{sid}\t{reading}\t{text}\n")
    print(f"{len(chosen)} of {len(rows)} usable sentences", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
