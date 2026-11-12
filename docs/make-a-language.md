# Make your own language

NeoOne Keyboard lets people type a language the way it sounds in English letters and get
its own script back: `namaste` gives नमस्ते, `oxom` gives অসম, `ente` gives എന്റെ. English is
built into the app. Every other language is a **language pack** that anyone can build from
openly licensed text, then import on their phone or contribute back.

This guide walks through building a pack with the tools in this repository as they work
today. It uses the Assamese pack (`packs/as.neopack`) as the worked example, because it is
the first that was built almost entirely from data. Tamil (`packs/ta.neopack`,
`data/langs/ta/`) is the example of a script the engine has no copy of the rules for; it was built
by following this guide. Hindi and Malayalam appear where they differ.

No step needs an edit to the engine, the app or the training code. A pack is data, plus one
short shell script that fetches its text. Some steps only work for certain kinds of language.
The guide says so where it happens, and [make-a-language-gaps.md](make-a-language-gaps.md)
lists each one with a proposed fix.

## Before you start: can your language be a pack today?

Check three things first.

1. **The script is described in `lang.json`.** How a script behaves (which characters are
   letters, its normal form, where a vowel sign attaches, what ends a sentence) is data: the
   `lang.json` of step 2, which the pack carries (the `lang` role) and the engine reads, so any
   script works without code, and no script's rules are code: Malayalam's, Hindi's and
   Assamese's are `data/langs/ml`, `hi` and `as/lang.json`. The engine holds a copy of those
   three files, used only for a pack in `mlym`, `deva` or `beng` that has no `lang` file (the
   packs published before packs carried one). Its `beng` copy is Assamese's, so a Bengali
   pack must carry its own file.
2. **The script's words are letters the romanisation format can walk.** The format reads a
   word as consonants, each with an inherent vowel unless a vowel sign or letter follows, and
   clusters (gap G3). A Brahmic script fits it directly. A script that writes no short vowels
   and has no virama fits too, as Urdu does (`data/langs/ur`): its vowel letters are the
   `vowelSigns`, `virama` is `""`, the aspirates are `clusters` written side by side, and the
   skeleton drops the vowels it can't see. A script in one 128-code-point block packs its
   romanised keys and transliterator vocabulary from the block start; a script spread over a
   larger block lists its letters as `charTable` instead (at most 225 characters, engine 8).
