# Pack file: `romanisation.json` (format 1)

How a language's native words are spelled in Latin letters (Manglish, Hinglish, romanised
Assamese), and the "skeleton" that folds away spellings people don't type consistently. The
pack builder uses it to key the romanised lexicon. At run time the engine applies `skeleton`
to what was typed. It is data for the script plug-in. The engine's code walks the syllables,
and this file says what each letter becomes.

The format reads a word the way Brahmic scripts write it: a consonant carries an inherent
vowel, a vowel sign replaces it, and a virama removes it. A script with no virama and no
written short vowels fits too, with `"virama": ""` (Urdu: its vowel letters are `vowelSigns`,
its aspirates `clusters`, and the skeleton drops the vowels it can't see). Every field is required unless marked optional. Keys
are strings and may be more than one code point (for example a consonant plus nukta, क़).
The walk tries the longest key first.

```json
{
  "format": 1,
  "script": "mlym",
  "virama": "്",
  "inherent": "a",
  "atWordEnd": {
    "virama": "u",
    "inherent": "a"
  },
  "independentVowels": { "അ": "a", "ആ": "aa" },
  "vowelSigns": { "ാ": "aa", "ി": "i" },
  "consonants": { "ക": "k", "ഖ": "kh" },
  "finals": { "ൻ": "n", "ം": "m" },
  "clusters": { "ന്റ": "nt", "റ്റ": "tt" },
  "skeleton": {
    "dropTrailing": { "letters": "yh", "afterVowel": true, "minLength": 3 },
    "pairs": [["chh", "c"], ["ch", "c"], ["kh", "k"], ["gh", "k"], ["jh", "j"],
              ["th", "t"], ["dh", "t"], ["ph", "p"], ["bh", "b"], ["sh", "s"], ["zh", "z"],
              ["aa", "a"], ["ee", "i"], ["ii", "i"], ["oo", "u"], ["uu", "u"]],
    "singles": { "c": "k", "q": "k", "d": "t", "f": "p", "w": "v", "x": "ks" },
    "collapseDoubles": true,
    "dropFinalVowel": { "vowels": "ue", "afterConsonant": true, "minLength": 3 }
  }
}
```

## The walk

The word is NFC-normalised and its ZWJ and ZWNJ are removed. Then, for each position:

1. If a `clusters` key starts here, emit its Latin and treat it as one consonant.
2. Otherwise, if a `consonants` key starts here, emit its Latin. Then:
   - if a vowel sign follows, emit that sign's Latin;
   - if the virama follows, emit nothing (or `atWordEnd.virama` if the word ends there);
   - otherwise emit `inherent` (or `atWordEnd.inherent` if the word ends there).
3. Otherwise emit the `independentVowels` or `finals` entry for the letter.
4. ASCII letters pass through lower-cased. Anything else is skipped.

`atWordEnd` handles languages whose words end differently from their middles:
- Malayalam writes a final virama as "u" (വീട് → veetu) and keeps the final inherent "a".
- Hindi drops the final inherent vowel (schwa deletion: कमल → kamal), so it sets
  `"inherent": ""`.

Medial schwa deletion (Hindi समझना → samjhna) is not in the walk: the walk keeps the vowel
(samajhana). The skeleton's optional `dropBetweenConsonants` step lets the typed form with or
without it meet the walked form.

## The skeleton

The skeleton is applied to Latin text, both the walk's output and what the user typed:

1. Lower-case it and keep only ASCII letters.
2. `dropTrailing`: remove one final letter from `letters` when it follows a vowel and the
   text is at least `minLength` long (entey → ente).
3. Left to right: at each position, use the first `pairs` entry that matches. Otherwise map
   one letter through `singles`, or keep it.
4. `collapseDoubles`: never emit the same symbol twice in a row.
5. `dropBetweenConsonants` (optional, default `""`): remove every letter of it that has a
   consonant on both sides, all at once (Hindi `"a"`: samajhana, samajhna, samjhna → smjna).
6. `dropFinalVowel`: remove a final vowel from `vowels` when it follows a consonant and the
   text is at least `minLength` long before the vowel is removed (ente → ent with 3).

The `pairs` order matters. Longer patterns go first (`chh` before `ch`).

Malayalam's file reproduces the engine's original coded tables exactly (every word of `ml.fst`
was checked before the code was removed), and
`engine/core/tests/romanisation_tables.rs` (in `make test-data`) checks that every key of every
pack's romanised lexicon is one of the loaded tables' skeletons of its word (one, or two with
`optionalFinals`, below).

Spellings that depend on context (Assamese: a sibilant is "x" alone but "s" in a cluster) are
listed as `clusters` in format 1. A per-context consonant spelling may come in a later format if
the lists grow unwieldy; Assamese needs about 250 entries today.

## Optional sections, added later

- `looseSpelling` (engine 6 reads it; older engines use Malayalam's): which typed spellings
  spell a word out, for ranking (`Romanisation::relaxed`, `manglish::fit_latin`).
  `{"singles": {"d": "t"}, "pairs": [["ee", "i"], ["oo", "u"]], "collapse": "aiu"}` is
  Malayalam's, and what a file without the section is read with: left to right, each letter is
  mapped through `singles`; if the last letter kept and this one are a `pairs` pattern, the two
  become the pair's letter; else a letter of `collapse` equal to the last one kept is dropped.

- `optionalFinals` (a list of `finals` keys; older engines ignore it): signs people leave
  unwritten at a word's end. Hindi's is `["ं", "ँ"]`: नहीं walks as `nahin` and is written
  `nahi` at least as often, हूँ `hun` and `hu`, में `men` and `me`. A word that ends in one
  has a second spelling, its walk without that sign's Latin (`Romanisation::romanize_short`),
  and so a second skeleton when the two differ (`word_skeletons`: `nhin` and `nhi`). The
  romanised lexicon stores the word under both keys with the same count
  (`lexicon::build_with_aliases`: the file's total counts it once, so no word's probability
  moves), a lookup returns it once, under the nearer key, and either spelling spells it out
  (`manglish::fit`). A swipe finds it through either skeleton. Only the sign at the very end
  is optional, and only after a written vowel (a vowel sign or an independent vowel; after a
  bare consonant, as in स्वयं or सं, it is the syllable's ending and is written): the anusvara of हिंदी is always written, and a final न (दिन `din`) is a
  consonant, not the sign, which is why this is not a `dropTrailing` letter (that would fold
  दिन into दी). Each entry must be a key of `finals` with a Latin spelling. An older engine
  reads the pack's lexicon with both keys and finds the word under either; it only doesn't
  count the short spelling as spelling the word out.

- `skeleton.dropTrailing.alsoKept` (true or false, default false; older engines ignore it):
  with `dropBetweenConsonants`, a word whose walk ends consonant, dropped-between letter,
  dropped trailing letter (Hindi जगह `jagah`, तरह `tarah`, सुबह `subah`) has the skeleton
  `jga`: once the `h` is gone the last `a` is no longer between consonants. A swipe then has
  to return to `a` before the `h`, and a finger that goes j, g, h never finds the word. With
  `alsoKept` such a word is also keyed by the skeleton of its walk with the trailing letter
  kept (`jgh`), when that is no longer than the first and at least three letters (`kh` for
  कह would stand for any path from k to h). The second key is stored as an `optionalFinals`
  one is. It is for swipes: typed `jagah` still folds to `jga`.

- `skeleton.alsoBetweenConsonants` (letters, default `""`; older engines ignore it): letters
  people write for the vowel `dropBetweenConsonants` drops. Hindi's is `"u"`: हम is `ham`
  and, more often, `hum`; हमसे `humse`, हमारा `humara`. Nothing changes in the skeleton or
  the lexicon's keys, so a word with a real u keeps its key (कुछ `kuc`, तुम `tum`). The typed
  letters get a second reading (`Romanisation::respellings`): their skeleton with every
  such letter between two consonants dropped (`humse` → `hmse`). `manglish::lookup` adds the
  words of exactly that skeleton after the words of the typed one, never as spelled out, at
  the distance of the letters with the vowel written back (`hamse`) plus one; the engine
  offers them in a side slot and never puts one in the middle (a word people write this way
  all the time belongs in `chat-forms.tsv`, as Hindi's forms of `hum` are). A swipe does not
  read a drawn `u` this way: it made every skeleton stand for its u-words too and cost Hindi
  two points of first place on the held-out swipes.

- `skeleton.alsoBesideConsonant` (an object, letter to consonants, default `{}`; older
  engines ignore it): letters people write for the vowel `dropBetweenConsonants` drops, but
  only beside the consonants named. Hindi's is `{"e": "h"}`: the short a before ह is said
  [ɛ] and written e (कहते `kehte`, रहना `rehna`, पहले `pehle`), and so is the one after the ह
  when the one before it is short too (बहन `behen`, शहर `sheher`). Each key is one lower-case
  letter, each value lower-case consonants. A letter of the skeleton, or of the typed
  letters, stands in for the dropped vowel when it is between two consonants and either the
  consonant after it is one of its own, or the consonant before it is and the letter before
  that is a consonant (the vowel between them was dropped), the dropped vowel still written,
  or another stand-in. So `kehte` → `khte`, `behen` and `bahen` → `bhn`, `sheher` → `shr`;
  `her` (the h begins the word), `mohen` (the h follows a kept vowel), `khet` and `sher`
  (the h is the second letter of a pair the skeleton folds) have no such letter. Nothing
  changes in the skeleton or the lexicon's keys (मेहनत keeps `mehnt`). The typed letters'
  other readings (`Romanisation::respellings`) are then up to two: with these letters
  dropped, and with those of `alsoBetweenConsonants` dropped as well when that differs
  (`mehsoos`: `mhsus`, महसूस's, then `mhss`). `manglish::lookup` treats the words of each
  as it does for `alsoBetweenConsonants`: after the words of the typed skeleton, never
  spelled out, one more than the distance of the nearest reading written out (`kahte`), a
  side slot and never the middle. Unlike `alsoBetweenConsonants`, a swipe reads the letter
  this way too (`swipe::SkeletonRules::also_beside`): where the rule allows, the path may go
  by way of the letter between the two consonants, as it may by way of the dropped letter
  itself. A skeleton fitted so (`SkeletonRules::stood_in`: its drawing has the letter and
  the skeleton does not) gives up `Weights::stand_in` (2.0), so a word with the real letter
  stays ahead. Measured on Hindi (2,000 held-out swipes): first 17.60% → 17.75%, top three
  31.05% → 31.30%, English beside Hindi 87.87% → 87.93% first; without the weight a real
  `eh` word lost to its neighbour too often (`behtar` gave बाहर 161 times in 200). One that
  still does: `chehra` gives कहा (c, e, h, a) more often than चेहरा.

- `swipe` (read from engine 6 on; older engines use Malayalam's for every pack):
  `{"aspirated": "ktpb", "trail": "h"}`, the skeleton letters a drawn `h` may follow and the
  letters a path may draw after a skeleton's end. Each is derived from the skeleton when
  absent (`romanisation.rs` `swipe_rules`): `aspirated` is the last letter of what each
  `pairs` pattern ending in `h` becomes, `trail` is `dropTrailing.letters` plus
  `dropFinalVowel.vowels` plus each `singles` letter that becomes one of those vowels
  (Assamese's `o`, read as its dropped final `a`). The keys a skeleton letter is drawn through
  are always derived: the letter, each `singles` letter that becomes it, and the first letter
  of each `pairs` pattern that does. So are the letters a path may draw inside a word: those
  of `dropBetweenConsonants`, between any two consonants of a skeleton (Hindi's `a`: a swipe
  through k-a-r-n-e finds `krne`). Malayalam's file derives exactly what `skeleton_keys`, `ASPIRATED` and `TRAIL`
  were in swipe.rs.
- `fromLatin` (read from engine 6 on; older engines spell by Malayalam's coded rules for a
  Malayalam pack and not at all for others): the rule-based Latin → native speller
  (`translit::Speller`), for a guess when no lexicon word or model answers. Parts and
  semantics are documented on `RawFromLatin` in `engine/core/src/translit.rs`; Malayalam's
  file holds what that module was as code. Without the section the engine derives one from
  the walk tables (`Speller::derived`).
- Script-specific notes for the plug-in, as free text under `"notes"`.
