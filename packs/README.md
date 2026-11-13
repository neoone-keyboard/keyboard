# Language packs

Each `<id>.neopack` folder is one language pack: the files the keyboard loads for a language.
The site serves each pack as a zip of its folder (`make site-packs`), and the app imports it.

## What a pack folder holds

- **Hand-written sources**, kept only here: `manifest.json` (the pack's name, script, files,
  checksums and `credits`), `layout.json` (the keyboard layout), and, in some packs,
  `chat-forms.tsv` (chat spellings) and `suffixes.tsv` (chat suffixes).
- **Copies of `data/langs/<id>/` files**, which `scripts/pack.py --write` copies in and
  `--check` compares: `lang.json` (the script rules), `romanisation.json` (copied by
  `data/build-pack-lexicons.sh`), `emoji-words.txt` and `emoji-search.txt`, and for Japanese
  `kana.json` and `dictionary-notices.txt`. Edit the file in `data/langs/<id>/`, not the copy.
- **Built binaries**: the lexicons (`.fst`: the word list, `next.fst` for next words, and the
  romanised lookup `romanised.fst`, `hinglish.fst` or `manglish.fst`), the models (`.mlkbnn`:
  `translit.mlkbnn` the transliterator, `lm.mlkbnn` the language model) and, for Japanese,
  `conversion.dict` and `connection.bin`. The Makefile builds them (`make pack-lexicons`,
  `make pack-models`, `make lm`, `make ja-pack`; see the comments there). They are committed on
  purpose, so that the engine tests and the site build read them without a rebuild. Don't edit
  them by hand: rebuild them, then run `scripts/pack.py packs/<id>.neopack --write` to update
  the checksums. `.mlkbnn` is the model file format; the name is historical.

Not every pack has every file. The Japanese pack has no lexicons, models or script rules: it
has `kana.json`, `conversion.dict`, `connection.bin` and `dictionary-notices.txt`. Only `as`,
`hi`, `ml` and `ta` have a language model; only `hi`, `ml` and `ne` have `chat-forms.tsv`, and
only `ml` has `suffixes.tsv`. `bho` and `mai` have no emoji files. `manifest.json`'s `files`
lists what each pack has.

## Licence

Each pack as a whole is licensed under
[CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/), because its lexicons and
models are built from CC BY-SA text (for example Wikipedia and its sister projects, and in
some packs the Dakshina dataset). The sources of each pack, with their licences, are credited
in its `manifest.json` under `credits`. Some sources have other licences (for example
Aksharantar: CC BY and CC0 1.0; English Wikinews: CC BY; Project Gutenberg: public domain).

The emoji files (`emoji-words.txt`, `emoji-search.txt`) are built from Unicode CLDR
annotations, under the Unicode License v3, credited in the manifest; its full notice is in
[`LICENSES/Unicode-3.0.txt`](../LICENSES/Unicode-3.0.txt).

The exception is the Japanese pack. `packs/ja.neopack/conversion.dict` and `connection.bin`
are built from the Mozc OSS dictionary and are **not** under CC BY-SA 4.0: they are under the
terms in [`packs/ja.neopack/dictionary-notices.txt`](ja.neopack/dictionary-notices.txt), which
are the IPAdic licence (NAIST), the conditions for ICOT Free Software and Google's BSD licence
for Mozc. That file must stay with every copy of the two files. The Japanese pack credits no
CC BY-SA source; its other files are the project's own, below.

The hand-written files (`layout.json`, `chat-forms.tsv`, `suffixes.tsv`, and the copied
`lang.json`) are this project's own work: licensed under the Apache License 2.0 like the
code, and also offered under the pack's CC BY-SA 4.0. See [`NOTICE`](../NOTICE).