3. **There are human romanisations to learn from.** The tools read them from
   [Aksharantar](https://huggingface.co/datasets/ai4bharat/Aksharantar), which covers 21
   languages. A language without Aksharantar data needs code changes (gap G6).

Some examples: Marathi, Nepali or Sanskrit (Devanagari) and Bengali need only data. So do
Tamil, Telugu, Gujarati, Kannada, Odia and Punjabi (Gurmukhi): their packs carry `lang.json`,
which only engine 4 and later apps read (step 5 says how to find the engine a pack needs).
Assamese's র → ৰ fold is in its own `lang.json`, so a Bengali pack keeps its র.
`packs/ta.neopack` is a Tamil pack built this way, `packs/gu.neopack` a Gujarati,
`packs/te.neopack` a Telugu, `packs/kn.neopack` a Kannada, `packs/or.neopack` an Odia and
`packs/pa.neopack` a Punjabi one, and `engine/core/tests/fixtures/gu.neopack` a test-sized
Gujarati one.

## What a pack is

A pack is a folder named `<id>.neopack/`, zipped into a `.neopack` file to install or
download. The folder holds `manifest.json` and the data files it names. Assamese:

```
packs/as.neopack/
  manifest.json       what the pack is, its files, their checksums, its credits
  as.fst              native words with their frequencies (the lexicon)
  romanised.fst       the same words keyed by their romanised spelling
  romanisation.json   how each letter is spelled in Latin, and the folds for loose typing
  next.fst            next-word pairs, native and English
  translit.mlkbnn     the transliterator: a small neural model, Latin in, native script out
  layout.json         the native-script keyboard
```

| Manifest role | Example file | What it does | Required? |
|---|---|---|---|
| `lexicon` | `as.fst` | Native words and counts. Completions, corrections and the native keyboard's suggestions come from it. | **Yes.** A pack without it is refused. |
| `romanisedLexicon` | `romanised.fst` | Each native word under the key its romanised spelling folds to. This is what turns `oxom` into অসম without any model. | No, but without it Latin typing finds nothing unless there is a transliterator. |
| `romanisation` | `romanisation.json` | The letter-by-letter spelling table and the skeleton folds. The engine folds what you type with it. Format: [romanisation spec](design/2026-11-06-m6-romanisation-format.md). | Needed with `romanisedLexicon`. |
| `nextWords` | `next.fst` | Word pairs for the next-word suggestions after a space. | No |
| `translit` | `translit.mlkbnn` | The transliterator (about 4.5M parameters, 4.7 MB int8). Spells words the lexicon has never seen. | No |
| `lm` | `lm.mlkbnn` | A bilingual neural language model (about 9.5 MB) that ranks suggestions by context. Malayalam, Hindi, Assamese and Tamil have one (step 12). | No |
| `layout` | `layout.json` | The native-script keyboard pages. Without it the language is typed in Latin only and the language key skips it. | No |
| `lang` | `lang.json` | The script's definition: `data/langs/xx/lang.json`, the file the lexicons were built with (step 2). `pack.py --write` copies it in. Engine 4. | **Yes.** `pack.py --check` fails without it. (The engine still reads an older `mlym`, `deva` or `beng` pack that lacks it.) |
| `suffixes` | `suffixes.tsv` | Endings that chat glues onto words, for splitting `stem + ending`. Only a pack whose `lang.json` has `suffixJoins` uses it (Malayalam today; step 4b). | No |
| `chatForms` | `chat-forms.tsv` | Hand-written chat spellings no rule reaches: `latin<TAB>native` per line. | No |
| `emojiWords`, `emojiSearch` | `emoji-words.txt`, `emoji-search.txt` | Your language's words for emoji: the first suggests an emoji in the bar for a typed word, the second is the emoji panel's search. Built from Unicode CLDR's annotations for your locale (step 8b). Read by the app, not the engine. | No. Without them the bar and the search know English only. |

Any missing optional file switches off only its feature.

## Prerequisites

- **A Mac with Apple Silicon** for model training. Training uses PyTorch on the GPU (MPS).
  The project's numbers come from an M5 Max with 48 GB. Building lexicons works anywhere
  Rust and Python run. Training on a CPU should work but nobody has timed it.
- **Rust** through [rustup](https://rustup.rs). `engine/rust-toolchain.toml` pins the version
  and rustup installs it on first use.
- **[uv](https://docs.astral.sh/uv/)** for Python. `training/` and `data/pipeline/` are uv
  projects. Python 3.12 is the minimum, not a pin: uv picks a newer one if you have it (3.14
  works), so don't install 3.12 specially. The first `uv run` in `training/` builds the
  engine's Python bindings (`engine/py`), which takes a few minutes.
- **Disk:** a few GB for a smaller language's dumps and builds. The Assamese Wikipedia,
  Wikisource and Wikiquote dumps together are small. You do *not* need the 3 GB English
  download, because the English half of `next.fst` is copied from the Malayalam pack.
- **Xcode and xcodegen** only if you build the app yourself. Importing a pack into the
  released app needs neither.

Rough times for Assamese (about 20M words of text): fetching and extracting text takes
minutes, lexicons take minutes, transliterator training takes about an hour on an M5 Max (it
stopped early at step 40,000), and an optional LM adds about 2 hours of data preparation and
4 to 7 hours of training.

A Marathi test run (39M words, an M-series Mac whose GPU was shared with another training
job) took: download 3 minutes, text 1.5 minutes, one calibrate run 25 seconds, lexicons 1.5
minutes, training pairs 1 minute, a short 6,000-step transliterator 17 minutes (about 6 steps
a second), and export plus evaluation 2 minutes. At that speed the default training (up to
100,000 steps, stopping early) would take 2 to 4.5 hours. Another job on the same GPU slows
training a lot; run one at a time if you can. The writing (sources, `lang.json`, the
romanisation tables, the manifest and the layout) took longer than all the commands together.

## Step by step

Every command runs from the repository root unless it says `cd training`. Replace `xx` with
your language's id (1 to 16 characters of `a-z 0-9 -`, starting with a letter; not `en`).
Use the ISO 639-1 code where there is one.

Do the steps in order; each uses what the ones before it made. Calibrating (step 4) reads
Aksharantar, which the text script (step 3) unzips. The lexicon build (step 7) needs the text
script, the fitted weights (step 4) and the manifest (step 5), and checks for all three
before it starts. Training (step 9) needs the lexicon (step 7).

### 1. Choose sources and check their licences

Packs are published under CC BY-SA 4.0, so every source must allow that. The project's rules
come from its [licence audit](design/2026-11-03-m3-licence-audit.md):

- **Use:** CC BY-SA, CC BY, CC0 and public domain. Wikipedia, Wikisource, Wikiquote and
  Wikinews are the usual base. Aksharantar's manual rows are CC BY and its mined rows CC0.
- **Reject:** anything NonCommercial (NC) or NoDerivatives (ND), web crawls, text whose
  licence comes only from a third party that doesn't own it, and claims you can't verify
  (an unsupported "public domain").
- **Eval only:** text you may measure on but never train on, such as a CC BY set of social
  media comments the curators don't own. It never ships, so it needs no credit.

Write one manifest per source in `data/manifests/<source-id>.json`. `data/fetch.sh` downloads
what they list into `data/raw/<source-id>/` and checks each file's digest. Copy
`data/manifests/aswiki.json`:

```json
{
  "id": "xxwiki",
  "name": "Xx Wikipedia",
  "homepage": "https://xx.wikipedia.org/",
  "licence": "CC BY-SA 4.0",
  "licence_url": "https://creativecommons.org/licenses/by-sa/4.0/",
  "licence_source": "where you read the licence, and when",
  "attribution": "Text of Xx Wikipedia articles by Wikipedia contributors.",
  "used_for": "what the pack builds from it",
  "retrieved": "2026-11-10",
  "files": [
    {
      "url": "https://dumps.wikimedia.org/xxwiki/20261101/xxwiki-20261101-pages-articles.xml.bz2",
      "sha1": "<from xxwiki-20261101-sha1sums.txt in the same dump folder>",
      "path": "xxwiki-20261101-pages-articles.xml.bz2"
    }
  ],
  "status": "in packs/xx.neopack"
}
```

`files` needs `url`, `path` and a `sha256` or `sha1`. The other fields are for people, and
the credits page reads `attribution`. Wikimedia publishes a sha1 for every dump file in
`<wiki>-<date>-sha1sums.txt` in the same folder.

For Aksharantar, copy `data/manifests/aksharantar-asm.json`. Three things change:

- the language code: `id`, `name`, and `asm.zip` in `url` and `path` (to, say, `mar.zip`);
- the prose in `verdict_reason`, `used_for`, `notes` and `status`, which describes the Assamese
  files. Rewrite it for yours once you have counted the rows (below). People read these fields
  and the licence audit relies on them; no tool does;
- `extracted`, the sha256 of each unzipped file. It is a record for people, and nothing checks
  it. Fill it in with `shasum -a 256 data/raw/aksharantar-xxx/extracted/*.json` after
  unzipping, or leave the field out.

Aksharantar publishes no checksums, so compute the zip's sha256 yourself:

```sh
curl -L -o /tmp/mar.zip https://huggingface.co/datasets/ai4bharat/Aksharantar/resolve/main/mar.zip
shasum -a 256 /tmp/mar.zip
```

Or leave `sha256` out of the manifest: `fetch.sh` then downloads the file, keeps it, prints
the line to add (`"sha256": "…"`) and stops for that source. Add it and run `fetch.sh` again.

Download only your sources (`fetch.sh` with no arguments fetches every manifest, about 3 GB).
`--only` takes a comma-separated list, or can be repeated:

```sh
./data/fetch.sh --only xxwiki,xxwikisource,xxwikiquote,aksharantar-xxx
```

An id with no manifest is an error. A source that fails doesn't stop the others; `fetch.sh`
lists the failures at the end.

Then look at what Aksharantar has for your language. Splits differ by language: Assamese has
manual rows (sources `AK-…`, typed by people) in `valid` and `test`, Marathi has none in
`valid`. Unzip it (`unzip -q data/raw/aksharantar-xxx/xxx.zip -d
data/raw/aksharantar-xxx/extracted`; the text script in step 3 does the same) and count the
rows per source in each split:

```sh
python3 -c '
import collections, json, sys
for split in ("train", "valid", "test"):
    path = f"data/raw/aksharantar-{sys.argv[1]}/extracted/{sys.argv[1]}_{split}.json"
    print(split, collections.Counter(json.loads(line)["source"] for line in open(path) if line.strip()).most_common())
' mar
```

```
train [('IndicCorp', 1379659), ('Dakshina', 37690), ('Samanantar', 21320), ('Wikidata', 8015), ('AK-Freq', 4146), ('Existing', 1918)]
valid [('Dakshina', 3631), ('IndicCorp', 3493), ('Samanantar', 336), ('Wikidata', 186)]
test [('Dakshina', 5645), ('AK-Freq', 4467), ('AK-NEI', 1247), ('AK-NEF', 831)]
```

The tools drop `Dakshina` and `Existing` rows (their licences don't allow a CC BY-SA pack).
Record what your files have and what is dropped in the manifest's `verdict_reason` and
`notes`, as the Assamese one does: for Marathi, "46,966 Dakshina and 1,918 Existing rows are
dropped by the filter". The manual rows matter in step 4: the tables are calibrated on
`valid`'s manual rows, or, when `valid` has none (Marathi), on a held-out quarter of
`train`'s. Without any manual rows, see step 4.

### 2. Describe the language: `data/langs/xx/lang.json`

This file tells the lexicon builder what a word is, and, shipped in the pack, tells the
engine the same: one definition for both. The engine's parser is
`engine/core/src/script/data.rs`. Copy the file nearest your script:

- `data/langs/as/lang.json` for a language in the Bengali-Assamese script;
- the Devanagari example below for a `deva` language;
- for any other script, `data/langs/ta/lang.json` (Tamil, a full pack) or
  `engine/core/tests/fixtures/gu.neopack/lang.json` (Gujarati, the smallest file that works).
  Both are scripts the engine has no code for, so the file is all the engine knows. Change the
  ranges to your block's, using its [Unicode chart](https://www.unicode.org/charts/).

| Field | Meaning |
|---|---|
| `id`, `name`, `script` | The pack id, English name, and ISO 15924 script code in lower case (`gujr`, `taml`; a private-use code such as `qaaa` for an invented script). |
| `block` | The script's Unicode block, first and last code point in hex. |
| `letters` | More ranges of the script's characters, `[["first", "last"], …]`, for a script not in one block. `block`, `letters` or both. |
| `charTable` | `[["first", "last"], …]`: every character a word of the script may hold, in the order the romanised lexicon and the transliterator number them (at most 225). For a script whose letters are not one 128-code-point block: Urdu's 47 letters are scattered over the 256 of Arabic. The ranges must lie inside `block` and `letters`. Don't change it once a pack is published: its transliterator and romanised lexicon are only read with the same table. Optional: without it, characters are numbered from the block start. Engine 8. |
| `notWord` | Ranges in the block that are not letters (digits, currency signs). |
| `firstLetter` | Ranges a word may start with: independent vowels, consonants, and any other letter that starts a written word, even if only in loanwords (Tamil lists aytham ஃ for ஃபேஸ்புக்). Never signs. A token that starts with anything else is dropped. Optional: any letter that isn't a sign. |
| `virama` | The virama's code point. Optional. |
| `signs`, `bases` | Vowel signs and the letters they may follow. A word with a sign after anything else is treated as mis-encoded and dropped. The keyboard shows the vowel-sign strip after a `bases` letter. Optional. |
| `apostrophe` | `true` if an apostrophe can stand inside a word (Assamese হ'ল). |
| `replace` | Text replacements applied before and after Unicode NFC, to fix old or wrong encodings. |
| `phraseBreaks` | The script's own sentence-ending characters (the danda). Next-word pairs never cross one. Latin punctuation (`. , ; : ? !` and brackets) always ends a phrase, so a script that uses only that (Tamil) can leave the list out or empty. |
| `notLetter` | Ranges that stay inside a run when text is split into words but that a word is not made of. Malayalam lists its digits and number signs, as its text has always been split that way; a new language should use `notWord`. Engine 5. |
| `looseSigns` | Vowel signs that loose spelling inserts between consonants (Malayalam ആവിശ്യം for ആവശ്യം). A typed word that reads as a rare word is also tried without one such sign before a consonant. Optional. Engine 5. |
| `finalPairs` | `[["a", "b"]]`, code points in hex: a word ending in a consonant and one of the two is also written with the other (Malayalam's final ു and ്). Optional. Engine 5. |
| `suffixJoins` | How a stem and a suffix of `suffixes.tsv` fuse: the vowel signs, glides and final forms of the script's sandhi. Step 4b explains each part, with Malayalam's and a Tamil example. Without it the pack is never split into stem and suffix. Optional. Engine 5 (6 for a `nasal` ending of several code points). |
| `joiners` | `"afterVirama"`: a ZWJ or ZWNJ between the `virama` and a character of the script is part of the word, because it changes how the cluster is drawn (Malayalam ഗ്‍ക; Devanagari's half form क्‍ष and explicit halant क्‌ष; Bengali's য-ফলা forms). `"drop"`, the default: every joiner is dropped. Decide before you build the lexicon (see below). Optional. Engine 6. |
| `zwjForms` | `[["consonant", "letter"]]`, code points in hex: the consonant + `virama` + ZWJ is an older encoding of the letter and becomes it (Malayalam's chillus: ന + ് + ZWJ is ൻ). Optional. Engine 6. |
| `maxChars`, `maxWords`, `minCount` | Longest word kept, lexicon size, and the fewest times a word must appear. |
| `aksharantar` | Aksharantar's three-letter code for the language (`asm`, `mar`). |
| `direction` | `"rtl"` or `"ltr"`: which way the script is written. The apps mirror the suggestion bar for `"rtl"`; the keys stay as listed. Optional: without it the apps take the Unicode direction of the script's letters (Arabic's is right to left). Read by the apps only, so it needs no engine level. |

The engine normalises typed text with this file's `replace` list, the same way the builder
did, so typed words meet stored ones. Only `script`, and `block` or `letters`, are required by
the engine; the other script fields are optional, and the builder's (`maxChars`, `maxWords`,
`minCount`, `aksharantar`, `notes`) are ignored by it.

**Joiners.** The engine's normal form is NFC plus what each script's `lang.json` says about
ZWJ and ZWNJ. By default every joiner is dropped. With `"joiners": "afterVirama"` one that
follows the script's virama, before another character of the script, is kept; Malayalam's file
says so, and no rule here is written for Malayalam. The builder and the engine read the same
field, so the lexicon and what is typed always agree, but the choice is fixed when the lexicon
is built: change it later and typed words stop matching stored ones until you rebuild.

Which to choose? Unicode gives joiners after a virama a meaning in several scripts: in
Devanagari and Bengali, virama + ZWJ asks for a half form and virama + ZWNJ for a visible
virama. In running text they are rare and inconsistent (the same word with and without one),
and a word typed without the joiner would then miss the stored one with it. So Hindi,
Assamese and Tamil drop them, as their published lexicons were built: keeping them now would
make every stored word with such a cluster unreachable from text that has the joiner, and
the reverse. Keep them only if your language tells words apart by a joiner, or writes common
words that are wrong without one (Malayalam's ഛത്തീസ്‌ഗഡ്), and then leave ZWJ and ZWNJ out
of `replace`. A Devanagari `lang.json` that drops them can say nothing; the first two
`replace` entries below drop them before NFC as well, which mends a few mis-typed
sequences earlier. `make test-pack` checks every lexicon word against the pack's normal form.

A Devanagari example (Marathi). Copy this rather than the Assamese file for any `deva`
language:

```json
{
  "id": "mr",
  "name": "Marathi",
  "script": "deva",
  "block": ["0900", "097F"],
  "notWord": [["0964", "0970"]],
  "firstLetter": [["0904", "0914"], ["0915", "0939"], ["0958", "0961"], ["0972", "097F"]],
  "virama": "094D",
  "signs": [["093A", "093B"], ["093E", "094C"], ["094E", "094F"], ["0955", "0957"], ["0962", "0963"], ["094D", "094D"]],
  "bases": [["0915", "0939"], ["093C", "093C"], ["0958", "095F"], ["0978", "097F"]],
  "apostrophe": false,
  "replace": [["‍", ""], ["‌", ""], ["अा", "आ"], ["अो", "ओ"], ["अौ", "औ"], ["एे", "ऐ"]],
  "phraseBreaks": ["।", "॥"],
  "maxChars": 30,
  "maxWords": 300000,
  "minCount": 3,
  "aksharantar": "mar"
}
```

- `"‍"` is ZWJ and `"‌"` ZWNJ. Marathi's eyelash ra (र + virama + ZWJ) becomes a
  plain र्, as the engine's normal form makes it when the file doesn't ask for
  `"joiners": "afterVirama"`.
- The other `replace` entries mend independent vowels typed as अ or ए plus a sign.
- `signs` lists what must follow a consonant (vowel signs, nukta forms, the virama).
  Anusvara ं, candrabindu ँ and visarga ः are left out on purpose: they can follow a vowel
  (आं, अः), so a word with one after a vowel is still a word. Assamese leaves its ং ঁ ঃ out
  for the same reason.
- Letters outside the Hindi set still need to be in the ranges: Marathi's ळ (U+0933) and
  ऱ (U+0931) are inside `0915-0939`, ॲ (U+0972) is in `0972-097F`, and its sign ॅ (U+0945)
  in the signs' `093E-094C`. Check every letter your
  language uses against the [Unicode chart](https://www.unicode.org/charts/PDF/U0900.pdf).
- A `notes` object (as in the Assamese file) is for people; the tools ignore it.

**Finding mis-encodings.** `replace` mends text that was typed or encoded wrongly, but you can't
list what you haven't seen. The lexicon build (step 7) shows you: `lexicon-stats.txt` lists the
20 most frequent tokens that had a letter of your script and were still dropped as not words.
A real word near the top of that list is usually one mistake repeated across the text. Tamil's
ா + pulli typed for ர் (செய்தாா் for செய்தார்) is one such: a vowel sign can't carry a pulli, so
every such word was dropped until `["ா்", "ர்"]` went into `replace`. Add the pair and build
again. Tokens that really aren't words can stay dropped. Tamil's list today is of that kind:
`ம் 37744, ல் 37205, … 2011ஆம் 11692, … ே 8591` are lone letters with a pulli, years glued to a
suffix, and vowel signs cut loose in scanned books.

### 3. Get the text

Write `data/build-xx-text.sh`. Copy `data/build-as-text.sh` and change the source ids, the
dump file names and the Wikisource page namespace. It must leave plain text, one paragraph
per line, in `data/raw/text/xx.txt` and unzip Aksharantar into
`data/raw/aksharantar-<code>/extracted/`. Make it executable (`chmod +x`). Keep its
`reset_sources`, `count_source` and `report_sources` lines (from `data/text-sources.sh`), with
your language id and source ids: they record how many words each source gave.

Wikisource keeps its proofread text in the Page namespace, not the main one, so the script
passes `--extra-namespaces` to the extractor: a comma-separated list of namespace keys or
names, any of which selects it. The Assamese script gives the key, the English name and the
local name, `104,Page,পৃষ্ঠা`, so it works whichever the dump uses. Find yours in the dump's
`<siteinfo>`:

```sh
bzcat data/raw/xxwikisource/*.bz2 | head -60 | grep namespace
```

For Marathi, Page is key 104, `पान`, so the argument is `104,Page,पान`. Newer Wikisources number
it 250 (Odia `250,Page,ପୃଷ୍ଠା`, Punjabi `250,Page,ਪੰਨਾ`); with the wrong key the extract has only
the main namespace, and nothing says so. Not every language has all three wikis: Odia and
Punjabi have no Wikiquote, so their scripts read two sources. Then run the script once
by hand:

```sh
./data/build-xx-text.sh
```

The script ends with the words each source gave:

```
ta text by source: tawiki 35696375 words (28%), tawikisource 91188734 words (72%), tawikiquote 274299 words (0%)
warning: tawikisource is 72% of the words, so its style will lead the lexicon and the next-word pairs. …
```

The lexicon ranks words by how often the text has them, and next-word pairs come from the same
text. When one source is most of it (the script warns above 60%), the pack sounds like that
source. Tamil Wikisource is mostly older literature and scanned books, so literary forms
(`என்னும்`, `அவன்`) rank above everyday ones. If the big source is not how people write today,
keep only part of it: thin its extract before it is added to the text, so it is no more than
about half. In the text script, after that source is extracted and before the `cat`:

```sh
awk 'NR % 3 == 1' "$TEXT/xxws.txt.tmp" > "$TEXT/xxws.thin.tmp" && mv "$TEXT/xxws.thin.tmp" "$TEXT/xxws.txt.tmp"
```

That keeps every third paragraph. Delete `data/raw/text/xx.txt` to extract again, and say what
you kept in the source's manifest (`used_for`). If the big source is ordinary modern prose,
leave it whole: more text is better.

**Articles written from a template.** Some Wikipedias hold thousands of articles a bot wrote
from one template, usually one per village: about 30,000 in Telugu's (15.4M of its 76.1M
words) and 16,000 in Gujarati's (a fifth of its words). Their words then lead the lexicon
(Telugu's 20 most frequent had గ్రామం "village", దూరంలో "at a distance" and హెక్టార్లు
"hectares") and their sentences fill the next-word pairs. If step 7's `xx top 20` shows such
words, drop the repeated paragraphs from that source in the text script, as
`data/build-te-text.sh` and `data/build-gu-text.sh` do:

```sh
py "$PIPELINE/extract_wiki.py" "$RAW"/xxwiki/*.bz2 --out "$TEXT/xxwiki.txt.tmp"
py "$PIPELINE/drop_repeats.py" "$TEXT/xxwiki.txt.tmp" --out "$TEXT/xx.txt.tmp"
rm "$TEXT/xxwiki.txt.tmp"
```

`drop_repeats.py` drops a paragraph when half its runs of four words (digits read as 0) occur
50 times or more in the text, and prints how many paragraphs and words went. Ordinary prose
stays. Say so in the source's manifest (`used_for`).

The lexicon build in step 7 runs this script again, so it must be safe to re-run. The
Assamese one skips work that is already done: a second run only checks the downloads'
checksums and prints the counts again. This script is per language today (gap G4).

### 4. Write the romanisation tables

This is the part that needs someone who knows how people type the language in Latin letters.
There are three files in `data/langs/xx/`:

- `romanisation.hand.json`, which you write. Every letter, sign, cluster and final gets its
  spellings as `[latin, weight]` options, the most common first. Copy
  `data/langs/as/romanisation.hand.json`. Its `about` field and the docstring of
  `training/mlkb/langs/romanisation.py` explain each table: `inCluster` and `afterVirama`
  (spellings that change by context), `inherent` (the inherent vowel), `geminateSingle` (how
  often a doubled letter is typed once), and `skeleton` (the folds that make `oxom` and
  `asom` the same key).
- `romanisation-weights.json`, fitted from the hand tables to Aksharantar's manual rows
  (below).
- `romanisation.json`, the pack's file, exported from the weights by the lexicon build
  (step 7).

The hand file's weights are more than a starting point for the fit. Half of the
transliterator's synthetic training pairs (step 9, `--chat-share 0.5`) are spelled from the
hand file and half from the fitted weights. Aksharantar's annotators type carefully (they
wrote Gujarati's છ as `chh` 97% of the time), so the fitted weights describe them; write the
hand weights for how people type in chat (`che`, a final anusvara left out), and the model
sees both.

**The inherent vowel.** `inherent` has one option list per position: `medial` (inside a
word), `final` (at the end, after a lone consonant), `finalAfterCluster` (at the end, after a
cluster), `only` (the word is one consonant), and optionally `schwaDeleted`. With a
`schwaDeleted` list, the tables mark the medial inherent vowels that aren't pronounced
(a vowel, a lone consonant, the inherent vowel, a lone consonant, a vowel; scanned right to
left) and spell those from `schwaDeleted` instead of `medial`: Assamese ৰাজনীতি is `rajniti`,
not `rajoniti`. Leave `schwaDeleted` out if your language doesn't drop these vowels in
speech. If it does, check how people actually type them before choosing its weights: Hindi
speakers drop them in typing, but Marathi's Aksharantar annotators mostly kept them (with
canonical `""` the lookup found 58% of the manual rows; with `a`, 96%).

When speech drops them and typists do either, also set `"dropBetweenConsonants": "a"` in the
hand file's `skeleton`: every `a` with a consonant on both sides then leaves the key, so
`karanar` and `karnar` find the same word. Hindi's file has it, and Marathi, Bengali, Maithili
and Bhojpuri use it (Marathi's calibration skeleton went from 96.2% to 98.0%, and the held-out
rows, whose typists drop far more of them, from 52.0% to 70.3%). Urdu, which writes no short
vowels at all, has it too, with every vowel folded to `a` first, so its skeleton is the
consonants and whether the word starts or ends with a vowel. The fit puts first whatever
spelling the typists used most; to keep a slot's canonical spelling first whatever they typed,
name the slot in the hand file's `keepCanonical` (`"inherent.medial"`, or a whole table).
Urdu keeps the medial `a`: typists usually leave it out, but a walk without it would run
ک and ہ together as `kh`, the aspirate. The cost is the one every
fold has: words that differ only by such a vowel share a key.

**Calibrate.** Fit the weights to the human pairs:

```sh
cd training
uv run python -m mlkb.langs.calibrate --lang xx            # report only
uv run python -m mlkb.langs.calibrate --lang xx --write    # write the weights
```

The report first says which pairs it calibrates on: Aksharantar's manual rows in `valid`, or,
when `valid` has none, a quarter of `train`'s manual rows held out by word (never a test
word; step 9's training leaves them out too). When neither has any (Telugu's manual rows are
all in the held-out test split), it calibrates on `valid`'s mined IndicCorp rows and says so:
those were matched by a model, not typed by people, so read the numbers as rough. It stops
with an error only if there are none of those either. Then it shows how often the tables give exactly what people typed
(`canonical`), whether they can spell it at all (`producible`), and how often the lookup
would find the word (`skeleton`, `prefix`).

- **Tune on the calibration numbers only.** Change the hand tables and run it again until the
  `calibration (after)` numbers stop improving.
- **See which pairs fail.** `--misses` lists the calibration pairs the fitted tables fail on
  (`--misses 30` for the first 30): first the ones the tables can't spell at all, then the
  ones whose typed skeleton isn't the word's key, so the lookup would miss them.

  ```
  misses: 10 of 200 calibration pairs (4 the tables can't spell, 6 with another skeleton)
    word	typed	canonical	key	typed skeleton	why
    ஏழரை	yaezharai	ezharai	elarai	ielarai	does not align
    இடமே	edame	idame	itame	etame	skeleton differs
  ```

  Each row shows what a person typed, what the tables spell, and the two keys that should
  have been equal. Look for a kind of miss, then add the spelling or the fold that mends all
  of that kind: Tamil's first run showed ய் typed `i` (`seithi`), which one fold (y to i)
  fixed. Test pairs are never listed.
- **The test numbers are held out.** They are for your pull request's description. Changing
  the tables to raise them tunes on the test set, and then they no longer say how well the
  tables do on words nobody tuned for.
- **Run `--write` after every edit, the skeleton's too.** `romanisation-weights.json` holds a
  copy of the whole hand file, skeleton included, and step 7 exports from it. An edit to the
  hand file's `skeleton` that isn't followed by `calibrate --write` changes nothing in the
  pack, and nothing warns you.

Without Aksharantar data, copy the hand file to `romanisation-weights.json` and skip the
fitting (gap G6). Or, when a language with Aksharantar data is written in your script and
typed with the same habits, borrow its pairs: put its code in your `lang.json`'s
`aksharantar`. Bhojpuri (`data/langs/bho/`) names Hindi's, `hin`: its tables are fitted to
Hindi's manual rows, and its transliterator would train on its own words spelled by those
tables beside Hindi's mined rows. No tool needs a change. What you lose is the measurement:
`data/eval/bho/translit-human.tsv` is then Hindi words, most of them not in a Bhojpuri lexicon,
so step 11's numbers say how the pack spells same-script words, not how well it knows
Bhojpuri. Say so in the pack's notes, and credit Aksharantar.

**Few manual rows.** Assamese calibrates on 1,794 pairs. Tamil has 776 manual rows in `train`
and none in `valid`, so it calibrates on a quarter of them: 200 pairs. Under 500 pairs the
report says so, and three things follow:

- One pair is a large step (0.5 points of 200), so don't chase a change of a pair or two. Fix
  kinds of miss.
- The fit can't correct what it rarely sees. A letter typed fewer than about 20 times in the
  calibration pairs keeps mostly its hand-set weights, so the order you write is the order
  that ships. Put the commonest spelling first yourself, from what you know of how people type.
- Training (step 9) picks its best checkpoint by scoring on the same pairs, so that score
  moves by a few points from one evaluation to the next, and the step it stops at is partly
  chance. Judge the model by the held-out numbers of step 11, which use thousands of rows.

You can't add your own typed pairs yet: the tools read human pairs only from Aksharantar
and, for a language listed in `DAKSHINA` in `training/mlkb/langs/human.py`, from Dakshina's
lexicon (gap G6). Punjabi is listed: Aksharantar has no typed Punjabi pairs outside its test
split, so its tables calibrate on Dakshina's train split, its transliterator trains on those
pairs beside Aksharantar's mined ones and validates on the dev split, and the test split is
held out. Dakshina is CC BY-SA 4.0: a pack that uses it credits it, as Punjabi's does.

**One letter, several sounds.** Some scripts write several sounds with one letter. Tamil has
one letter for k, g and h (க), one for t and d (ட), one for th and dh (த), one for p and b
(ப), and one for s, ch and j (ச). People type the sound they hear. The tables have no "at the
start of a word", "between vowels" or "after a nasal" context (`inCluster` and `afterVirama`
depend on the letter's place in a cluster, not on its neighbours), so do this instead, as
`data/langs/ta/romanisation.hand.json` does:

- list every spelling on the letter: `"க": [["k", 0.65], ["g", 0.3], ["h", 0.05]]`;
- fold each group to one symbol in `skeleton`, so `magan` and `makan` share a key: g to k,
  d to t, b to p and j to s in `letters`, and `["th", "t"]`, `["dh", "t"]`, `["ch", "s"]` in
  `replace`;
- where a neighbour fixes the sound, put the pair in `clusters`: a nasal and its stop (ங்க
  `ng`, ஞ்ச `nj`, ண்ட `nd`, ந்த `nth`, ம்ப `mb`) and clusters said as one sound (ற்ற `tr`,
  ன்ற `ndr`).

Every fold puts more words under one key. Words that share a key are told apart only by
their frequency and by how closely the typed letters match each one's canonical spelling, so a
fold costs a little ranking for every word it merges. Add one when `--misses` shows a kind of
spelling that never reaches the key: Tamil folds o to u because people type `oo` for both ஊ
and ஓ. Leave it out when it would mend a pair or two: folding e to i would have caught 3 of
Tamil's 200 pairs and merged every e and i word. After step 7, check with step 11's script
that common words still come first.

**Letters the skeleton drops.** Two optional steps of the pack's skeleton can be written in
the hand file's `skeleton`, in the pack file's own shape; the export copies them:

```json
"dropTrailing": {"letters": "nm", "afterVowel": true, "minLength": 3},
"dropBetweenConsonants": "a"
```

- `dropTrailing` removes one final letter of `letters` after a vowel, before any fold, when
  the typed text is at least `minLength` long. Gujarati's and Telugu's files have it for the
  anusvara at the end of a word, which is typed `n`, `m` or not at all (કરવું `karvun`,
  `karvum`, `karvu`): all three get one key. The price is that a word really ending in `n` or
  `m` shares its key with the word without it (નામ with ના).
- `dropBetweenConsonants` removes every such letter that has a consonant on both sides, after
  the folds. Gujarati has `"a"`, as Hindi's file does: the pack's walk has no schwa rule and
  keeps every medial inherent vowel (કરવું is `karavun`), and people type it or not (`karvu`,
  `gujrati`). Don't use it for a language that always says the vowel (Telugu, Tamil).

On Gujarati's 1,607 calibration pairs the two took the skeleton from 84.1% to 86.3%, and the
engine's first native suggestion on the held-out rows, lexicons only, from 29.0% to 32.2%;
what they are for (chat spellings) the annotators' pairs hardly measure.

**One spelling list per letter, wherever it stands.** The anusvara is the clearest case: `n`
inside a word (before a labial `m`) and `m` or nothing at its end. The fit makes the most
typed spelling canonical, and the key of every word then has that letter in both places.
Gujarati lists only `n` and nothing for ં (not `m`, though annotators type a final `m` more
than `n`), folds `mb` and `mp` to `nb` and `np`, and drops the final letter with
`dropTrailing`. Telugu, which has no dropped vowels to bring a real `m` next to a consonant
when typed, folds `m` before every consonant to `n` (a `replace` entry per consonant spelling)
and drops the final one. Folding `m` to `n` everywhere also works and costs more: every મ and
ન word pair shares a key (Gujarati `naam` gave માં before નામ).

**Spellings that spell the same word: `looseSpelling`.** The skeleton only finds candidates.
To rank them, the engine asks whether the typed letters spell a word out: are they its
canonical spelling, up to the differences your language's typists don't hear? A word spelled
out beats one the letters only resemble, and may replace an English word. Those differences
are the hand file's `looseSpelling` (the export copies it into `romanisation.json`):

```json
"looseSpelling": {
  "singles": {"d": "t"},
  "pairs": [["ee", "i"], ["oo", "u"]],
  "collapse": "aiu"
}
```

- `singles`: a letter read as another. Malayalam has `d` as `t` (ട is typed either way:
  `veedu` for വീട്, canonical `veetu`). Tamil has `d`, `g`, `b`, `j` as `t`, `k`, `p`, `s`, one
  letter each in its script. Hindi and Assamese have none: द and त are different letters, so
  `dal` must not count as spelling ताल.
- `pairs`: two letters in a row read as one letter (`ee` as `i`, `oo` as `u`).
- `collapse`: letters that count once however often they repeat (`aa` as `a`). Leave
  consonants out when a doubled consonant is another word (Malayalam `veedu` / `vittu`).

Every part is optional; `{}` means only the same letters spell the word. Keep it narrower than
the skeleton: a fold here stops the typed letters from telling two words apart. Without the
field in the hand file the export writes `{}`. `scripts/pack.py --check` fails on a
`romanisation.json` that lacks it, because the engine reads such a file as it read the packs
published before the field, with Malayalam's three habits above. Measure a change with step
11's `mlkb.eval.pack`: Tamil's voiced letters moved its held-out top-1 from 47.8% to 47.9%
and the first slot from 24.8% to 25.0%; dropping the field's vowel folds cost Hindi a point of
first slot.

**A final sign people leave unwritten: `optionalFinals`.** Some signs at a word's end are
written about half the time. Hindi's anusvara and chandrabindu are the case: नहीं is `nahin`
and `nahi`, हूँ `hun` and `hu`, में `mein` and `me`. Don't drop the letter in the skeleton
(`dropTrailing` `n` would also turn दिन, `din`, into दी), and don't give the sign an empty
spelling in `finals` unless nobody writes it (Assamese's chandrabindu). List it instead:

```json
"optionalFinals": ["ं", "ँ"]
```

Each entry is a key of `finals`. A word that ends in one is then stored under two keys, the
skeleton of its walk and the skeleton of its walk without that sign, and typing or swiping
either spelling finds it and counts as spelling it out. A sign inside a word is always
written, and so is one that follows a bare consonant (स्वयं). The field is in the pack's `romanisation.json` (and in the hand file, from which the
export copies it); after adding it, rebuild the romanised lexicon from the tables
(`lexbuild-rekey romanisation.json <id>.fst <romanised>.fst` does it without the corpus) and
run `scripts/pack.py --write`. `lexbuild --lang` and `lexbuild-native --romanisation` write
both keys; `mlkb.langs.romanised_keys` writes one, so for a `data/langs` language that uses
the field `data/build-pack-lexicons.sh` runs `lexbuild-rekey` on the pack after it. For such
a language write the field in both `romanisation.hand.json` and `romanisation-weights.json`,
beside `finals` (Marathi, Maithili and Bhojpuri have it).

**Three more for a skeleton that drops a vowel between consonants** (Hindi's
`dropBetweenConsonants: "a"`), all optional:

- `"alsoBetweenConsonants": "u"` in `skeleton`: a letter people write for the dropped vowel
  (`hum`, `humse`, `humara` for हम, हमसे, हमारा, which walk with `a`). Typed letters with it
  between two consonants are also looked up with it dropped, behind the words that really
  have it (`kuch` is still कुछ first). It doesn't change the lexicon. For the few such words
  people swipe most, add rows to `chat-forms.tsv`: a swipe doesn't use this field. In a
  `data/langs` language it goes in the `skeleton` of the hand and weights files, and the
  export copies it (Maithili and Bhojpuri have it: `hum` for हम, `humar` for हमार). Measure
  it: on held-out rows it moved a few first suggestions the wrong way in every language
  tried, most in Marathi, which was left without it.
- `"alsoBesideConsonant": {"e": "h"}` in `skeleton`: a letter people write for the dropped
  vowel only beside certain consonants. Hindi's short a before ह is said and written e
  (`kehte`, `rehna`, `pehle` for कहते, रहना, पहले, which walk `kahte`, `rahna`, `pahle`), and
  so is the one after that ह (`behen`, `sheher` for बहन, शहर). The key is the letter, the
  value the consonants: between two consonants, the letter may stand for the dropped vowel
  when the consonant after it is one of them, or the consonant before it is and the vowel
  before that one is dropped as well (the second e of `behen`; not the e of `her`, whose h
  begins the word). Typed letters are then also looked up with it dropped, as for
  `alsoBetweenConsonants`: beside what they say as written, never in the middle, and behind
  the words that really have the letter (`mehnat` is still मेहनत first). Unlike that field, a
  swipe uses this one: the finger may go by way of the letter there, and a word found so
  ranks a little behind one that has the letter. That has a cost: a frequent word can now
  be drawn through a rarer one's keys (Hindi's `chehra` swiped gives कहा more often than
  चेहरा, which it gave about a third of the time before). It doesn't change the lexicon. Words written this way all the time still belong
  in `chat-forms.tsv`, which is what puts them in the middle (Hindi lists the forms of कहना
  and रहना, `behen`, `shehar`, `pehle`). Measure it on your held-out rows, typed and swiped,
  before keeping it.
- `"alsoKept": true` in `skeleton.dropTrailing`: a word that ends consonant, dropped vowel,
  dropped trailing letter (जगह `jagah`: `jga`) gets a second key with the trailing letter
  kept (`jgh`), so a swipe that goes j, g, h without returning to `a` finds it. It adds keys:
  rebuild the romanised lexicon as for `optionalFinals`.

**Swiping romanised words.** A swipe over the Latin keys is matched against the skeletons of
your romanised lexicon, and the rules for that come from your skeleton, with nothing to write:
a skeleton letter may be drawn through every letter the skeleton folds into it (Tamil's `k`
through `g` and `q`, its `s` through `c` and `j`; Hindi's `t` only through `t`), the finger
may pass through `h` after a letter that a pattern ending in `h` folds to (`th` to `t`), and it
may go on to the letters the skeleton drops from a word's end (`dropTrailing`,
`dropFinalVowel`, and any `singles` letter that becomes one of those vowels: Assamese drops a
final `a` and reads `o` as `a`, so a swipe may end on `o`). If your skeleton has
`dropBetweenConsonants`, the finger may also go to one of those letters between any two
consonants of a skeleton: Hindi's `karne` is the skeleton `krne`, and a swipe through
k-a-r-n-e finds it. With `alsoBesideConsonant` it may go to that letter in the places the
field allows (k-e-h-t-e for `khte`, कहते). If the derived lists are wrong for your language, a `swipe` section in
`romanisation.json` overrides the aspirated and trailing letters: `"swipe": {"aspirated": "ktpb", "trail": "h"}`
(Hindi's file has one). The export doesn't write it; add it to the pack's file by hand after
step 7 and run `scripts/pack.py --write` again. A swipe also fits the letters of each row of
your `chat-forms.tsv` as they are written, so a chat spelling no skeleton holds (Hindi's
`kyu` for क्यों) can be swiped as well as typed.

**A spelled guess without a model: `fromLatin`.** When nothing else answers for typed
letters (no lexicon word, and no transliterator, or one that wasn't asked because the letters
are an English word), the engine spells them by rule and offers that in a side slot. With no
work from you the rules are your tables turned round: each Latin spelling of a consonant or a
vowel gives its letter back. That is rough where two letters share a spelling, since the
tables don't say which is commoner and the lower code point wins (Hindi `t` gives ट, not त;
Tamil `tamil` gives டமில்). On the held-out sets, with the model taken out of the pack, it
answers many more words but few of them right (first native suggestion right: Tamil 33.1% to
36.0%, Assamese 34.1% to 35.0%, Hindi unchanged at 50.0%; no native suggestion at all: 55% to
23%, 51% to 20%, 32% to 24%). A pack with a transliterator hardly uses it.

For a better guess, add a `fromLatin` section to the pack's `romanisation.json` (by hand,
after step 7, then `scripts/pack.py --write`; the export doesn't write it). Malayalam's is
the worked example, and `RawFromLatin` in `engine/core/src/translit.rs` documents each part.
Two tables are required, read left to right with the first match taken, so put longer
spellings first:

```json
"fromLatin": {
  "consonants": [["kh", "ख"], ["k", "क"], ["t", "त"], ["b", "ब"]],
  "vowels": [["aa", "आ", "ा"], ["a", "अ", ""], ["i", "इ", "ि"]],
  "bareFinal": true
}
```

- `consonants`: `[latin, native]`. List every letter a to z that isn't a vowel spelling, or a
  word with a missing letter gets no guess.
- `vowels`: `[latin, letter, sign]`: the independent letter, and the sign on a consonant
  (`""` for the inherent vowel).
- `bareFinal`: a word-final consonant is written bare (Hindi कमल). Without it, it takes the
  virama, or its `finalForms` entry (`[native, final]`: Malayalam's chillus, ന to ൻ).
- The rest encode habits of Malayalam's typists that other languages may share:
  `longBeforeOpenSyllable` (a short e or o is long before one consonant and a vowel: `mohan`),
  `finalVowels` (a final `y` after a consonant is ി), `fused` (`r` + `i` after `k` is ൃ),
  `afterConsonant` (`n` after `sh` is ണ), `betweenVowels` (a lone `ch` is ച്ച) and
  `beforeConsonant` (`r` before a consonant is the chillu ർ).

**Doubled letters.** A consonant doubled across a virama (க்க) is one slot. By default it is
spelled with the letter's first Latin letter doubled (க `k` gives `kk`, but த `th` gives
`tth`), or, `geminateSingle` of the time, like the single letter. When people type it another
way (`thth` for த்த), add the doubled letter to `clusters` with its spellings: a `clusters`
entry replaces the default.

**Keys of more than one code point.** A key in `consonants` may be several code points, for a
sound the script writes that way: Assamese's letter + nukta (য়). So may a key in
`independentVowels`: Bengali writes the vowel of English "and" as অ + the -ya sign + the aa
sign (অ্যাপ, অ্যান্ড), one key of four code points. For the lexicon to keep such words, the
letter the signs follow must be in `lang.json`'s `bases` (Bengali lists অ and এ). The longest
key that matches is used. A sign written before the consonant it doubles (Gurmukhi's addak ੱ:
ਪੱਕਾ pakka) has no table of its own; Punjabi lists each addak + consonant as a `consonants` key
spelled with the letter doubled (`data/langs/pa/romanisation.hand.json`, gap GO3). One limit: `finals` are matched before consonants, so a key whose first code point
is also in `finals` is never reached. Tamil lists ஃப (`f`) and ஃஜ (`z`) in `consonants`
and ஃ in `finals`, so ஃப is read as ஃ, then ப (`p`). Typed `f` still finds those words
only because Tamil's skeleton folds f to p (gap G35).

**What the fitting changes.** The fit reorders each option list by how often people typed it,
so the canonical spelling (the first option) becomes the most typed one. For Marathi it
turned ा ी ू into `aa` `ee` `oo` and झ into `z`, so the exported table spells मराठी
`maraathee`. That is fine. The engine uses the canonical spelling in two places: as the
lookup key, after the skeleton folds (give your `skeleton.replace` pairs like Marathi's
`["aa", "a"]`, `["ee", "i"]`, `["oo", "u"]` so both styles get one key), and to rank words by
how closely the typed letters match it, which treats `aa`/`a`, `ee`/`i` and `oo`/`u` as the
same anyway. Don't edit `romanisation-weights.json` by hand: the next `--write`
replaces it. If an option is wrong, take it out of the hand file and calibrate again.

**Notes.** `romanisation.json` has a `notes` field for people reading the file. The export
copies it from the hand file's `notes` (a string, or an object of topic to text, as in the
Assamese hand file), so write your language's notes there. The Assamese hand file also has
`inClusterLetters` (`শষস`): letters whose `inCluster` spelling holds in every two-letter
cluster they are in, which the export lists as clusters. Leave it out unless your language
has such letters.

### 4b. Optional: endings glued onto words (`suffixes.tsv`)

Skip this unless your language glues clitics and case endings onto words the way Malayalam
does (സുഖം + ആണോ is സുഖമാണോ) and people type such forms all the time. Most of them never
occur in your text, so the lexicon doesn't have them. With a suffix table the engine reads a
typed word that has no whole-form match as a stem from the lexicon plus one or two endings,
and joins them by your script's sandhi. Malayalam is the only pack that ships one; nothing in
the mechanism is Malayalam's, but nobody has measured it for another language yet, so treat
a new table as an experiment and check it with step 11's script.

Two files. **`data/langs/xx/lang.json` gets `suffixJoins`**, the sandhi (code points in hex;
every part is optional):

| Part | Meaning | Malayalam | Tamil |
|---|---|---|---|
| `vowelSigns` | `[vowel, sign]`: the sign an independent vowel becomes on a consonant; `""` for the consonant's own vowel. A vowel suffix must start with one of these. | അ `""`, ആ ാ, ഇ ി … | அ `""`, ஆ ா, இ ி … |
| `elide` | Final signs that give way to the suffix's vowel, after a consonant. | ് and ു (കഴിച്ചു + ഓ is കഴിച്ചോ) | the pulli ் (அவன் + இடம் is அவனிடம்) |
| `glides` | `{"glide": consonant, "after": ranges}`: the consonant put between a stem ending in one of `after` and a vowel. | വ after u and o sounds, യ after the rest | வ after ா ு ூ ொ ோ ௌ, ய after ி ீ ெ ே ை (பள்ளி + இல் is பள்ளியில்) |
| `inherentGlide` | The glide after a bare consonant (its own vowel). | യ (അമ്മ + ഓട് is അമ്മയോട്) | வ |
| `finalForms` | `[letter, consonant]`: a word-final form that becomes its consonant before a vowel. | the chillus (അവൻ + ഓട് is അവനോട്) | none |
| `nasal` | `[ending, consonant]`: a stem ending in `ending` takes a vowel suffix as the consonant + sign, takes no case ending, and loses the ending to an oblique one. Several code points are separated by spaces. | `["0D02", "0D2E"]`: ം, മ (മരം + ത്തിൽ is മരത്തിൽ) | `["0BAE 0BCD", "0BAE"]`: ம், ம (காலம் + த்தில் is காலத்தில்) |
| `consonantAfter` | Finals, besides vowels and consonants, that take a suffix starting with a consonant. | ർ and ൾ (അവൾ + ക്ക് is അവൾക്ക്) | none |

Tamil's, complete (the engine's tests join the examples above with exactly this):

```json
"suffixJoins": {
  "vowelSigns": [["0B85", ""], ["0B86", "0BBE"], ["0B87", "0BBF"], ["0B88", "0BC0"],
                 ["0B89", "0BC1"], ["0B8A", "0BC2"], ["0B8E", "0BC6"], ["0B8F", "0BC7"],
                 ["0B90", "0BC8"], ["0B92", "0BCA"], ["0B93", "0BCB"]],
  "nasal": ["0BAE 0BCD", "0BAE"],
  "elide": ["0BCD"],
  "glides": [
    {"glide": "0BB5", "after": [["0BBE", "0BBE"], ["0BC1", "0BC2"], ["0BCA", "0BCC"]]},
    {"glide": "0BAF", "after": [["0BBF", "0BC0"], ["0BC6", "0BC8"]]}
  ],
  "inherentGlide": "0BB5"
}
```

It needs `bases` (your consonants) in the same file: a consonant, for the joins, is a `bases`
letter that is not a combining mark.

**`packs/xx.neopack/suffixes.tsv`** lists the endings, one per line, tab-separated, and the
manifest's `files` gets `"suffixes": "suffixes.tsv"`:

```
# latin	native	join	[inner]
il	இல்	case	inner
thil	த்தில்	oblique	inner
a,aa	ஆ	vowel
kku,ku	க்கு	consonant
```

- `latin`: the ways people type the ending, comma-separated, lower-case a to z. It is matched
  against the end of the typed word; what is left must be at least three letters.
- `native`: the ending in your script.
- `join`: `vowel` (starts with a vowel, which becomes a sign on the stem), `case` (the same,
  but a stem with the `nasal` ending doesn't take it), `consonant` (starts with a consonant;
  appended to a stem that ends in a vowel, a consonant or a `consonantAfter` letter), or
  `oblique` (replaces the `nasal` ending; only such stems take it).
- `inner`: write it when another ending may follow this one.
- Row order breaks ties: among cuts of equal length, earlier rows are tried first. Keep the
  table small. Every row is a guess about words the engine has never seen, so each ending
  must be common.

Two things about the typed stem come from your data, not from code: an oblique ending is
typed without the stem's nasal (`kaala` + `thil`), so the engine adds how your tables spell
the `nasal` ending (`m`) before it looks the stem up; and a stem typed with its glide
(`ammay` + `odu`) is also looked up without it, the glides being your `glides` and
`inherentGlide` letters as your `consonants` table spells them. A language without a
`nasal` or glides gets neither.

### 5. The manifest

Write `packs/xx.neopack/manifest.json` by hand now (gap G8): the lexicon build in step 7
writes its checksums, and stops at the start if it is missing. Copy
`packs/as.neopack/manifest.json`:

```json
{
  "format": 1,
  "id": "xx",
  "version": "2026.11.10",
  "name": "Xx",
  "nativeName": "<the language's name in its script>",
  "script": "deva",
  "romanised": { "name": "Romanised Xx" },
  "minEngine": 6,
  "files": {
    "lexicon": "xx.fst",
    "romanisedLexicon": "romanised.fst",
    "nextWords": "next.fst",
    "translit": "translit.mlkbnn",
    "romanisation": "romanisation.json",
    "layout": "layout.json"
  },
  "sha256": {},
  "credits": [
    { "name": "Xx Wikipedia", "licence": "CC BY-SA 4.0", "url": "https://xx.wikipedia.org/" },
    { "name": "Xx Wikisource", "licence": "CC BY-SA 4.0", "url": "https://xx.wikisource.org/" },
    { "name": "Aksharantar (AI4Bharat)", "licence": "CC BY and CC0 1.0",
      "url": "https://huggingface.co/datasets/ai4bharat/Aksharantar" },
    { "name": "English Wikipedia", "licence": "CC BY-SA 4.0", "url": "https://en.wikipedia.org/" }
  ]
}
```

- `version` is a date, `YYYY.MM.DD`, compared part by part. Raise it on every change you
  publish. The app won't install an older version over a newer one.
- `minEngine` is the oldest engine that reads everything in the pack right. An app whose engine
  is older refuses the pack; an app that accepts it but is older than a feature the pack uses
  would ignore that feature without a word (Hindi's `optionalFinals` on an engine 6 app: typed
  `nahi` finds nothing). So it is checked, not chosen: the engine works out the level a pack
  needs from its script, its roles (an `lm` or `translit` in a script other than Malayalam's),
  its `lang.json` and every field of its `romanisation.json` (`Pack::needs` in
  `engine/core/src/pack.rs`, which lists every level). To see it, once the pack has its files
  (after step 7, and again before you publish):

  ```sh
  make pack-needs PACK_LANG=xx
  ```

  ```
  ta: minEngine 6, needs 6: ok
    6: romanisation.json has looseSpelling
    4: a translit in script taml, decoded in its own block
    4: script taml, which the engine has no copy of: its rules are the lang file
    3: an lm in script taml, whose native pieces are its own
    2: script taml, not Malayalam's
  ```

  `PACK_DIR=<folder>` in place of `PACK_LANG` checks a pack folder anywhere.

  Write that number. `make test-pack` (step 10) and `make test-engine` fail while the
  manifest's is lower, naming each thing that needs more; nothing raises it for you. Most
  new packs need 6, as every `romanisation.json` has `looseSpelling`, and a pack that uses
  Hindi's habits (`optionalFinals`, the skeleton's `alsoBetweenConsonants`,
  `alsoBesideConsonant`, `dropTrailing.alsoKept`) or a `deva` or `beng` pack whose
  `lang.json` isn't Hindi's or Assamese's needs 7, and one whose `lang.json` has a
  `charTable` (Urdu's) needs 8. Malayalam's own pack stays at 1: engine 1
  has its rules in code. A field or role the engine has no level for is an error too: an
  engine change that adds one raises `PACK_ENGINE`, says in its comment what the new level
  reads, and gives the field its level in `Pack::needs`.
  `pack.py --write`, which the lexicon build runs at the end of step 7 (and you run again in
  step 10), copies `data/langs/xx/lang.json` into the pack and adds `"lang": "lang.json"` to
  `files`, but leaves `minEngine` alone.
- `romanised.name` is what the typed-in-Latin form is called (Manglish, Hinglish).
- `sha256` starts empty; the build fills it in.
- `files` lists every file the finished pack will have, including ones not built yet
  (`translit.mlkbnn` comes in step 9). `pack.py --write` lists the absent ones as it goes.
- `credits` lists every source in step 1 that the pack was built from, plus English Wikipedia,
  because `next.fst` carries its English pairs. Each credit needs `name`, `licence` and `url`,
  as in the Assamese manifest. Two fields are optional: `licenceUrl`, needed for any licence
  that `scripts/publish-packs.py` doesn't know (else the credits page shows it without a
  link), and `attribution`, which the credits page otherwise takes from the
  `data/manifests` source whose `homepage` is the credit's `url`. The app's About screen and
  the site's credits page read this list.
- `englishFingerprint` (Hindi has one) names the English lexicon that the pack's next words and
  LM were built against. Leave it out: no tool writes it yet, and a wrong one makes a test
  fail.
- Every value in `files` must be a plain file name (no `/`, not starting with `.`).

### 6. The keyboard layout: `layout.json`

This is the native-script keyboard, and the manifest lists it, so write it before the
build. The format is in the
[packs design](design/2026-11-06-m6-language-packs-design.md), under "Pack format".
Copy `packs/as.neopack/layout.json`:

```json
{
  "format": 1,
  "spaceBar": "অসমীয়া",
  "languageKey": "অ",
  "pages": [
    { "id": "letters", "label": "কখ", "rows": [["ক", "খ", "…"], ["…"], ["…"]] },
    { "id": "more",    "label": "শষ", "rows": [["…"]] },
    { "id": "vowels",  "label": "অআ", "rows": [["…"]] }
  ],
  "longPress": { "ড": ["ড়"] },
  "vowelSignStrip": ["া", "ি", "ী", "ু", "ূ", "ৃ", "ে", "ৈ", "ো", "ৌ", "ং", "ঁ", "ঃ", "ৎ", "্"]
}
```

- `spaceBar` is the space bar's text while this layout is up. `languageKey` is the key that
  switches to it from another layout.
- `pages` come in the order the layout opens them. Each `label` is the label of the key that
  shows the page; by convention it is the page's first two letters. Keep the order `letters`,
  `more`, `vowels`, so the bottom row is the same in every language.
- `rows` list what each key types. A key may type more than one code point (a nukta letter, a
  conjunct). The keyboard adds backspace, the page keys, space and return itself. The
  existing layouts use three rows of up to 10 keys.
- `longPress` maps a key's text to its long-press options.
- `vowelSignStrip` is the row of vowel signs shown after a letter a sign can attach to. Each
  entry must be one character. Leave it empty for no strip.
- `punctuation` (optional) maps the Latin marks to your script's own:
  `{".": "۔", ",": "،", "?": "؟", ";": "؛"}` for Urdu. After a word in your script, those keys
  type your marks, and the double-space full stop does too. The symbols page shows them.
- `digits` (optional) lists your script's digits, zero to nine (exactly ten). They go on the
  symbols page's top row, and as long-press options on the Latin letters' top row.
- `currency` (optional) is the currency sign on the symbols page: `"₨"` for Urdu. Without it
  the page shows ₹.
- Right-to-left scripts need nothing more here. Rows are listed left to right, as they
  appear. The Android app mirrors the suggestion bar when the script is right to left. It
  reads the direction from `direction` in `lang.json` (`"rtl"` or `"ltr"`) when given, or
  else from the Unicode direction of the script's letters.
- The app refuses a layout with another `format`, no pages, or an empty key.

`pack.py --check` (step 10, and `make test-pack`) reads the layout and fails on what the app
would refuse, on a strip entry that isn't one character, on a key, long-press option or strip
sign that isn't in your script (punctuation is fine), and on a long-press entry for a key no
page has. It can't tell you whether the layout is good to type on. Ask someone who types the
language.

**A small alphabet.** The pages exist because most Indic scripts have more letters than 30
keys. Tamil has 18 consonants and 5 Grantha letters, so `letters` holds them all with room to
spare. Then:

- keep the three pages and their order anyway, so the bottom row matches other languages;
- give `more` what is typed least: the script's own digits, rare signs, conjuncts that are
  one key (க்ஷ, ஸ்ரீ);
- choose each `label` to say what the page holds. "The first two letters" is only a
  convention: Tamil's `more` page starts with digits, so that rule gives `௧௨`, which is a
  fair label for a page of digits and signs;
- use the spare keys on `letters` for what people reach for most (the pulli, aytham), not for
  a second copy of the vowels.

### 7. Build the lexicons

```sh
make pack-lexicons PACK_LANG=xx
```

This runs `data/build-pack-lexicons.sh xx`. Because `data/langs/xx/lang.json` exists, it:

1. runs `data/build-xx-text.sh`;
2. builds `lexbuild-native` and counts words and word pairs into `xx.fst` and `next.fst`. The
   English pairs of `next.fst` are copied from `packs/ml.neopack/next.fst`, which was counted
   against the same English lexicon. Set `ENGLISH_NEXT=<path>` to use another;
3. exports `data/langs/xx/romanisation.json` from the weights;
4. keys every word by its romanised skeleton and builds `romanised.fst`;
5. copies the files into `packs/xx.neopack/` and writes the checksums into the manifest.

Before any of that it checks for what earlier steps make, and stops with a list of what is
missing: `data/build-xx-text.sh` (step 3), `data/langs/xx/romanisation-weights.json`
(step 4) and `packs/xx.neopack/manifest.json` (step 5).

Read `data/langs/xx/lexicon-stats.txt` afterwards:

- `xx top 20` should be the language's common short words. If they are not, fix `lang.json`.
  Marathi's started `आहे आणि व या हे ते एक हा नाही`.
- `xx dropped top 20` lists the most frequent tokens that had a letter of the script and were
  dropped as not words, each with its count. Look for real words in it: see "Finding
  mis-encodings" in step 2.
- `xx text by source` repeats step 3's words per source, for whoever reviews the pack.

The romanised-keys step prints a line such as `299,999 keys for 300,000 words`, and under it
the words that got no key. Such a word's skeleton is empty: the tables have no spelling for
its letters, or every letter folds away. Latin typing can't reach it; the native keyboard
can. One or two rare words are fine. Many means a letter is missing from the tables.

The build's last step (`pack.py --write`) also copies `lang.json` into the pack. Now set
`minEngine` to what `make pack-needs PACK_LANG=xx` prints (see step 5).

Every build exports `romanisation.json` again from the tables, so fix its notes in the hand
file (step 4), not in the exported file.

You can try the lexicons now, before training anything: step 11's script works on a pack
without `translit.mlkbnn` if you take `translit` out of the manifest's `files` for the moment
(the script stops on a listed file that is missing). Put it back before step 9.

Hindi is built a different way: `data/lang/hi.conf` plus `lexbuild --lang`, which takes the
Devanagari rules from the engine's copy of `data/langs/hi/lang.json` instead of a
`--lang-config`, and it needs the full English text. Use the Assamese path for a new language.

### 8. Next-word data

Step 7 already wrote `next.fst`: pairs of adjacent words from your text, plus the English
pairs. Nothing more is needed for next-word suggestions. Sentences never span a
`phraseBreaks` character.

### 8b. Optional: emoji words

The bar offers an emoji for a word (🍕 for `pizza`) and the emoji panel has a search. Both
know English from the app, and your language from your pack. The words come from
[Unicode CLDR](https://cldr.unicode.org/)'s emoji annotations, which exist for most locales
(Unicode License v3, already credited by the app).

Look first at what CLDR has for your locale: some have no annotation files (Bhojpuri, `bho`)
and some only a handful of entries (Maithili, `mai`, has eight). Such a pack brings no emoji
words; don't give it a related language's.

1. Add your locale's two files to `data/manifests/cldr-annotations.json`, next to
   Malayalam's: `common/annotations/xx.xml` as `xx.xml` and `common/annotationsDerived/xx.xml`
   as `derived-xx.xml`, with their sha256 (`shasum -a 256`), and your pack to its `packs`:
   pack id to CLDR locale (`"xx": "xx"`; they differ only if your pack id isn't CLDR's code).
   Hindi, Marathi, Bengali, Gujarati, Assamese and Tamil are there already.
2. `make emoji EMOJI_PACKS=xx` writes `data/langs/xx/emoji-words.txt` and `emoji-search.txt`.
   Once they exist, a plain `make emoji` rebuilds them with every other language's. It
   rewrites the app's English files too, with the same bytes unless Unicode's data changed.
3. `scripts/pack.py packs/xx.neopack --write` (step 10) copies the two files into the pack,
   adds the `emojiWords` and `emojiSearch` roles and their checksums. `--check` fails when
   the pack's copies differ from `data/langs/xx/`. They need no engine level: the app, not
   the engine, reads them, and an older app ignores them (it has only Malayalam's, built in).
4. Add a credit to the manifest, as Malayalam's has: id `cldr-annotations`, licence
   `Unicode License v3`, its `licenceUrl`, and the attribution with Unicode's copyright line.

The words are in your script only: CLDR has no romanised ones. A word typed in English
letters gets its emoji once the bar's native word is in the text, which is how Manglish
works too.

Everyday names for flags that CLDR only spells out in full (`അമേരിക്ക` for 🇺🇸) are a short
list per pack id in `data/pipeline/emoji_data.py` (`PACK_FLAG_NAMES`); that one is a code
edit, and optional.

### 9. Train the transliterator

The romanised lexicon only finds words whose spelling folds to the right key. The
transliterator handles the rest: names, rare words, and loose spellings.

```sh
make pack-models PACK_LANG=xx
```

This:

1. writes the training pairs (`python -m mlkb.langs.pairs --lang xx`) to
   `training/build/xx/pairs/`: 3M rows by default, 70% spelled from your tables for the
   lexicon's common words and 30% Aksharantar's mined rows. It also writes Aksharantar's
   manual test rows to `data/eval/xx/translit-human.tsv`, the held-out set, which training
   never sees. `valid-human.tsv` is step 4's calibration pairs (it prints which split they
   came from), and their words are left out of training too;
2. trains (`python -m mlkb.translit.train --lang xx`) into `training/build/xx/translit.pt`.
   Training stops when validation stops improving. It resumes if interrupted: run the same
   command again. It is skipped if `translit.pt` already exists, so delete that file to retrain;
3. exports to int8 (`translit.mlkbnn`). It also records the model's outputs for five reference
   pairs (`training/build/xx/translit-reference.json`, for the engine's forward-pass check).
   The pairs are data: the first five rows of `data/eval/xx/translit-human.tsv`, or, if you
   would rather choose them, `"reference": [["latin", "native"], …]` in
   `data/langs/xx/lang.json` (Assamese does). No code edit is needed;
4. prints held-out accuracy (`python -m mlkb.langs.eval_translit`), the model alone: one line
   for all rows and one per section of the held-out set (`AK-Freq` common words, `AK-NEI`
   Indian names, `AK-NEF` foreign names), each with top-1 and top-3, and the same again with
   the lexicon's words moved first. Tamil's lines:

   ```
   all       4,648 pairs (49.8% of targets in the lexicon)  model alone: top-1 56.0%  top-3 71.2%  lexicon-first: top-1 57.8%  top-3 72.6%
   AK-Freq   2,603 pairs (56.7% of targets in the lexicon)  model alone: top-1 78.9%  top-3 89.5%  lexicon-first: top-1 78.1%  top-3 89.9%
   AK-NEF      829 pairs (38.5% of targets in the lexicon)  model alone: top-1 26.7%  top-3 45.4%  lexicon-first: top-1 29.9%  top-3 49.2%
   AK-NEI    1,216 pairs (42.7% of targets in the lexicon)  model alone: top-1 27.0%  top-3 49.6%  lexicon-first: top-1 33.3%  top-3 51.5%
   ```

   Assamese scored 57.7% / 72.4% over all rows. Names score far below common words in every
   language so far: annotators typed them in their source spelling (`harbhajan`), which the
   script may not be able to carry;
5. copies the model into the pack and updates the checksums;
6. types the same held-out words into the engine, with the lexicons only and with the model
   (`python -m mlkb.eval.pack`; step 11 explains its output).

Training runs up to 100,000 steps and stops early when validation stops improving: about an
hour for Assamese and for Tamil on an M5 Max, longer on a slower Mac or with another job on
the GPU (see Prerequisites). Each evaluation prints its score and either `(best, saved)` or
`(no gain 2/5; best 0.8507)`: evaluations since the best one, how many end the run, and the
best score so far. A score can be above the one before it and still be "no gain", because it
is compared with the best. For a first try, train a short run:

```sh
make pack-models PACK_LANG=xx TRANSLIT_STEPS=6000
```

`TRANSLIT_STEPS` sets the schedule's length; warmup and evaluation scale with it. Marathi's
6,000-step run took 17 minutes and scored 47.9% / 64.5% (model alone, top-1 / top-3) on the
held-out rows. `TRANSLIT_ARGS` passes anything else to `mlkb.translit.train` (see its
`--help`). A finished `translit.pt` is reused, so delete `training/build/xx/translit.pt` before
the full run.

### 10. Checksums and a quick check

```sh
python3 -I scripts/pack.py packs/xx.neopack --write   # fill in sha256 for every listed file
python3 -I scripts/pack.py packs/xx.neopack --check   # fails on a stale checksum, bad script rules or a bad layout
```

Run `--write` after every change to a file in the pack. A stale checksum makes the app refuse
the pack as damaged. `--write` also copies `data/langs/xx/lang.json` in (see the manifest
notes in step 5), and `--check` fails when the pack lacks it or its copy differs.

### 11. Try it on your computer

There is no single command for a new pack yet (gap G10). This script loads a pack in the
engine through its Python bindings and prints what the suggestion bar would show. Save it
anywhere as `try_pack.py`:

```python
import json, sys
from pathlib import Path
import mlengine

root = Path.cwd().parent
pack = root / "packs" / f"{sys.argv[1]}.neopack"
m = json.loads((pack / "manifest.json").read_text())
f = m["files"]
absent = [name for name in f.values() if not (pack / name).is_file()]
if absent:  # the engine would load without them, silently
    sys.exit(f"{pack}: listed in the manifest but missing: {', '.join(absent)}")
opt = lambda role: str(pack / f[role]) if role in f else None
e = mlengine.Engine(english=str(root / "apps/ios/KeyboardExtension/Lexicon/en.fst"),
    malayalam=opt("lexicon"), manglish=opt("romanisedLexicon"), romanisation=opt("romanisation"),
    layout=opt("layout"), next_words=opt("nextWords"), translit=opt("translit"),
    lang=opt("lang"), pack=m["id"], script=m["script"])
for typed in sys.argv[2:]:
    print(typed, "->", [t for t, _, _ in e.suggest(typed, "latin", True)])
```

(The `malayalam` and `manglish` parameters are old names for the pack's lexicon and romanised
lexicon. They work for any language.)

```sh
cd training
uv run python /path/to/try_pack.py as tumi oxom "ami ghorot" kolkatar
# tumi -> ['তুমি', 'tumi', 'time']
# oxom -> ['অসম', 'oxom', 'অসমীয়া']
# ami ghorot -> ['ঘৰত', 'ghorot', 'ঘৰতে']
# kolkatar -> [...]
```

Each argument is what you type; a space inside one gives the context before the last word.
Try your language's common words, and
also a word the lexicon doesn't have, such as a name or a long compound, like `kolkatar`
above. The lexicon can't suggest anything for that one, so any native-script suggestion for
it comes from the transliterator: check that one appears, in your script, and that it is a
plausible spelling.

The script stops if the manifest lists a file the folder doesn't have, because the engine
would load the pack without it and say nothing.

**Numbers.** Measure the pack as the keyboard uses it. This types every Latin word of your
held-out set (`data/eval/xx/translit-human.tsv`) into the engine, first with the lexicons only
and then with the transliterator, and takes about 20 seconds:

```sh
cd training
uv run python -m mlkb.eval.pack --lang xx
```

Tamil's output:

```
ta.neopack 2026.11.10: 4,648 held-out pairs of translit-human.tsv, typed into the engine
lexicons only: sources ['english', 'ta:lexicon', 'ta:romanised', 'ta:next_words']
lexicons only            all       4,648 pairs  top-1 33.1%  in bar 34.5%  first slot 8.7%  none 55.2%
lexicons only            AK-Freq   2,603 pairs  top-1 50.8%  in bar 52.5%  first slot 13.7%  none 40.8%
lexicons only            AK-NEF      829 pairs  top-1 1.6%  in bar 1.6%  first slot 0.2%  none 92.9%
lexicons only            AK-NEI    1,216 pairs  top-1 16.6%  in bar 18.5%  first slot 3.7%  none 60.6%
with the transliterator: sources ['english', 'ta:lexicon', 'ta:romanised', 'ta:next_words', 'ta:translit']
with the transliterator  all       4,648 pairs  top-1 47.8%  in bar 54.4%  first slot 24.8%  none 19.0%
with the transliterator  AK-Freq   2,603 pairs  top-1 70.4%  in bar 80.2%  first slot 39.7%  none 1.4%
with the transliterator  AK-NEF      829 pairs  top-1 10.5%  in bar 11.1%  first slot 0.5%  none 64.0%
with the transliterator  AK-NEI    1,216 pairs  top-1 24.7%  in bar 28.6%  first slot 9.5%  none 25.8%
```

- `top-1`: the first native-script suggestion is the word. The other slots of the bar hold
  the typed text and English words, so this is the pack's best guess whatever English does.
- `in bar`: the word is one of the three suggestions.
- `first slot`: the word is the first suggestion, the one space commits.
- `none`: the bar offered nothing in your script.

Check that the `sources` line of the second run names `xx:translit`: a model that doesn't fit
the pack is left out silently, and the two runs then print the same numbers. The transliterator
should raise every row. For scale, Assamese scores 34.1% / 36.3% (top-1 / in bar) with the
lexicons only and 54.5% / 60.4% with the transliterator. `--pack` and `--set` run a pack folder
or a set elsewhere, `--json OUT` writes the numbers, and it works for any pack (`--lang hi`).

The transliterator alone, without the engine's lexicon search and ranking, is step 9's eval:

```sh
uv run --group train python -m mlkb.langs.eval_translit --lang xx --model ../packs/xx.neopack/translit.mlkbnn
```

The two measure different things, and the engine's top-1 can be the lower one (Tamil: 47.8%
in the engine, 56.0% for the model alone). The engine offers a lexicon word before a spelling
the lexicon doesn't have, shares the bar with English, and doesn't ask the transliterator
about a word that is English: the more of your held-out words your lexicon holds, the smaller
the gap (Hindi, 96.7% of them: 58.0% in the engine, 55.5% alone). `--stages` says where each
word is lost, with the model alone beside the engine (it needs torch, so
`uv run --group train python -m mlkb.eval.pack --lang xx --stages`). Report both.

The full evaluation (`python -m mlkb.eval`: transliteration, next words, English no-harm, with
baselines and gates) has entries for Malayalam and Hindi only (gap G11).

**Record your pack's suggestion bars.** An accuracy score says how many words changed, not
which. Once the pack types the way you want, record the exact bar it gives for every prefix of
a few hundred of your held-out words (and of the first sentences of any
`data/eval/xx/next-*` file you have):

```sh
make golden-save-packs GOLDEN_PACKS=xx    # writes data/golden/packs/xx.tsv.gz; commit it
make golden-check-packs GOLDEN_PACKS=xx   # prints "golden xx: identical", or the bars that moved
```

Run the first again whenever you change the pack's files on purpose (new lexicons, a new
model): the recording is of the pack as it is, so any such change moves it. Without
`GOLDEN_PACKS`, both cover every recorded pack. `engine/core/tests/golden_pack.rs` says what
is typed.

Swipe accuracy on the same held-out words, each drawn as a noisy path over QWERTY, is
`make eval-swipe SWIPE_PACKS=xx`. It prints English and Malayalam first, then your pack over
all rows and over the words your romanised lexicon has; a pack with no entry in
`data/eval/baselines-swipe.json` is printed and not gated.

### 12. Optional: a neural language model

Malayalam's pack has a bilingual language model trained on Malayalam, its romanised form and
English. It ranks suggestions by context, and the next-word bar uses it. Hindi, Assamese and
Tamil have one too. The pipeline (`make lm-data lm LM_LANG=xx`) has a per-language config in
`training/mlkb/lm/lang.py`, which lists `ml`, `hi`, `as` and `ta`; add yours there, copying
Assamese's (`pack_lang=True` spells the romanised training text with your fitted tables from
step 4). A pack is not worse without one: single words typed without a sentence come out the
same either way. If you do build one:

