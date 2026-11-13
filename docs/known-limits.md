# Known limits for pack authors

What doesn't work yet when you [make a language](make-a-language.md), and what to do
meanwhile. Each entry is still open. When one is fixed, it is removed from this page.

## Scripts and spelling tables

### Scripts the spelling format does not cover

The [romanisation format](design/romanisation-format.md) reads a word as consonants, each with
an inherent vowel unless a vowel sign follows, plus clusters. Brahmic scripts fit it, and so
does a script that writes no short vowels (Urdu, with `"virama": ""`). It can't describe a
script whose letters change sound by context beyond that, such as Ethiopic syllables or Thai
vowels written before their consonant. Typed input is the letters a to z only.

### No position context for a letter

A letter has one list of spellings wherever it stands. Languages where a letter sounds
different at the start of a word, after a nasal, between vowels or at the end (Tamil's stops,
an anusvara typed `n` inside a word and `m` at its end) can't say so. Workaround: list every
spelling on the letter and fold them together in the skeleton, and add the commonest
combinations as `clusters` or `skeleton.replace` entries (Tamil, Telugu and Sinhala do this).
The cost is a looser skeleton.

### Doubling signs and doubled letters

There is no table for a sign that doubles the next consonant (Gurmukhi's addak ੱ). Punjabi
lists each addak + consonant as a two-code-point `consonants` key instead
(`data/langs/pa/romanisation.hand.json`). A doubled consonant is spelled by doubling its first
Latin letter only (`th` becomes `tth`); if people type `thth`, list it as a cluster, as
Sinhala does.

### A consonant key that starts with a final sign

`finals` are matched before `consonants`, so a consonant key whose first code point is also a
final is never used. Tamil's ஃப (`f`) is read as ஃ then ப (`p`). Typed words still match only
if the skeleton folds the two spellings together (Tamil folds f to p). The training pairs never
use the key's own spelling.

### Doubled letters next to a dropped vowel

The skeleton collapses doubled letters before it drops vowels between consonants, so a typed
vowel between two equal consonants can leave the typed and stored keys different (Punjabi
ਖੇਡਦੀ typed `khedadi`). No workaround in the tables; add the word to `chat-forms.tsv` if it
matters.

### Skeleton folds that overlap

`skeleton.replace` takes the longest match at each position from left to right, so one fold
can hide another that starts a letter later (`aa` taken before `ae` in `tapaae`). Try each fold
and compare the calibration report before and after.

### Words of a single consonant

When the walk drops the final inherent vowel, a one-consonant word has a one-letter key, and
typing it with its vowel (`ra`, `ma`) doesn't reach it. Nepali's commonest words are like this.
Workaround: list them in `chat-forms.tsv`.

### Replacements that depend on the letter before

A `lang.json` `replace` pair can't say "only after a letter". A mis-encoding that is wrong only
in some places (Kannada text using the digit zero ೦ for the anusvara ಂ) needs one pair for
every letter that may come before it.

### Joiners

There is no tool that counts how often words are written with and without ZWJ or ZWNJ, which
is what decides whether a pack should keep them; you need your own count. If the text writes
the same word both ways (Sinhala), the lexicon stores both and frequency picks the one shown.

### What the exported table leaves out

The pack's `romanisation.json` is exported from your hand file and fitted weights. The export
always writes `atWordEnd.virama` as `""` and writes no `swipe` or `fromLatin` section, so a
`fromLatin` you add to the pack by hand is lost the next time `make pack-lexicons` runs. A
change to `skeleton`, `clusters` or `looseSpelling` in the hand file only reaches the pack after
`calibrate --write`.

### The rule-based guess for unknown words is rough

When neither the lexicon nor the transliterator answers, the engine spells the typed letters
from your tables turned round. Where two letters share a spelling it takes the one with the
lower code point, every vowel comes out short, and nothing is doubled. A hand-written
`fromLatin` section is better (see the export limit above).

### Suffix splitting

Only Malayalam ships a suffix table. The four `join` kinds are the only ways a suffix can join
its stem, so a language whose endings change the stem some other way (Tamil's doubling in
வீட்டில்) can't describe it.

## Building a pack

### Text is fetched by a script per language

Each language has its own `data/build-<id>-text.sh` that names its dumps, its Wikisource Page
namespace and its extra steps. Copy the closest one. Thinning a source that is most of the text,
or dropping template-written articles (`data/pipeline/drop_repeats.py`), is an edit to that
script. Newer Wikisources number the Page namespace 250, not 104; with the wrong number the
extractor reads only the main namespace without a warning.

### Two ways to build a lexicon

Hindi is built from `data/lang/hi.conf` with `lexbuild --lang`; every other pack from
`data/langs/<id>/lang.json` with `lexbuild-native`. Copy a `data/langs` language, not Hindi:
Hindi's `lang.json` lacks the builder's keys and Hindi has no hand romanisation file.

### English next-word pairs come from the Malayalam pack

The lexicon build reads English's next-word pairs from `packs/ml.neopack/next.fst` unless
`ENGLISH_NEXT` names another file, so building any pack needs the Malayalam pack in the
checkout.

### The manifest is written by hand

There is no single command that builds a pack. You write `manifest.json` yourself, copy its
`credits` from `data/manifests`, and write `englishFingerprint` by hand when the pack has next
words or an LM. `make pack` zips only the Malayalam pack; use `scripts/pack.py --zip` for
yours.

### Chat forms are added to the pack by hand

`pack.py --write` copies emoji files from `data/langs/<id>/` but not `chat-forms.tsv`. Put the
file in the pack folder yourself, add `"chatForms"` to the manifest, and add its own source
manifest in `data/manifests`.

### Emoji words

`make emoji EMOJI_PACKS=xx` also rewrites the app's own emoji files (normally to the same
bytes). Flag names per language and the list of grammar words that get no emoji are tables in
`data/pipeline/emoji_data.py`, and only English has the grammar-word list.

