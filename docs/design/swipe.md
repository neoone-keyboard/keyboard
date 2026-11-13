# Swipe typing

How a word drawn across the QWERTY keys is read. Written for English and Manglish (Malayalam typed in Latin letters); since then each pack's romanised words are reached the same way, through the pack's own skeleton (see [the romanisation format](romanisation-format.md), `swipe`).

## 1. Goal

Drawing a word across the QWERTY keyboard (English/Manglish layout) types it. A swipe can give an English word or a Manglish word; a Manglish swipe types its Malayalam word, as a typed Manglish word does when its suggestion is chosen. Offline, inside the 40 MB budget, no new model.

Out of scope: swipe on the Malayalam layout, swipe on the symbol and emoji pages, continuous phrase swipe (several words without lifting), a setting to turn swipe off (add it only if it misfires).

## 2. Fixed alongside

Two things were fixed with swipe, because it adds work on top of both:

1. The space key starts its next-word suggestions at touch-down, like letters already do (2 of the 4 empty bars on the new build came right after a space).
2. Find what each keyboard opening leaves behind: memory at load rose from 12 MB to 24.6 MB over 25 opens (about 0.5 MB each) while old controllers were released.

## 3. Gesture (KeyboardCore)

- A touch that starts on a letter key of the QWERTY layout and moves more than `swipeActivation` (one key width) from its start before the long-press fires becomes a swipe. `KeyGesture` gains a `swiping` mode; from then on it records points and never types the start key.
- Spacebar cursor drag, backspace swipe-to-delete-word, long-press alternates and the language-key settings hold are unchanged: they start on other keys or after the long-press.
- A second finger landing during a swipe is ignored until the swipe ends.
- Points are kept in the keyboard view's coordinates, at most one per 4 pt of movement and at most 256 points; on lift the gesture emits `.swipe(points)`.
- `KeyGesture` stays pure and unit-tested on macOS: activation distance, long-press-before-move still opens alternates, short wobble still taps.

## 4. Decoder (engine/core `swipe.rs`)

Input: the path, and the centre and size of every letter key in the same coordinates, sent by the app on each layout change (number row on/off and screen size move the keys). Output: up to 20 candidates, each the word to insert, its source (English, Manglish, learned) and a score.

1. **Resample** the path to evenly spaced points (0.4 key widths apart, at most 64 points). It is not smoothed: on the eval set smoothing cost accuracy, because it rounds off the corners that mark letters.
2. **Walk the word lists along the path.** Beam search, width 256, stepping letter by letter through two letter trees at once:
   - `en.fst` (English words, with their counts);
   - the skeleton part of `manglish.fst` (the key before the separator), so each Manglish entry is reached by its skeleton. The skeleton merges doubled letters and folds `kh`/`gh`→`k`, `ee`→`i` and so on, which is what a swipe can express anyway.
   Each partial word holds its position along the path. Adding a letter moves it forward to the path point nearest that key and costs the distance (in key widths) from the path to the key. The first letter must be near the start of the path and the last near the end. Keys the path only passes over cost nothing. A doubled letter costs nothing extra.
3. **Finish** with words that end in the tree and near the end of the path. For a Manglish skeleton, expand to its Malayalam words (the packed words after the separator) and keep the best few by count.
4. **Rank** the top 20 with the same scorer typed words use: path fit + word count + LM score for the sentence so far + learned-word counts + the language bias chip (Auto / English / Malayalam). The mixing weight for path fit is tuned on the eval set (§7).
5. **Learned words** take part: words the user has taught the keyboard are added to the beam's word sources.

The decoder is exact and bounded: beam width, path length and candidate count are capped, so time grows with the path length only.

Performance: the path is decoded once, at lift. On the Mac a decode takes p50 2.6 ms (English) to 4.9 ms (Manglish), p90 under 6 ms, so starting while the finger moves was not needed. Target on iPhone 15 Pro Max: p90 ≤ 25 ms from lift to candidates; a swipe over 25 ms is logged as slow.

## 5. Typing the result (KeyboardExtension)

- On lift the best candidate is inserted, with a space before it if the cursor follows a word and a space after it (as SwiftKey does).
- The bar shows the next two candidates; tapping one replaces the inserted word.
- Backspace straight after a swipe deletes the whole swiped word (and its space).
- The inserted word counts as a commit for learning and next-word prediction, like a chosen suggestion.
- Shift: a capital start key or shift on capitalises the first letter; caps lock types the word in capitals. Malayalam words are left as they are.
- Fallback: if the words arrive more than 250 ms after lift (the engine was busy or opening), the decode errors, or prediction is off for memory, nothing is typed and the bar briefly says "Swipe again". Typing is never blocked.

## 6. Trail

A smooth line in the accent colour that follows the finger: thick at the finger, thinning and fading along the last part of the path, gone about 0.2 s after lift. Drawn in one `CAShapeLayer` over the keys (no per-point views), cleared on lift and when the keyboard is hidden. The keys under the finger don't light up during a swipe.

## 7. Evaluation

- **Synthetic swipe set:** for each word of the English and Manglish eval sentences (`data/eval/english-sentences.txt`, `next-manglish.tsv` and their held-out splits), draw a path through its key centres with noise: each key aimed at with σ 0.22 key, a shake of 0.04 key along the path, corners cut by up to 0.4 key, and the spellings people use for Manglish (`sugamano`, `sukhamano`). Generated in `engine/core/tests/swipe_eval.rs`, with a fixed seed.
- **Gates** (tuned on the tune split, reported on held-out): top-1 and top-3 accuracy for English and for Manglish, recorded in `data/eval/baselines-swipe.json` (apart from `baselines.json`, which `mlkb.eval --record` rewrites whole) and checked by `make eval-swipe`; later changes must not drop them.
- **Speed:** a Rust benchmark of decode time per word length; a device check with the same paths through the FFI.
- **Unit tests:** gesture activation (§3), beam correctness on tiny hand-made word lists, doubled letters, skeleton matching, the 256-point cap.

## 8. Memory

No new files: both word lists are already memory-mapped. Beam state is a few KB. The trail layer is one path. Expected cost below 0.5 MB.

## 9. Logging

One line per 100 swipes: count, how many were replaced from the bar or deleted straight away (a misread), and p50/p90 decode time. Slow swipes (over 25 ms) are logged like slow suggests, with path length and candidate count. No words or paths are written to the log.
