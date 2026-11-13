# Language packs: design and format

**Goal:** make every language other than the always-present English base a self-contained
pack that the app downloads or imports from Files, and that the keyboard loads without Full
Access. Malayalam (with Manglish) becomes the first pack and, like every other language, is
**not built into the released app**. Only English ships inside it.

## Why

Anyone should be able to add a language by contributing a pack. Before packs, English,
Malayalam and Manglish were wired through every layer: fixed `EngineConfig` slots, two-valued
`Script` / `Layout` / `Language` / `InputLayout` enums, Malayalam rules in the engine, and
Malayalam pages in Swift.

## Facts this design rests on

- **The keyboard can read the App Group without Full Access.** Tested on iOS 26 on a
  phone: the app wrote a file into `group.me.jishnu.keyboard` and the keyboard read
  it with `RequestsOpenAccess: false`. Apple doesn't document this, so a future iOS could
  close it; see Fallback.
- **The bilingual LM is fingerprint-locked to `en.fst` plus `ml.fst`** (`engine.rs` `lm_fits`).
  `next.fst` likewise mixes both vocabularies.
- **People who type a language in Latin letters mix it with English.** Manglish typing
  switches between English and Malayalam words mid-sentence.

## Decisions

1. **English is the only built-in language.** Every other language, Malayalam included, is
   a pack the user downloads or imports. The English base carries `en.fst`, an English-only
   `next.fst` (bigrams from the English sources), the English emoji words and no LM, so
   English alone ranks without an LM. An English-only LM is later work if that ranking proves
   too weak. When a pack is active, its bilingual LM ranks both English and the pack's
   language, as today.
2. **A pack is bilingual with English.** Its LM and next-word model are built against the
   English base and its own lexicon, as today. A pack names the English lexicon fingerprint
   it was built against. If English is updated and the fingerprint no longer matches, the
   pack's LM is switched off (today's `lm_fits` behaviour) until the pack is rebuilt.
3. **Data in packs, script rules in code.** What differs between languages that share a
   script is data. How a script behaves is code, compiled into the engine as a script
   plug-in identified by an ISO 15924 code (`latn`, `mlym`; later `deva`, `taml`, `arab`, …).
   This covers grapheme normalisation, the base for vowel signs, syllable walking for
   romanisation, and suffix joining. A pack declares its script. If this build has no
   plug-in for that script, the pack is refused with "Needs a newer NeoOne Keyboard".
   Anyone can add a language in a supported script with data alone; a new script needs a
   code contribution, which is also open source.