- it trains only on what your manifest credits. Assamese and Tamil use their own text from
  step 3, Aksharantar and, of English, English Wikipedia alone (`english=("en-wiki",)`), so
  they needed no new credit and no Dakshina. Hindi's and Malayalam's also read Simple English
  Wikipedia, Wikinews and Project Gutenberg, which their manifests credit;
- measure the pack without it first: add the pack to `LANGS` in
  `training/mlkb/eval/__main__.py`, write the romanised next-word set
  (`python -m mlkb.eval.next_sets --lang xx`: Dakshina's dev sentences where it has your
  language, eval only, before `make lm-data`; else a synthetic set, after it), then
  `python -m mlkb.eval --lang xx --translit ../packs/xx.neopack/translit.mlkbnn --record-nolm`;
- it needs about 1B tokens of training (120,000 steps: 3.85 hours for Assamese and 4.62 for
  Tamil on an M5 Max), plus data preparation (43 minutes for Assamese's 21M words, 1 hour 37
  minutes for Tamil's 127M);
- `make lm` refuses to copy a model that fails the memorisation check (no run of 50 tokens
  copied from the training text) or makes the evaluation worse than the pack without an LM.
  It copies the file but does not list it: add `"lm": "lm.mlkbnn"` to the manifest's `files`,
  raise `version`, and run `pack.py --write`;
- the pack needs `minEngine` 3 at least (`make pack-needs` says what it needs);
- `make bench-pack-lm LM_LANG=xx` is the latency gate (suggest p99 under 8 ms).

### 13. Put it on your iPhone

Zip the pack:

```sh
python3 -I scripts/pack.py packs/xx.neopack --zip build/packs/xx.neopack
```

Get `xx.neopack` onto the phone with AirDrop, iCloud Drive or anything else that saves to
Files. Then, in NeoOne Keyboard: **Languages → Import from Files**, and pick the file. A newly
imported language is switched on if fewer than four are on.

iOS only reloads the keyboard when it restarts the keyboard's process. If the new language
doesn't appear, switch to another keyboard and back, or close the app you were typing in.

If the import fails, the app shows one sentence:

| Message | Usual cause |
|---|---|
| "This file is damaged or isn't a language pack." | Not a zip, or a zip64, encrypted, or larger than 1 GiB unpacked. |
| "This file isn't a NeoOne Keyboard language pack." | No `manifest.json` in the zip. |
| "This pack is damaged. Download it again." | Invalid manifest JSON, a checksum that differs (run `pack.py --write`), or a checksum for a file that isn't in the zip. |
| "This pack needs a newer NeoOne Keyboard." | `format` isn't 1, or `minEngine` is above the app's engine (7 today); on an app before engine 4, a `script` other than `mlym`, `deva` and `beng`. |
| "This pack wasn't made correctly, so it can't be added." | A bad `id`, a file name with a path in it, no lexicon file, a file in the zip with no checksum, a script other than `mlym`, `deva` and `beng` without `lang.json`, or a `lang.json` the engine refuses (it names another script, has no characters, or overlaps another script's). |
| "You already have a newer version of this language." | The installed pack has a higher `version`. Raise yours. |

The installer doesn't check `name`, `nativeName` or `layout.json`. A manifest without
`nativeName`, or a broken layout, installs and then doesn't load in the keyboard, with no
message (gap G13). Copy the Assamese manifest's fields and you won't hit this.

To test with a build of your own instead, `make twopacks` builds a Debug app with extra packs
copied in. Its device ID is in the `Makefile` (`DEVICE`), and `scripts/debug-packs.sh`
explains the `ML_DEBUG_PACKS` setting.

### 14. Android

There is no Android app yet. The engine is plain Rust and nothing in it depends on iOS, so
the planned Android app will read the same `.neopack` files. A pack you build today is the
pack you will import there.

## Contribute your pack

Open a pull request with:

- `data/manifests/*.json` for every new source;
- `data/langs/xx/` (`lang.json`, `romanisation.hand.json`, `romanisation-weights.json`,
  `romanisation.json`, `lexicon-stats.txt`);
- `data/build-xx-text.sh`;
- `data/eval/xx/translit-human.tsv`;
- `data/golden/packs/xx.tsv.gz`, the pack's recorded suggestion bars (step 11);
- `packs/xx.neopack/` with every file and up-to-date checksums;
- in the description: the calibrate report (where its calibration pairs came from, and the
  held-out test numbers you didn't tune on), the output of `python -m mlkb.eval.pack --lang xx`
  (step 11), the transliterator's held-out accuracy and how many steps it trained, a few typed
  examples and their suggestions (one out of the lexicon), the words per source from step 3,
  and who checked the layout and the spellings (ideally a native speaker).

Leave out `training/build/` and `data/raw/`. They are large and anyone can rebuild them.

Before you open it, run every check of your pack:

```sh
make test-pack PACK_LANG=xx
```

It checks the pack's checksums, script rules and layout (`pack.py --check`), that the engine
opens the pack and loads every file the manifest lists, that every lexicon word is in your
script's normal form, and that every romanised key is the one your tables give
(`engine/core/tests/pack_check.rs`); then the checks that cover all packs; then, when you
have recorded them, that the pack's suggestion bars are the recorded ones. It ends with
`test-pack: packs/xx.neopack passes`. Also run `make test-engine` and `make golden-check`,
which must print `golden: identical` and `golden ml-only: identical`: a new pack changes
nothing for the others.

Two checks fail for a pack that isn't published yet, and both are for a maintainer to clear
with `make site-packs`, which publishes the language: the last step of `make test-data`
(`publish-packs.py --check`: "out of date: packs/index.json"), which runs after every other
check there, and `PackIndexTests` in `make test-swift`. Leave them.

A pack that should be in the repository but not on the site yet (a trial pack for a few
people to try) is cleared the other way: add its id to `ids` in
`site-src/unlisted-packs.json`. An unlisted pack is checked like any pack (`pack.py --check`,
`pack_check.rs`, its checksums in `publish-packs.py`), but `make site-packs` leaves it out of
`site/packs/index.json`, the credits page and the uploads, so the app's Download section never
offers it, and both checks above pass. Hand it out as a file
(`python3 -I scripts/pack.py packs/xx.neopack --zip xx-<version>.neopack`), which Import from
Files takes. To publish it, take the id off the list and run `make site-packs`. The script
refuses an id that `packs/` has no pack for.

## Host your own packs

You don't need a server to share a pack. A `.neopack` file anyone can download is enough,
because Import from Files takes it directly.

The app's Download section reads one pack index, `index.json`, whose URL is set when the app
is built (`apps/ios/PackIndex.xcconfig`, generated from `site-src/site.json`). There is no
setting in the app for another index (gap G14). So hosting your own index means building your
own app with your URL. The index is plain JSON and any https host works:

```json
{
  "format": 1,
  "packs": [
    {
      "id": "xx", "name": "Xx", "nativeName": "…", "version": "2026.11.10",
      "script": "deva", "minEngine": 2,
      "size": 12345678, "sha256": "<sha256 of the .neopack file>",
      "url": "xx-2026.11.10.neopack"
    }
  ]
}
```

`url` may be relative to the index. `size` and `sha256` are of the zipped `.neopack`. Only
https URLs are fetched. `scripts/publish-packs.py` (`make site-packs`) builds the zips, this
index and the credits page from `packs/`. Its upload step (`make site-deploy`) is written for
the project's Cloudflare Worker and R2 bucket. On another host, upload
`build/site-packs/*.neopack` and `site/packs/index.json` yourself.

An index may list the same `id` more than once. Each app offers, per language, the highest
`version` its engine reads, so one language shows as one row. Use this when a new version
raises `minEngine`: apps that are still on the older engine can't read the new version, and
without an older entry they would stop offering the language. Hindi 2026.11.15 needs engine
7 because of its `optionalFinals` and the skeleton's also-rules. The index still lists
2026.11.12, which engine 6 reads right, for the apps already released, and engine 7 apps
offer 2026.11.15 as an update over it. Pick the older entry by what it needs, not by what it
declared: `make pack-needs PACK_DIR=<the unzipped older pack>`.

`make site-packs` takes these older entries from `site-src/kept-packs.json`, not from `packs/`.
List the old version there with the `size`, `sha256` and `url` the index had when it published
that version (they are in git history of `site/packs/index.json`). Keep its file on the host.
The script refuses an entry that isn't older, and for a lower `minEngine`, than the pack in
`packs/`. It also refuses one that no app would pick any more because another entry is at
least as new for the same engines. Delete such entries. Delete the rest once no app in use
has the older engine.

Packs built from share-alike sources are themselves CC BY-SA 4.0. Publish them without DRM
and keep their credits.
