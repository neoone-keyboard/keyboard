# Hindi LM

Date: 2026-11-10. How the Hindi pack's language model was built, as a worked example of a
pack LM beside [the pack format](pack-format.md). The Hindi pack (`packs/hi.neopack`, version
2026.11.10) now ships `lm.mlkbnn`, a bilingual English and Hindi LM built by Malayalam's recipe.
Shape, tokeniser size, training budget, export and gates are unchanged from Malayalam's LM
(`training/mlkb/lm/`); this document records only what differs and the measured numbers.

## 1. Pipeline

- `training/mlkb/lm/lang.py` holds each language's settings: native text, romanised bucket
  name, romaniser, pack lexicon, Dakshina and Aksharantar files, eval files and build paths.
  Every `mlkb.lm` command takes `--lang`. `ml` is the default and keeps its paths, bucket names
  and seeds.
- `make lm-data LM_LANG=hi` builds the text, vocabulary and shards in `training/build/lm-hi`.
  `make lm LM_LANG=hi` trains `training/build/lm-hi.pt`, then exports, runs the memorisation
  check and the eval's LM gate, and copies the model into the pack. The variable is `LM_LANG`
  rather than `LANG`, because `LANG` is the locale. `make lm-data` and `make models` build
  Malayalam as before.
- `make bench-hi` runs the latency gates on the Hindi pack.

## 2. Data

All sources were already credited by the Hindi pack, or were used by Malayalam's LM. No new
source was fetched.

| Bucket | Source | Words | Tokens | Tokens/word |
|---|---|---|---|---|
| `en-wiki`, `en-simple`, `en-news`, `en-gutenberg` | as Malayalam's LM | 213M, 43M, 7.4M, 104M | 340M, 70M, 11M, 150M | 1.44–1.65 |
| `hi` | Hindi Wikipedia, then Hindi Wikisource (`data/raw/text/hi.txt`, the pack lexicons' extract) | 97.2M | 161M | 1.66 |
| `hinglish-{0,1}` | `hi` romanised word by word with Hindi's spelling table (`langs/hi-spelling.json`, `table_romanise.variants`); a loanword becomes its English with p = 0.7; dandas become full stops | 95.0M each | 337M | 1.77 |
| `mixed-0` | `hi` with 20–80% of words romanised, loanwords in English with p = 0.3 | 95.1M | 163M | 1.72 |

- Hindi Wikipedia is 3.7 times the size of Malayalam's, so a single Hinglish pass already
  holds about 170M tokens. The Hindi build therefore makes two Hinglish passes and one mixed
  pass, where Malayalam made four Manglish passes and two mixed. The bucket weights are
  unchanged, so training (1.1B tokens) sees about 1.5 epochs of `hi`, 0.55 of Hinglish and 0.75 of mixed.
- Clean-up follows Malayalam's rules: paragraphs deduplicated with digits masked (142,584
  Hindi paragraphs dropped), 13,446 templated Hindi sentences cut past 3 copies, and runs cut.
- Leaks: every sentence of every eval set, and every Dakshina `hi` romanised dev and test
  sentence, is removed from the training text by hash, in both scripts and with and without
  punctuation. The Hindi text lost 8,516 sentences this way (47,205 leak keys).
- Loanwords: 8,567. They come from mined Aksharantar `hin_train.json` rows (IndicCorp,
  Samanantar) and from English words whose first transliteration is a Hindi word. Plural and
  oblique endings (ों, ें, ियां, ियाँ) are excluded.
- Vocabulary: 16,384 pieces. Hindi script and Hinglish fell short of their 35% and 25% sample
  shares (2.59M and 1.89M sentences). The Rust tokeniser matches `sentencepiece` on 100% of
  10,000 validation lines.

**Known limitation, for the next run.** The training text was built before 4e848de, which made
each pack's transliterator decode into its own script. Until then the Hindi transliterator
decoded into Malayalam script, so the transliterator-based half of the loanword search found
nothing, and the loanword list is probably smaller than intended. The training text itself is
not corrupted. Rebuilding with the fixed engine should mark more loanwords; the model was not
retrained for this.

## 3. Engine changes

- The LM tokeniser used to flag only Malayalam pieces as native. Now the engine sets the
  native block from the pack's script when it opens the pack's LM (`Lm::set_native`).
  Malayalam keeps its block, so its output does not change (`make golden-check`: identical,
  and identical for ml-only). Ranking code and weights are untouched. Hindi uses
  `rank.rs`'s LM weights as they are: on the Hindi gate sets they already beat no LM on every
  gated metric, so no Hindi-specific sweep was needed.
- `PACK_ENGINE` is now 3, and the Hindi pack has `minEngine` 3. An engine-2 build would load
  the LM but flag none of its Devanagari pieces. It would then offer no Hindi next words and
  tilt the bar towards English, so engine-2 builds refuse the new pack and keep 2026.11.06.