4. **Up to N languages are active, chosen from English and the installed packs.** N is
   one constant, `MAX_ACTIVE`, which is 4 (2 until 2026-11-08). N counts English, so English
   and every pack there is (Malayalam, Hindi, Assamese) can be on together. Nothing in the
   code may assume that English is active or that only one pack is active. Raising N meant
   changing the constant and measuring, not redesigning; the measurements are below. The
   default, before the user chooses, stays English and the first pack.
   - **Measured for N = 4** (2026-11-08). Every installed pack is loaded whether active or
     not; lexicons and models are memory-mapped and clean, so iOS doesn't count them in the
     footprint, and the engine's heap is what grows. Engine heap
     (`engine/core/tests/multi_lang_memory.rs`, release, typing words of each language, 10
     suggestions), and suggest p99 on the Mac (`engine/core/tests/multi_lang.rs`, every
     active romanised pack running its transliterator, Hinglish and Assamese words added to
     the gate's inputs):

     | Active | Packs installed | Heap after open | Heap peak typing | Mapped files | p99 (keyboard cutoffs) |
     |---|---|---|---|---|---|
     | en | ml | 2.0 MB | 2.0 MB | 30.6 MB | — |
     | en ml | ml | 2.0 MB | 4.2 MB | 30.6 MB | 3.4 ms |
     | en ml hi | ml hi | 2.3 MB | 4.6 MB | 46.5 MB | 4.3 ms |
     | en ml hi as | ml hi as | 2.4 MB | 5.2 MB | 61.8 MB | 4.3 ms |
     | ml hi as | ml hi as | 2.4 MB | 5.2 MB | 61.8 MB | 3.2 ms |

     The simulator can't measure the extension's footprint: a Release build with English
     alone sits at 49.6 MB (`testFootprintWhileTyping`), over the 32 MB shed point, so the
     engine is unloaded there whatever is active. The engine's heap is what four languages
     add: 3.2 MB more than English alone at the peak, and the packs' files are mapped, clean
     pages iOS doesn't count. The 8 ms gate holds with four. On the phone, from
     idle, a suggest runs 5–7× slower; the transliterators share the keyboard's 8 ms deadline
     and run likeliest pack first, so a slow request costs the less likely packs' model
     spellings, never the bar. Still to measure on the phone with three packs installed.
   - **Which pack a Latin word is (N > 2).** The priors are as before: the previous word
     (its pack's script), the habit per language, the LM's bias for its own pack. Lexicon
     frequencies don't compare across packs (Assamese's smaller corpus put পানী ahead of
     പണി for `pani`), so when two or more active packs type in Latin, the pack whose prior
     from the previous word and the habit (not the LM's) leads every other's by
     `rank::FAVOURED_LEAD` (0.5) is favoured. Its word keeps the middle unless another
     pack's scores `rank::FAVOURED_MARGIN` (2.0, a previous word's worth) more; the two side
     slots shown with the middle hold English and its words, its best among them; other
     packs' words move just past them, further along the scrolling bar. With no favoured
     pack the bar is by score. After another pack's word the LM's scores are left out (it
     knows nothing of that script), so next words after मेरा are Hindi's. Without English the
     habit weighs the packs against the most used one. With one pack nothing changes.
   - **English is a language like any other in the list, and can be switched off.** Off
     means no English words, completions, corrections or next words in the bar, and no
     English autocorrect. What you type is still kept as typed. English always ships, and
     its lexicon stays loaded as plumbing when an active pack's LM needs it (decision 2).
     It is never offered as a suggestion.
   - At least one language is always active.
   - **Layouts follow the active list.** There is one Latin layout if any active language is
     typed in Latin (English, or a pack's romanised form such as Manglish), plus one native
     layout per active pack. The language key cycles through them in list order. With
     English + Malayalam, that is today's മ / EN toggle.
   - **Candidates carry their language.** On the Latin layout, every active language
     offers candidates for the same input. Ranking adds a prior for each language: from the
     previous word's language, from the user's habit counts per language, and from each
     pack's LM. With two languages this is exactly today's Manglish bias, written as a
     per-language vector instead of one number.
5. **Packs are read-only to the keyboard.** The app installs, updates and removes packs.
   The keyboard only reads them.

## Pack format

A pack is a folder named `<id>.neopack/`. To import or download it, the folder is zipped
into a `.neopack` file. The folder holds `manifest.json` and data files.

```json
{
  "format": 1,
  "id": "ml",
  "version": "2026.11.06",
  "name": "Malayalam",
  "nativeName": "മലയാളം",
  "script": "mlym",
  "romanised": { "name": "Manglish" },
  "minEngine": 6,
  "englishFingerprint": "<hex of the en.fst this pack's LM/next were built against>",
  "files": {
    "lexicon": "lexicon.fst",
    "romanisedLexicon": "romanised.fst",
    "nextWords": "next.fst",
    "translit": "translit.mlkbnn",
    "lm": "lm.mlkbnn",
    "suffixes": "suffixes.tsv",
    "chatForms": "chat-forms.tsv",
    "romanisation": "romanisation.json",
    "emojiWords": "emoji-words.txt",
    "emojiSearch": "emoji-search.txt",
    "layout": "layout.json"
  },
  "sha256": { "lexicon.fst": "…", "…": "…" },
  "credits": [
    { "name": "Malayalam Wikipedia", "licence": "CC BY-SA 4.0", "url": "https://ml.wikipedia.org" }
  ]
}
```

- `credits` list the pack's data sources for the About screen: `name`, `licence` and `url`
  (the source's site), and, as the About screen shows them, `id` (the data/manifests id),
  `licenceUrl` and `attribution`. The keyboard lists English's own credits, then each
  pack's in order, each `id` once.
- Every file except `lexicon` is optional. A missing file switches off only that feature,
  the same way a missing bundle file does today.
- `romanisation.json` holds the tables now in `romanize.rs`: independent vowels, vowel
  signs, consonants, finals, clusters, the skeleton folds, and the swipe skeleton tables
  (`ASPIRATED`, `TRAIL`).
- `suffixes.tsv` holds the clitics and case endings that were `suffix.rs` `SUFFIXES`. One
  suffix per line, tab-separated: `latin` (the typed spellings, comma-separated, lower-case
  a–z), `native` (the suffix in the pack's script), `join` (`vowel`, `case`, `consonant` or
  `oblique`: how it fuses with its stem, carried out by the script plug-in's
  `join_suffix`), and an optional fourth field `inner` when another suffix may follow it.
  `#` lines and blank lines are skipped; any other malformed line fails the load (the
  pack then has no suffix splitting). Row order breaks ties between cuts of equal length,
  so it is kept. A pack without the file, or in a script with no joins, is never split.
- `chat-forms.tsv` holds irregular chat spellings (was `data/chat-forms.tsv`, compiled
  in): `latin<TAB>native` per line, `#` lines and blank lines skipped. A typed word equal
  to a row's `latin` (ignoring ASCII case) offers the row's word.
- `layout.json` holds the native pages (letters, more letters, vowels), long-press
  alternates such as chillus, the space-bar label, the language-key label, and the
  vowel-sign strip order. Format 1:

  ```json
  {
    "format": 1,
    "spaceBar": "മലയാളം",
    "languageKey": "മ",
    "pages": [
      { "id": "letters", "label": "കഖ", "rows": [["ക", "ഖ", "…"], ["…"]] },
      { "id": "more", "label": "ശഷ", "rows": [["…"]] },
      { "id": "vowels", "label": "അആ", "rows": [["…"]] }
    ],
    "longPress": { "ന": ["ൻ"], "…": ["…"] },
    "vowelSignStrip": ["ാ", "ി", "…", "്"]
  }
  ```

  `spaceBar` names the space bar while the layout is up; `languageKey` labels the
  language key on another layout, to switch to this one. `pages` come in the order the
  layout opens them, the first being its alphabet; each `label` is the label of the key
  that shows the page, and `rows` list, row by row, the text each key types. The
  keyboard adds its own keys (backspace at the end of the last row, the bottom row with
  the page keys, space and return), so a page lists only what it types. `longPress` maps
  a key's text to its long-press options. `vowelSignStrip` is what the strip offers after
  a letter a sign can attach to (`ScriptRules::mark_base`), each entry one character, in
  the order shown when the engine has no preference; the engine orders it by the lexicon,
  and without it there is no strip. Unknown keys are ignored, so later versions can add
  fields. Every pack has the file; Hindi's and Assamese's share Malayalam's structure.
  - Page order is `letters`, `more`, `vowels` in every language, so the bottom row is the
    same everywhere: the vowels page's key left of space, the more page's right of it. Each
    page's label is its first two letters. This is a changeable default; the file is data.
  - Hindi: space bar हिन्दी, language key हि, pages कख / शष / अआ. Assamese: অসমীয়া, অ,
    pages কখ / শষ / অআ. Nukta letters are base + nukta (ज़ = ज + U+093C), so a key may
    type more than one code point. The strips are the orders under "Script plug-ins".
- Per-pack ranking constants (`MANGLISH_*`, `LM_WEIGHT_ML`, `CONTEXT_AFTER_MALAYALAM`) move
  into an optional `"tuning"` object in the manifest. Defaults are today's values.

## Where packs live

| Location | Who writes it | Used for |
|---|---|---|
| Extension bundle `Packs/` | the build | Debug builds only (Malayalam, plus `ML_DEBUG_PACKS`); none in Release builds |
| App Group `Packs/` | the app | every pack a user has: downloaded or imported |

The keyboard scans both. An installed pack replaces a built-in one of the same `id` only
with a higher `version`; an equal or older one doesn't. (`PackDescriptor.merge`. Versions compare part by part at the
dots, numerically where both parts are numbers, a missing part counting as 0. Bundled ids
keep their place first and installed-only ids follow by id, so installing a pack never
changes the engine's default active list of English and the first pack.) The container is
`group.me.jishnu.keyboard` on both targets (`KeyboardCore.PackLocations`).

**Installing.** The app unzips into `Packs/.incoming-<uuid>/`, checks the `sha256` of every
file, checks `format`, `minEngine` and `script`, then renames the folder into place. If an
older version is installed, it is replaced in one rename. Download and Files import share
this code. Downloads come from a pack index (`index.json`) published on the download site (see "Hosting").

The installer is `KeyboardCore.PackInstaller`:
- The zip may hold one folder (`hi.neopack/…`, as `scripts/pack.py --zip` and Finder write
  it) or the files alone; Finder's `__MACOSX/`, `.DS_Store` and `._*` are ignored. Stored and
  deflated entries only; no zip64, encryption or names that could leave the folder; CRC-32
  and sizes are checked; at most 1 GiB unpacked.
- Refused: `format` other than the engine's, a `script` it has no rules for, `minEngine`
  above `PACK_ENGINE` (the app asks the engine, FFI `pack_support()`), an `id` not 1–16 of
  `a-z 0-9 -` starting with a letter or `en`, a name in `files` that isn't a plain file
  name, no lexicon, a file in the zip that the manifest names without a checksum, a
  checksum for a file not in the zip, a digest that differs, and a version older than the
  installed one. The same version installs again, which repairs a damaged install. Files
  the manifest doesn't name are not installed.
- The replace is `renamex_np(RENAME_SWAP)`; the old folder is deleted after. Leftover
  `.incoming-*` folders (an app killed mid-install) are removed before each install; a dot
  folder is never scanned, so the keyboard never sees a half-made pack.
- Files import: `PackImport.install(from:with:)` copies the picked URL under file
  coordination and installs the copy. The app declares the `.neopack` type
  (`me.jishnu.keyboard.neopack`, a `public.zip-archive`).

**Fallback.** If the active pack can't be opened (missing, refused, or the App Group is
unreadable on a future iOS), the keyboard uses English alone. It shows "Malayalam isn't
available — open NeoOne to fix" once per session in the bar.
(The engine leaves out, each with a `pack_problems` line, a pack whose manifest is
refused, whose lexicon doesn't open, whose id is already loaded, or whose id is `en`; English
and the other packs carry on, and the keyboard's layouts follow what loaded. The bar message
waits for its design.)

**First launch.** The app asks which languages you type and downloads them, or lets you
import a `.neopack` from Files. Until a pack is installed the keyboard is English only.
(A sheet shown once, with the available languages as toggles under the same rules
as the Languages screen, and below them the Languages screen's Download section, so a Release
build, which bundles no pack, starts with English and offers the rest. Import from Files is
in Languages; the footer says so.)

**Choices and the Languages screen** (these are defaults and can be changed):
- `MAX_ACTIVE` is 4 and counts English (2 until 2026-11-08; decision 4 has the
  measurements). The app mirrors it as `ActiveLanguages.maxActive`, pinned by a test, since
  the FFI doesn't expose it. At least one language stays on; when four are on, the other
  toggles are disabled.
- The active list is in the Languages list's order: English first, then bundled packs, then
  installed ones by id. The language key cycles in that order. No reordering yet.
- The choice is `active-languages.json` in the App Group root, written by the app only. The
  keyboard reads it once per process, drops ids it has no pack for, and falls back to the
  engine's default (English and the first pack) if it is missing or empty. It wins over the
  debug-only `ML_ACTIVE_LANGUAGES`. Nothing is written until the user chooses.
- Choices and newly installed packs reach the keyboard only when iOS restarts its process.
  Re-reading on each appearance would be immediate but means reopening the engine; that is a
  separate change.
- Removing works on App Group packs only; a bundled pack shows "Built in". A removed pack
  leaves the choice. A newly imported or downloaded language is switched on if fewer than
  `MAX_ACTIVE` are on; an update never changes the choice.
- The index (`index.json`, format 1) lists `id`, `name`, `version`, `script`, `size`,
  `sha256` of the `.neopack`, `url` (may be relative), and optionally `nativeName` and
  `englishFingerprint`. `PackDownloader` checks size and digest, then runs the installer;
  only `https` is fetched (`file` for tests). The index is fetched when the Languages screen
  opens, a pack only on Get or Update. Refusals are shown as one plain sentence
  (`PackMessages`).
- The index location is the Info.plist key `NeoPackIndexURL`, from the build setting
  `ML_PACK_INDEX_URL`, set in `apps/ios/PackIndex.xcconfig` for Debug and Release (an empty
  value hides the download section). A fetch that fails shows one sentence in the section.

**Hosting:**
- https://neoone.jishnu.me is a Cloudflare Worker, `neoone-site` (`hosting/wrangler.jsonc`,
  `hosting/worker.js`), on a custom domain of the `jishnu.me` zone in the same Cloudflare account.
  Its static assets are `site/`: `index.html` (a short product page), `privacy.html`,
  `support.html` (GitHub issues, no email), `credits.html`, `style.css`, `_headers` and
  `packs/index.json` (cached 5 minutes). The domain, the download URL, the R2 bucket and the
  repository are set in one place, `site-src/site.json`.
- `.neopack` files are larger than a static asset may be (25 MiB), so they are objects in the R2
  bucket `neoone-packs`, key `packs/<id>-<version>.neopack`; the Worker serves them at
  `https://neoone.jishnu.me/packs/<id>-<version>.neopack` (GET and HEAD, ranges, `ETag` and
  conditional requests, `application/zip`, cached a year as immutable: a name carries its
  version). Only `/packs/*.neopack` and `/packs/*.zip` reach the Worker's code; the rest is
  served as assets. `PackDownloader` still follows redirects (`aRedirectedDownloadIsFollowed`),
  so moving the files behind a redirect later needs no app change.
- `make site-packs` (`scripts/publish-packs.py`) zips each pack reproducibly (fixed dates and
  modes, so the digest depends only on the files) into `build/site-packs/`, writes `site/` from
  `site-src/` and the manifests (credits from each pack manifest and English's sources in
  `data/manifests`), the index with each zip's `size` and `sha256`, and `PackIndex.xcconfig`,
  and prints the `wrangler r2 object put` and `wrangler deploy` commands. `make site-deploy`
  runs them: it uploads each file the site doesn't serve yet (a HEAD on its URL), refuses one
  served with another size, and deploys. The script refuses a pack whose version is already in
  the index with other bytes: a published file isn't replaced, the version is raised. `make
  test-data` checks `site/` is up to date, and `PackIndexTests` that the index lists every pack
  in `packs/` at its manifest's version.
- **Kept for older engines** (2026-11-10). When a pack's new version raises `minEngine` (Hindi
  2026.11.10 needs engine 3), released apps on the older engine can't read it. The index then
  also lists the newest version they can read (Hindi 2026.11.06, engine 2), from
  `site-src/kept-packs.json`: entries written by hand with the `size`, `sha256` and `url` they
  were published with. Their files stay in R2. `make site-deploy` checks that they are still
  served and never uploads them. The script refuses an entry that isn't older, and for a lower
  engine, than the pack in `packs/`, or that no engine would pick. Within an id, the index lists
  the newest version first. Every app takes, per id, the highest version it reads
  (`PackIndex.newest(readableBy:)` for Download, `PackUpdates.offers` for updates), so a
  language shows once. The iOS code released as TestFlight build 2 already did this. An app on
  engine 3 with Hindi 2026.11.06 is offered 2026.11.10 as an update.
- Share-alike ([licence audit](licence-audit.md) §3): the packs and English's built-in word lists
  (`english-base-<sha256 prefix>.zip`, in the same bucket) are published under CC BY-SA 4.0
  without DRM, and `credits.html` says so and links them.

**Upgrading from the build with Malayalam inside.** On first open after the update, the app
sees learned Malayalam words but no Malayalam pack, and offers to download it. Learned words
live outside packs, so nothing is lost.
(Known limitation, for now: learned words are in the keyboard's own container, which the app can't read, and the
keyboard can't write the App Group without Full Access, so the app can't see them yet. The
offer could instead be made for every user of the old build, or the keyboard could say so in
the bar.)

**Updates.** `KeyboardCore.PackUpdates.offers` decides, given the user's packs, the index's
entries and the fingerprint of the bundled `en.fst` (`EnglishFingerprint`, the engine's
FNV-1a): an entry is offered when this build can read it, it isn't built against another
English, and either its version is higher, or the user's pack names another English (its LM
is off, decision 2) and the entry is at least as new. A pack without `englishFingerprint` is
never stale. A language not installed is never an update.

## Engine changes

- `EngineConfig` takes `english_dir` and `pack_dir` (the pack folder) in place of the
  fixed `malayalam`/`manglish`/`next_words`/`translit`/`lm` paths. The engine reads the
  manifest itself. It now takes `pack_dirs`, a list (`pack_dir` stays as a deprecated alias for
  one entry); each refused pack is reported in `Engine::pack_problems`.
- `Script` becomes a registry of script plug-ins behind a trait: `Script::Latin`,
  `Script::Malayalam`, `Other`. Malayalam's plug-in owns the logic now in `malayalam.rs`,
  the chillu half of `normalize.rs`, the syllable walk in `romanize.rs`, the joins in
  `suffix.rs`, and `translit.rs`. Its tables come from the pack.
- `Layout {Latin, Malayalam}` becomes `Layout {Latin, Native}`. `InputLayout` follows.
- `manglish.rs` `pack`/`unpack` take the script block base from the plug-in (`0x0D00` for
  Malayalam).
- The transliterator vocabulary is read from the model file, not from the hard-wired
  `LATIN_SIZE`/`MALAYALAM_SIZE`. For format 1, the existing Malayalam models are accepted
  with their current vocabulary.
- `malayalam_vowel_sign_base` becomes `native_mark_base(script, context_before)` (keyed by the manifest's `script`, since the keyboard asks on the main thread
  without the engine), with `vowel_signs` taken from the
  pack.
- `chat_forms.rs` loads the pack's file instead of `include_str!`.
- **Habit lines are keyed by language.** The habit tags `H m` / `H e` become `H <id>`
  (`H ml`, `H en`); a log with the old tags reads them as `ml` and `en` and is rewritten
  in the new tags when opened. Learned words stay in one `learned-words.log` for every
  language, each word's language following from its script and the packs; splitting the log by language later is one more migration.

### Script plug-ins

*Since engine 5 no script's rules are code:* each is its `lang.json` (see "Scripts as data"
below), and the engine embeds Malayalam's, Hindi's and Assamese's for packs in `mlym`, `deva` or
`beng` published without one. What follows is how the first three scripts were added, and the
table still describes their rules.

`engine/core/src/script/`, one file per script, registered in `mod.rs` (Malayalam first;
the blocks don't overlap, so registering a script never reclassifies another's text). A
pack's `minEngine` is 2 when it needs one of these scripts or the table roles (`PACK_ENGINE` is 2;
Malayalam's pack keeps `minEngine` 1). It is 3 for an LM in a pack of another script (Hindi's,
[hindi-lm.md](hindi-lm.md)): engine 3 flags the LM's pieces of the pack's script as native, where
engine 2 flagged only Malayalam's (`PACK_ENGINE` is 3).

| | `mlym` | `deva` (Hindi) | `beng` (Assamese) |
|---|---|---|---|
| Block | U+0D00–U+0D7F | U+0900–U+097F | U+0980–U+09FF |
| Not letters | U+0D64–U+0D79 | । ॥, digits, ॰ (U+0964–U+0970) | digits U+09E6–U+09EF, signs U+09F2–U+09FE |
| Normal form | NFC, atomic chillus, joiners kept only after a virama before a letter | NFC (nukta letters become consonant + ़; ऩ ऱ ऴ stay single), every ZWJ/ZWNJ dropped | as `data/langs/as/lang.json`: ত্‍ → ৎ, র → ৰ, া+ে → ো, অ+া → আ, around NFC with every ZWJ/ZWNJ dropped; য় ড় ঢ় stay base + ় |
| Phrase end | | । ॥ | । ॥ ৷ |
| Inner apostrophe | | | yes (হ'ল, ক'ত) |
| `mark_base` | a consonant | a consonant, or the consonant before a nukta | a consonant (ৰ ৱ included), or before a nukta; not ৎ |
| Loose spelling, final u, suffix joins | yes | none | none |

- The engine normalises every word with `script::normalize_word`: `normalize`, then the
  script's own rules for a native word. They are what lexbuild built the lexicon with
  (`lexbuild --lang` for hi, `lexbuild-native` for as), so a typed word meets the stored
  one; `make test-data` checks every word of `hi.fst` and `as.fst` is unchanged by them
  (`tests/script_normal_form.rs`: 300,000 and 253,459 words, 4,679 of them with an
  apostrophe, no exceptions), and lexbuild's tests check the beng rules against `lang.json`.
- Words: a word runs on across virama, nukta and joiners; a script's phrase end ends the
  word and the phrase (no next-word pair is learned across a danda). lexbuild's
  `romanisation.json` tables take their letters from the plug-ins.
- The romanised lexicon packs a word one byte per character from its script's block base
  (`manglish::pack`/`unpack`, `block_base`), with the apostrophe as 0x27.
- The র → ৰ fold is Assamese's, not the script's. A Bengali pack would need its own normal
  form: a language-level rule, or a plug-in per language. *Since engine 4:* the
  coded `beng` keeps only the script's replacements; the fold is in Assamese's `lang.json`,
  and in code only for an `as` pack without that file (`beng::ASSAMESE`).

### Scripts as data (engine 4)

A pack may carry its script's definition as `lang.json` (role `lang`), the same file
`lexbuild-native` built its lexicons with (`data/langs/<id>/lang.json`). The engine parses it
with `script::data::ScriptDef` (which lexbuild shares) and makes a plug-in of it
(`DataScript`): characters (`block` and/or `letters` ranges, less `notWord`), the normal form
(`replace` around `normalize`), `mark_base` from `bases` (a base that is a combining mark, the
nukta, stands for the base before it), `phraseBreaks` and `apostrophe`. Only `script` and the
characters are required. `script::for_pack` uses the file when there is one, except for a
coded plug-in that `needs_code` (Malayalam); `Sources::open` installs the rules for the script
(`script::install`), refusing a script whose characters overlap another's. `PACK_ENGINE` is 4;
only a pack in a script without coded rules needs `minEngine` 4. The installer accepts such a
pack when it has the file (`PackSupport.packScripts`) and asks the engine to open it first
(FFI `pack_problem`).
- Vowel-sign strips, as the `layout.json` files carry them (the order shown when
  nothing is preferred): Hindi ा ि ी ु ू ृ े ै ो ौ, then ं ँ ः, then ् ़; Assamese
  া ি ী ু ূ ৃ ে ৈ ো ৌ, then ং ঁ ঃ ৎ ্.
- `englishFingerprint` is read but not checked when a pack opens: an LM loads only if its
  own tables fit the lexicons (`lm_fits`), and with another `en.fst` next words still work,
  with slightly stale English pairs. A test checks the committed packs match the bundled
  `en.fst`; the installer compares it to offer an update.

## App and keyboard changes

- `Language` becomes `.english` or `.pack(PackDescriptor)` (the language
  key cycles `engine.layouts()`, and the bottom row puts the second page's key right of
  space and the third's left of it). The labels ("മലയാളം", "മ",
  "കഖ", "അആ") and the Malayalam pages come from `layout.json`. `Malayalam.swift` tables
  move into the pack.
- `EngineSuggestionProvider.bundled()` becomes `load(activePack:)`, built from the scanned
  packs.
- `Credits` are built from the manifests of the installed packs plus English.
- Emoji words and search are loaded per active pack plus English.
- **App: a Languages screen** (list, active choice, import, download, remove). Its look is
  designed separately.

### More suggestions (decided default, open to change)

The bar can hold 3, 10 or 20 suggestions: the keyboard settings panel's Suggestions control
(`KeyboardSettings.suggestionCount`), **10 by default**. The engine returns at most
`max_suggestions` candidates (`Engine::set_max_suggestions`, `EngineConfig.max_suggestions`;
`DEFAULT_SUGGESTIONS` 3, `MAX_SUGGESTIONS` 20). More only adds to the end: the extra ones come
from what the engine already ranks (completions, corrections, romanised spellings, next
words), and the first three are the same three in the same order whatever the count
(`make golden-more` checks 3, 10 and 20 over the golden inputs; with 3, `make golden-check` is
identical). The bar's three slots look exactly as before; the rest follow the right slot,
each as wide as its word, and a finger slides the bar to them. A tap commits one as a slot tap
does; new content starts the bar at the beginning; an emoji keeps the right slot and the word
it displaces leads the rest. Changing the count is set on the open engine, no reopen.

Latency on the Mac (`bundle.rs` `latency_by_suggestion_count`, with the LM): mid-word p99
3.67 ms at every count; after a space p99 1.18 / 1.90 / 2.20 ms at 3 / 10 / 20 (the next-word
search keeps more leaders). Over every eval prefix the mean is 0.64 / 0.66 / 0.68 ms. 20 stays
well inside the keyboard's cutoffs, so it is offered.

## Build

- A pack is built step by step, as [make-a-language.md](../make-a-language.md) describes:
  `scripts/pack.py --write` writes the manifest's checksums and `--zip` the `.neopack` file
  (`make pack` zips the Malayalam pack).
- Debug builds copy chosen packs into the extension's `Packs/` so packs can be tried on a
  phone without the installer. Release builds copy none. Debug builds bundle `packs/ml.neopack`; `make twopacks` builds a Debug app with
  `ML_DEBUG_PACKS=hi` (`scripts/debug-packs.sh` copies `packs/hi.neopack` in beside it) and
  `ML_ACTIVE_LANGUAGES=ml,hi` (Info.plist `MLActiveLanguages`, set at engine open), in
  `build/DerivedData-twopacks`. Both settings are empty by default, and the script then
  removes any extra pack an earlier build left. With `hi` and `as` as the extra packs, the language key reaches their native layouts.
- `lexbuild` also writes the English base's own `next.fst` from the English vocabulary alone.
  (`en-next.fst` beside `en.fst` in `Lexicon/`, the same English pairs it puts in
  the pack's `next.fst`. The committed file was made from `packs/ml.neopack/next.fst` by
  `lexbuild-english-next`; `make test-data` checks they agree. The engine reads it only while
  English is active and no active pack has a `next.fst`, so with Malayalam active nothing
  changes.)

## Out of scope

- Swipe for native-script layouts. Swipe stays Latin; it already covers romanised typing.
- An LM independent of English.
- More than `MAX_ACTIVE` (4) languages at once: there are no more packs yet. Four was
  measured in the engine (decision 4), not yet on the phone.
- A dedicated language identifier. With N > 2 the previous word, the habit and the LM pick
  a favoured pack (decision 4); with none of them (a new user, the start of a message) the
  bar is by score, and the smallest pack's frequent words can take the middle (`pani` →
  পানী with Assamese on). Comparable frequencies across packs, or an identifier, would fix it.
- Lazy loading of inactive packs. Every installed pack is opened; its files are mapped and
  cost little heap, so it isn't needed yet.
- Pack signing beyond checksums. A pack is data and is read-only. Signing can be added if
  third-party pack sources appear.

## Testing

- Moving Malayalam into a pack left the eval baselines byte-identical. Unit tests cover the
  manifest parser (missing optional files, unknown `format`, wrong `script`) and a
  round-trip of the Malayalam tables from code to pack files.
- A migration test for learned words (old log in, split logs out). Swift tests for
  labels and pages from `layout.json`.
- Installer tests (bad checksum, interrupted unzip, replacing a version, refused
  script) and fallback tests (pack missing at load). Done: `PackInstallerTests`,
  `ZipReaderTests`, `PackVersionTests`, `PackUpdatesTests` (KeyboardCore), and
  `tests/pack_fallback.rs` plus the FFI's fallback tests. The screens' logic has
  `ActiveLanguagesTests`, `PackIndexTests` and `PackMessagesTests`.
- `engine/core/tests/packs_hi_as.rs` (layouts load through the manifest, every key
  is in the pack's script and lexicon, the strip order, typed native keys give suggestions)
  and KeyboardCore's `PackLayoutTests` (language key, labels and pages).
