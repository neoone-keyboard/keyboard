# Make your own language

NeoOne Keyboard lets people type a language the way it sounds in English letters and get
its own script back: `namaste` gives नमस्ते, `oxom` gives অসম, `ente` gives എന്റെ. English is
built into the app. Every other language is a **language pack** that anyone can build from
openly licensed text, then import on their phone or contribute back.

This guide walks through building a pack with the tools in this repository as they work
today. It uses the Assamese pack (`packs/as.neopack`) as the worked example, because it is
the most recent and was built almost entirely from data. Hindi and Malayalam appear where
they differ.

Some steps still need a code edit or only work for certain kinds of language. The guide says
so where it happens, and [make-a-language-gaps.md](make-a-language-gaps.md) lists each one
with a proposed fix.

## Before you start: can your language be a pack today?

Check three things first.

1. **The script has a plug-in in the engine.** How a script behaves (its normal form, where a
   vowel sign attaches, what ends a sentence) is Rust code in `engine/core/src/script/`. Today
   there are three: `mlym` (Malayalam), `deva` (Devanagari) and `beng` (Bengali-Assamese). The
   app refuses a pack in any other script with "This pack needs a newer NeoOne Keyboard". A
   new script means writing a plug-in (gaps G1, G2).
2. **The script is Brahmic and fits one 128-code-point Unicode block.** The romanisation
   format (consonants with an inherent vowel, vowel signs, a virama), the transliterator's
   vocabulary and the romanised lexicon's encoding all assume this (gap G3).