## 4. Eval sets (`data/eval/hi/`)

| Set | From | Size |
|---|---|---|
| `next-hi.heldout.txt` (gate), `next-hi.tune.txt` | held-out Hindi Wikipedia and Wikisource paragraphs, 4–20-word sentences split by hash, none of them a sentence of the training text | 916, 972 sentences |
| `next-hinglish.tsv` (gate), `.tune.tsv` | Dakshina `hi` romanised dev sentences, word-aligned, split into disjoint halves | 1,000, 500 rows |
| English no-harm | `english-sentences.txt`, `english-typos.tsv`, `english-typos-context.tsv` (shared with Malayalam) | |
| `translit-human.tsv` | the existing Hindi transliteration set | 2,000 |

`python -m mlkb.eval --lang hi` runs them. `--record-nolm` records the run without the LM in
`baselines-nolm.json`. That record was taken again after 4e848de: the first one had the
transliterator bug, with Hindi transliteration at top-1 0.1225 and top-3 0.5680, against 0.3010
and 0.6505 with the fix. `--gate lm` holds an LM to the LM ship rule against that record.
Next-word top-3 and keystroke savings must rise on every next-word set, top-1 on typos in
context must rise, and English wrong replacements must not.

## 5. Training

Same model and recipe as Malayalam: 8,933,376 parameters, 120,000 steps, MPS float32. The run
took 5.04 h at about 8 steps/s and 67k tokens/s. The best checkpoint was the last one, and
early stopping never fired.

Validation loss in nats per token, at the final step: en-wiki 4.065, en-simple 3.804, en-news
4.291, en-gutenberg 4.372, hi 4.420, hinglish 4.823, mixed 5.070. Score 4.612 (ppl 100.6).
The score was 5.63 at 2k steps, 5.05 at 10k, 4.85 at 50k, 4.69 at 90k and 4.61 at 120k.
Malayalam's run reached 4.399.

## 6. Results

The Hindi pack with English and its transliterator, with and without the LM, on the gate files:

| Set | Without LM: top-1 / top-3 / KS | With LM: top-1 / top-3 / KS |
|---|---|---|
| next_hi (8,279 positions) | 0.1359 / 0.2338 / 0.3405 | **0.1860 / 0.2950 / 0.3838** |
| next_hinglish (10,051) | 0.1495 / 0.2596 / 0.2451 | **0.2084 / 0.3261 / 0.2945** |
| next_en (1,576) | 0.0838 / 0.1561 / 0.1956 | **0.1231 / 0.2348 / 0.2721** |
| english_typos_context (125) | 0.8480 / 0.9360 | **0.9040 / 0.9760** |
| english_typos (176) | 0.8750 / 0.9716 | 0.8750 / 0.9716 |
| translit-human (2,000) | 0.3010 / 0.6505 | 0.3010 / 0.6505 |
| English wrong replacements | 3 / 1,876 | 3 / 1,876 |

The LM ship gates pass. `baselines.json` records the run with the LM.

**Memorisation** passes (`data/eval/hi/memorisation.json`). 2,000 prompts (500 each from
English Wikipedia, Simple English Wikipedia, Hindi Wikipedia and Hindi Wikisource), 3
generations each, matched against 1.07B indexed 16-token windows. The longest continuation of the source
is 8 tokens. The longest run found in the share-alike text is 34 tokens (limit 50), and none of
the long runs is a loop. Malayalam's figures were 10 and 42.

**Size:** `lm.mlkbnn` is 9,557,016 bytes, under the 10 MB limit. The pack zip is 26.2 MB.

**Latency** (`make bench-hi`, Mac, release build):

| Measure | Without LM | With LM | With LM, keyboard cutoffs |
|---|---|---|---|
| suggest, all (1,820 requests): p50 / p99 | 0.10 / 2.59 ms | 0.31 / 2.50 ms | 0.31 / 2.50 ms |
| after a space: p99 | 0.008 ms | 0.53 ms | 0.52 ms |

The suggest gate (p99 < 8 ms) passes. LM commit p50 is 0.26 ms and p99 0.36 ms (gate 1 ms). A
fresh 32-token prefill takes p99 3.0 ms. The sliding context cache predicts within +0.23%
(English) and +0.36% (Hindi) of fresh windows (limit 2%).

## 7. Release

The pack is version 2026.11.10, with `files.lm` and its sha256 added. Its credits add the LM's
other English sources: Simple English Wikipedia, English Wikinews (CC BY, attributed to
Wikinews) and Project Gutenberg. `make site-packs` writes `hi-2026.11.10.neopack` into the
index. Deploying is left to the maintainer (`make site-deploy`). Once deployed, the index lists only
the engine-3 Hindi pack, so engine-2 builds will no longer be offered Hindi.