### Transliterator training

A finished `translit.pt` is reused whatever `TRANSLIT_STEPS` says, so after a short trial run
delete it by hand before a full one. `make pack-models` writes the pairs and trains in one step,
so it can't wait for another job that is using the GPU.

### Aksharantar's licence filter isn't reported

The pair reader drops Aksharantar's Dakshina and "existing sources" rows, but nothing prints
how many for your language. Count them yourself for the manifest's `verdict_reason`; the guide
gives the command.

## Measuring a pack

### No command to try a pack

Trying a pack is the guide's `try_pack.py` snippet (step 11), not a tool in the repository.

### Full evaluation for a few languages only

`python -m mlkb.eval` (next words, English no-harm, baselines and gates) knows Malayalam, Hindi,
Assamese and Tamil only. For any other pack, `python -m mlkb.eval.pack --lang xx` measures
typed words through the engine. There is no option to swap in another pack's transliterator
for comparison (copy the model in and run `pack.py --write`), and a second held-out set has to
be made by hand.

### Language models for a few languages only

The LM pipeline (`training/mlkb/lm/lang.py`) lists its languages in code: Malayalam, Hindi,
Assamese and Tamil. A new pack ships without an LM unless that file gains an entry.

### Calibration numbers flatter the tables

The calibration pairs are a slice of the training data, and the tables are tuned by looking at
them, so the held-out numbers are often much lower. With few manual pairs (under about 500)
training's early stopping is partly noise. A language with manual rows only in Aksharantar's
test split calibrates on mined rows, which follow a mining model's spellings rather than
people's. Your own typed pairs (`data/langs/xx/pairs.json`) are the better source when you
have them.

### The engine can trail the transliterator alone

On held-out words the engine's first suggestion is sometimes worse than the transliterator's
own best guess: a lexicon word that fits loosely, a commoner lexicon word, or English can take
the slot. `python -m mlkb.eval.pack --stages` shows where each word is lost.

## Installing and hosting

### A broken layout installs without a word

The app opens a pack with the engine before installing it, so a bad manifest or `lang.json` is
refused. A `layout.json` the app can't read is not: the pack installs and the language has no
native-script keys. Run `pack.py --check`, which catches it.

### One pack index per app build

The app's Download section reads the one index URL it was built with. There is no setting for
another index, so packs hosted elsewhere reach users through Import from Files.

### Publishing tools assume Cloudflare

`make site-deploy` uploads with wrangler to R2 and a Worker. To host on another static host,
build the `.neopack` files and `index.json` yourself (see the guide's "Host your own packs").

### Credits with an unknown licence

The credits page links only the licence names it knows (`LICENCE_LINKS` in
`scripts/publish-packs.py`). Any other licence needs `licenceUrl` in the
manifest's credit, and nothing warns when it is missing.

### Currency signs longer than one character

`currency` in `layout.json` may be a short string (Sinhala's රු), but `pack.py --check` doesn't
read it and a two-character key hasn't been checked on screen.