3. **There are human romanisations to learn from.** The tools read them from
   [Aksharantar](https://huggingface.co/datasets/ai4bharat/Aksharantar), which covers 21
   languages. A language without Aksharantar data needs code changes (gap G6).

Some examples: Marathi, Nepali or Sanskrit (Devanagari) need only data. Bengali also uses
`beng`, but the plug-in folds the Bengali letter র into the Assamese ৰ, so a Bengali pack
needs a code change first (gap G2). Tamil, Telugu, Gujarati and Kannada each need a new
plug-in.

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
| `lm` | `lm.mlkbnn` | A bilingual neural language model (about 9.5 MB) that ranks suggestions by context. Malayalam has one. | No |
| `layout` | `layout.json` | The native-script keyboard pages. Without it the language is typed in Latin only and the language key skips it. | No |
| `suffixes` | `suffixes.tsv` | Endings that chat glues onto words, for splitting `stem + ending`. Only scripts whose plug-in joins suffixes use it (Malayalam today). | No |
| `chatForms` | `chat-forms.tsv` | Hand-written chat spellings no rule reaches: `latin<TAB>native` per line. | No |

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
before it starts. Training (step 9) needs the lexicon (step 7) and your reference pairs.

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

For Aksharantar, copy `data/manifests/aksharantar-asm.json` and change the language code
(`asm.zip` becomes, say, `mar.zip`). Aksharantar publishes no checksums, so compute the zip's
sha256 yourself:

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

This file tells the lexicon builder what a word is. Copy `data/langs/as/lang.json`:

| Field | Meaning |
|---|---|
| `id`, `name`, `script` | The pack id, English name, and ISO 15924 script code (`deva`, `beng`, `mlym`). |
| `block` | The script's Unicode block, first and last code point in hex. |
| `notWord` | Ranges in the block that are not letters (digits, currency signs). |
| `firstLetter` | Ranges a word may start with (vowels and consonants, not signs). |
| `virama` | The virama's code point. |
| `signs`, `bases` | Vowel signs and the letters they may follow. A word with a sign after anything else is treated as mis-encoded and dropped. |
| `apostrophe` | `true` if an apostrophe can stand inside a word (Assamese হ'ল). |
| `replace` | Text replacements applied before and after Unicode NFC, to fix old or wrong encodings. |
| `phraseBreaks` | Characters that end a sentence (the danda). Next-word pairs never cross one. |
| `maxChars`, `maxWords`, `minCount` | Longest word kept, lexicon size, and the fewest times a word must appear. |
| `aksharantar` | Aksharantar's three-letter code for the language (`asm`, `mar`). |

The engine's script plug-in has its own normal form in Rust. Your `replace` list must produce
the same text the plug-in produces, or typed words won't match stored ones. For `deva` the
plug-in applies NFC and drops every ZWJ and ZWNJ, so a Devanagari `lang.json` must drop them
too, with the first two `replace` entries below. For `beng` it applies Assamese's fixes,
which are wrong for Bengali. There is no test for a new pack yet (gap G9).

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
  plain र्, as the plug-in makes it.
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

### 3. Get the text

Write `data/build-xx-text.sh`. Copy `data/build-as-text.sh` and change the source ids, the
dump file names and the Wikisource page namespace. It must leave plain text, one paragraph
per line, in `data/raw/text/xx.txt` and unzip Aksharantar into
`data/raw/aksharantar-<code>/extracted/`. Make it executable (`chmod +x`).

Wikisource keeps its proofread text in the Page namespace, not the main one, so the script
passes `--extra-namespaces` to the extractor: a comma-separated list of namespace keys or
names, any of which selects it. The Assamese script gives the key, the English name and the
local name, `104,Page,পৃষ্ঠা`, so it works whichever the dump uses. Find yours in the dump's
`<siteinfo>`:

```sh
bzcat data/raw/xxwikisource/*.bz2 | head -60 | grep namespace
```

For Marathi, Page is key 104, `पान`, so the argument is `104,Page,पान`. Then run the script once
by hand:

```sh
./data/build-xx-text.sh
```

The lexicon build in step 7 runs this script again, so it must be safe to re-run. The
Assamese one skips work that is already done. This script is per language today (gap G4).

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

**Calibrate.** Fit the weights to the human pairs:

```sh
cd training
uv run python -m mlkb.langs.calibrate --lang xx            # report only
uv run python -m mlkb.langs.calibrate --lang xx --write    # write the weights
```

The report first says which pairs it calibrates on: Aksharantar's manual rows in `valid`, or,
when `valid` has none, a quarter of `train`'s manual rows held out by word (never a test
word; step 9's training leaves them out too). It stops with an error if there are no manual
rows at all. Then it shows how often the tables give exactly what people typed
(`canonical`), whether they can spell it at all (`producible`), and how often the lookup
would find the word (`skeleton`, `prefix`).

- **Tune on the calibration numbers only.** Change the hand tables and run it again until the
  `calibration (after)` numbers stop improving.
- **The test numbers are held out.** They are for your pull request's description. Changing
  the tables to raise them tunes on the test set, and then they no longer say how well the
  tables do on words nobody tuned for.

Without Aksharantar data, copy the hand file to `romanisation-weights.json` and skip the
fitting (gap G6).

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
  "minEngine": 2,
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
- `minEngine` is 1 for a Malayalam pack, 2 for `deva` or `beng`, and 3 for a pack in a script
  other than Malayalam that has an `lm`.
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
- The app refuses a layout with another `format`, no pages, or an empty key.

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

Read `data/langs/xx/lexicon-stats.txt` afterwards. The top 20 words should be the language's
common short words. If they are not, fix `lang.json`. Marathi's started `आहे आणि व या हे ते
एक हा नाही`.

Every build exports `romanisation.json` again from the tables, so fix its notes in the hand
file (step 4), not in the exported file.

You can try the lexicons now, before training anything: step 11's script works on a pack
without `translit.mlkbnn` if you take `translit` out of the manifest's `files` for the moment
(the script stops on a listed file that is missing). Put it back before step 9.

Hindi is built a different way: `data/lang/hi.conf` plus `lexbuild --lang`, using the engine's
own plug-in instead of `lang.json`, and it needs the full English text. Use the Assamese path
for a new language.

### 8. Next-word data

Step 7 already wrote `next.fst`: pairs of adjacent words from your text, plus the English
pairs. Nothing more is needed for next-word suggestions. Sentences never span a
`phraseBreaks` character.

### 9. Train the transliterator

The romanised lexicon only finds words whose spelling folds to the right key. The
transliterator handles the rest: names, rare words, and loose spellings.

Before training, add your language to `REFERENCE_CASES_BY_LANG` in
`training/mlkb/translit/export.py`: five `(latin, native)` pairs that the export checks the
model against. Without them the export stops with `FileNotFoundError` (gap G7). Then:

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
3. exports to int8 (`translit.mlkbnn`) and checks it against the reference pairs;
4. prints held-out accuracy (`python -m mlkb.langs.eval_translit`), the model alone, top-1 and
   top-3. Assamese scored 57.7% / 72.4% over all test rows and 73.7% / 85.7% on common words;
5. copies the model into the pack and updates the checksums.

Training runs up to 100,000 steps and stops early when validation stops improving: about an
hour for Assamese on an M5 Max, longer on a slower Mac or with another job on the GPU (see
Prerequisites). For a first try, train a short run:

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
python3 -I scripts/pack.py packs/xx.neopack --check   # fails if any checksum is stale
```

Run `--write` after every change to a file in the pack. A stale checksum makes the app refuse
the pack as damaged.

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
    pack=m["id"], script=m["script"])
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

For numbers, rerun the transliterator's held-out accuracy:

```sh
cd training
uv run --group train python -m mlkb.langs.eval_translit --lang xx --model ../packs/xx.neopack/translit.mlkbnn
```

The full evaluation (`python -m mlkb.eval`: transliteration, next words, English no-harm, with
baselines) has entries for Malayalam and Hindi only (gap G11).

### 12. Optional: a neural language model

Malayalam's pack has a bilingual language model trained on Malayalam, its romanised form and
English. It ranks suggestions by context, and the next-word bar uses it. Hindi's is being
built now. The pipeline (`make lm-data lm LM_LANG=hi`) has a per-language config in
`training/mlkb/lm/lang.py`, which lists only `ml` and `hi`, and it needs Dakshina's romanised
sentences (12 languages) besides Aksharantar (gap G12). Plan on a pack without an LM; Assamese
has none. If you do build one:

- it needs about 1B tokens of training, 4 to 7 hours on an M5 Max, plus about 2 hours of data
  preparation;
- `make lm` refuses to copy a model that fails the memorisation check (no run of 50 tokens
  copied from the training text) or makes the evaluation worse than the pack without an LM;
- the pack's `minEngine` becomes 3.

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
| "This pack needs a newer NeoOne Keyboard." | `format` isn't 1, the `script` has no plug-in in this app version, or `minEngine` is above the app's engine (3 today). |
| "This pack wasn't made correctly, so it can't be added." | A bad `id`, a file name with a path in it, no lexicon file, or a file in the zip with no checksum. |
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
- `packs/xx.neopack/` with every file and up-to-date checksums;
- your language's entry in `REFERENCE_CASES_BY_LANG` (`training/mlkb/translit/export.py`);
- in the description: the calibrate report (where its calibration pairs came from, and the
  held-out test numbers you didn't tune on), the transliterator's held-out accuracy and how
  many steps it trained, a few typed examples and their suggestions (one out of the lexicon),
  and who checked the layout and the spellings (ideally a native speaker).

Leave out `training/build/` and `data/raw/`. They are large and anyone can rebuild them.

`make test-data` checks every pack's checksums, then checks that the published site lists
every pack in `packs/`. That second check fails until a maintainer runs `make site-packs`,
which publishes the language. Leave it to them.

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

Packs built from share-alike sources are themselves CC BY-SA 4.0. Publish them without DRM
and keep their credits.
