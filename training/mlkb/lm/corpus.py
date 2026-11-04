"""The LM training text: one file per bucket and pass, one paragraph per line, `lm_normalize`d.

    uv run --group train python -m mlkb.lm.corpus --out build/lm/text [--seed 20261103]

Reads data/raw/text/ (see `mlkb.lm.sources` and data/build-lexicons.sh) and writes:

| File | From |
|---|---|
| `en-wiki.txt` | `en.txt`, the first `EN_WIKI_WORDS` words |
| `en-simple.txt`, `en-news.txt`, `en-gutenberg.txt` | `simplewiki.txt`, `wikinews.txt`, `gutenberg.txt` |
| `ml.txt` | `ml.txt` (Malayalam Wikipedia, then Wikisource) |
| `manglish-{0..3}.txt` | `ml.txt` through `romanise_sentence`, `p_loan` 0.7, a new seed per pass |
| `mixed-{0,1}.txt` | `ml.txt` through `mix_sentence`, `frac` ~ U(0.2, 0.8) per sentence, `p_loan` 0.3 |
| `valid-<bucket>.txt` | the held-out paragraphs of each source, with the same transforms |

Rules (design §5, §10): paragraphs are deduplicated exactly after masking numbers (Gutenberg's
consecutive dialogue copies excepted); training paragraphs lose runs of one repeated unit past
`MAX_RUN` characters (`cut_runs`) and every sentence of `TEMPLATE_MIN_WORDS` or more words past
the first `TEMPLATE_CAP` with its digits-masked key (`keep_template`); every sentence that is in
an eval set, or is a Dakshina romanised dev or test sentence, is cut out of its paragraph
(`is_leak`); 1/`VALID_EVERY` of the
paragraphs of every source is held out by a hash of its first 64 characters; and 30% of the
paragraphs of every bucket lose their punctuation (chat register).

Also writes `heldout-en.txt` and `heldout-ml.txt` (2,000 held-out sentences each, for
perplexity) next to the output directory, the tune splits `data/eval/next-en.tune.txt` (Simple
Wikipedia and Wikinews, 500 short held-out sentences) and `data/eval/next-ml.tune.txt`
(Malayalam Wikipedia and Wikisource, `TUNE_SENTENCES_ML`), and the next_ml gate's held-out half
`data/eval/next-ml.heldout.txt` (`GATE_SENTENCES_ML`): never trained on, and no Malayalam one
is a sentence of the training text.
"""
import argparse
import hashlib
import json
import math
import random
import re
import sys
import time
from collections.abc import Callable, Iterable, Iterator
from functools import lru_cache
from multiprocessing import Pool
from pathlib import Path

import mlengine

from mlkb.data import calibrate, human
from mlkb.data.romanise import _slots, variants_of

REPO = Path(__file__).resolve().parents[3]
RAW_TEXT = REPO / "data/raw/text"
EVAL = REPO / "data/eval"
LEXICON = REPO / "apps/ios/KeyboardExtension/Lexicon"
DAKSHINA_ROMANIZED = REPO / "data/raw/dakshina/dakshina_dataset_v1.0/ml/romanized"
AKSHARANTAR_TRAIN = REPO / "data/raw/aksharantar/extracted/mal_train.json"
SMC_BORROWED = REPO / "data/raw/smc-words/english-borrowed.txt"
TRANSLIT = REPO / "apps/ios/KeyboardExtension/Models/translit.mlkbnn"

SEED = 20261103
EN_WIKI_WORDS = 230_000_000
VALID_EVERY = 200  # 0.5% of paragraphs
MIN_LEAK_WORDS = 2
# A Malayalam paragraph goes into the synthetic buckets only if at least this share of its
# words are Malayalam: ml.txt also holds English citations and titles, which romanising leaves
# as they are, so they would only repeat English text six times over.
SYNTHETIC_MIN_MALAYALAM = 0.5
# Image and gallery markup the wiki extractor leaves behind ("file:x.jpg|thumb|right|").
_MARKUP = re.compile(r"\|thumb\b|\.(?:jpe?g|png|svg|gif)\b", re.I)
NO_PUNCT_SHARE = 0.3
MANGLISH_PASSES, MANGLISH_P_LOAN = 4, 0.7
MIXED_PASSES, MIXED_P_LOAN, MIXED_FRAC = 2, 0.3, (0.2, 0.8)
HELDOUT_SENTENCES = 2000
TUNE_SENTENCES, TUNE_WORDS = 500, (4, 20)
# Malayalam held-out sentences are split by a hash into the tune half (`TUNE_SENTENCES_ML`, for
# the weight sweep) and the gate half (`GATE_SENTENCES_ML`, `next-ml.heldout.txt`, which the
# next_ml gate reads with the SMC conversations): together about 7,000 next-word positions each,
# so that one row no longer decides a ship gate (the SMC set alone has 436).
TUNE_SENTENCES_ML, GATE_SENTENCES_ML = 1000, 950
# Aksharantar sources that are mined text (CC0): their Latin side is how the word is written.
LOANWORD_SOURCES = frozenset({"IndicCorp", "Samanantar"})
LOANWORD_MIN_LOG_PROB = -14.0
TRANSLITERATED_WORDS = 60_000  # most frequent English words run through the transliterator
# Case and clitic endings: a mined pair like കമ്മിഷനും -> "commission" drops the ending, and
# nobody would type "commission" for it, so inflected forms are not loanwords.
LOANWORD_ENDINGS = ("ും", "ിൽ", "ന്റെ", "ുടെ", "ിനെ", "ിന്", "ിനു", "ക്ക്", "ാണ്", "ായി", "ിലെ", "ോട്", "ോടെ")
# Sources whose consecutive identical paragraphs are kept: Gutenberg writes dialogue three times.
REPEATS_KEPT = frozenset({"en-gutenberg"})
# Templated sentences (design §10): a training sentence of at least `TEMPLATE_MIN_WORDS` words
# is kept only while fewer than `TEMPLATE_CAP` sentences with its key (digits masked) have been,
# across all sources. Wikipedia's place articles repeat their census sentences thousands of times
# with only the numbers changed ("for every 100 females there were 0 males", 4,864 times), and
# the v1 model recited 53-60 tokens of them. How often a sequence is duplicated is what drives
# memorisation; three copies in about one epoch is a handful of sightings, and still teaches the
# model the wording, which one copy might not. Shorter sentences ("he retired in 0.", or "c." left by the
# sentence split) can't make up a long run, and capping them would cut ordinary text.
TEMPLATE_CAP, TEMPLATE_MIN_WORDS = 3, 6
# Runs of one unit (design §10): a unit of up to 8 characters repeated 4 or more times in a run
# longer than `MAX_RUN` characters ("1000000000000000", ". . . . . .", "൦൦൦൦൦൦") is cut to the
# whole units that fit in `MAX_RUN`, and to no fewer than 3. Digits and these marks are one
# token each, so no 16-token window of training text is one loop, and a model that loops
# can't match a long run of it. `MAX_RUN` keeps every number up to 10^12.
MAX_RUN = 12
_RUN = re.compile(r"(.{1,8}?)\1{3,}")

# (bucket, raw file, word cap)
ENGLISH = (
    ("en-wiki", "en.txt", EN_WIKI_WORDS),
    ("en-simple", "simplewiki.txt", None),
    ("en-news", "wikinews.txt", None),
    ("en-gutenberg", "gutenberg.txt", None),
)
TUNE_SOURCES_EN = frozenset({"en-simple", "en-news"})

_END = re.compile(r"(?<=[.?!।])\s+")
_NUMBER = re.compile(r"\d(?:[\d,.]*\d)?")
_PUNCT = re.compile(r"[.,!?;:\"()…।]")
_SPACES = re.compile(r"\s+")
_MALAYALAM = re.compile("[ഀ-ൿ‌‍]+")
_TUNE_EN = re.compile(r"[A-Za-z][A-Za-z' ,.?!-]*")
_TUNE_ML = re.compile("[ഀ-ൿ‌‍][ഀ-ൿ‌‍ ,.?!-]*")


def sentences(paragraph: str) -> list[str]:
    """Split after `.`, `?`, `!` and `।` followed by a space."""
    return [s for s in _END.split(paragraph.strip()) if s]


def _key(normalized: str) -> int:
    h = hashlib.blake2b(_NUMBER.sub("0", normalized).encode("utf-8"), digest_size=8)
    return int.from_bytes(h.digest(), "little")


def dedupe_key(text: str) -> int:
    """`lm_normalize`, every number replaced by `0`, then a 64-bit blake2b hash."""
    return _key(mlengine.lm_normalize(text))


def strip_punct(text: str) -> str:
    """The text with sentence punctuation dropped, as people often type it."""
    return _SPACES.sub(" ", _PUNCT.sub("", text)).strip()


def is_leak(sentence: str, leaks: set[int]) -> bool:
    """Whether the sentence, with or without its punctuation, is in `leaks`. A sentence of
    fewer than `MIN_LEAK_WORDS` words never is: "A." or "2013." tell the model nothing about
    an eval set."""
    bare = strip_punct(sentence)
    if len(bare.split()) < MIN_LEAK_WORDS:
        return False
    return dedupe_key(sentence) in leaks or dedupe_key(bare) in leaks


def _cut_run(m: re.Match) -> str:
    unit, run = m.group(1), m.group()
    if len(run) <= MAX_RUN:
        return run
    return unit * max(3, MAX_RUN // len(unit))


def cut_runs(text: str) -> str:
    """The text with every run of one unit longer than `MAX_RUN` characters cut short."""
    return _RUN.sub(_cut_run, text)


def keep_template(sentence: str, counts: dict[int, int]) -> bool:
    """Whether a training sentence (`lm_normalize`d) stays: fewer than `TEMPLATE_MIN_WORDS`
    words, or fewer than `TEMPLATE_CAP` kept before it with the same key. Counts what it keeps."""
    if len(sentence.split()) < TEMPLATE_MIN_WORDS:
        return True
    key = _key(sentence)
    n = counts.get(key, 0)
    if n >= TEMPLATE_CAP:
        return False
    counts[key] = n + 1
    return True


def _eval_strings(path: Path) -> Iterator[str]:
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        cols = line.split("\t")
        for c in cols:
            if c.strip():
                yield c
                yield from sentences(c)
        if len(cols) == 3 and cols[0].strip():  # context<TAB>typed<TAB>wanted
            yield f"{cols[0]} {cols[1]}"
            yield f"{cols[0]} {cols[2]}"


def leak_keys(eval_dir: Path = EVAL, dakshina: Path = DAKSHINA_ROMANIZED) -> set[int]:
    """Keys of every string in every eval set (each column, each context with its word, each
    sentence), and of every Dakshina romanised dev and test sentence, in both scripts, with and
    without punctuation. The tune splits this module writes come from held-out text already."""
    own = {"next-en.tune.txt", "next-ml.tune.txt", "next-ml.heldout.txt"}
    strings: list[str] = []
    for path in sorted(eval_dir.iterdir()):
        if path.suffix in (".txt", ".tsv") and path.name not in own:
            strings.extend(_eval_strings(path))
    for path in sorted(dakshina.glob("ml.romanized.rejoined.*.txt")):
        for line in path.read_text(encoding="utf-8").splitlines():
            if line.strip():
                strings.append(line)
                strings.extend(sentences(line))
    keys = set()
    for s in strings:
        if len(strip_punct(s).split()) < MIN_LEAK_WORDS:
            continue
        keys.add(dedupe_key(s))
        keys.add(dedupe_key(strip_punct(s)))
    return keys


def loanwords_from(
    rows: Iterable[dict],
    english: set[str] | dict[str, int],
    malayalam: set[str],
    borrowed: Iterable[str] = (),
    reverse: dict[str, list[str]] | None = None,
) -> dict[str, str]:
    """Malayalam spelling -> English word.

    A Malayalam word (in `malayalam`, not inflected: `LOANWORD_ENDINGS`) is a loanword if a
    mined Aksharantar row (CC0) gives it an English word (in `english`) as its Latin side, or if
    it is in `borrowed` (SMC's list of English borrowings). Its English is the most frequent
    (`english` as word -> count; first seen on ties) of those Latin sides and of the English
    words the transliterator turns into it (`reverse`, native -> English words). The mined
    Latin side alone is often a near miss: ബസ് has "buzz" and "buses", the transliterator's
    "bus" is what people mean.
    """
    count = english.get if isinstance(english, dict) else (lambda _w, _d=0: 0)
    candidates: dict[str, list[str]] = {}
    for row in rows:
        source = row.get("source")
        if source not in LOANWORD_SOURCES:
            continue
        score = row.get("score")
        if source == "IndicCorp" and score is not None and score < human.INDICCORP_MIN_SCORE:
            continue
        latin = str(row["english word"]).strip().lower()
        native = human.normalize(str(row["native word"]).strip())
        if latin in english and native in malayalam and not native.endswith(LOANWORD_ENDINGS):
            candidates.setdefault(native, []).append(latin)
    for native in borrowed:
        if native in malayalam and not native.endswith(LOANWORD_ENDINGS):
            candidates.setdefault(native, [])
    for native, words in (reverse or {}).items():
        if native in candidates:
            candidates[native].extend(w for w in words if w in english)
    out = {}
    for native, words in candidates.items():
        if words:
            out[native] = max(words, key=lambda w: (count(w, 0), -words.index(w)))
    return out


def transliterated(words: Iterable[str]) -> dict[str, list[str]]:
    """Malayalam word -> the English words whose first transliteration (the bundled M3a
    transliterator and Malayalam lexicons, no English lexicon) it is."""
    engine = mlengine.Engine(
        malayalam=str(LEXICON / "ml.fst"), manglish=str(LEXICON / "manglish.fst"), translit=str(TRANSLIT),
    )
    out: dict[str, list[str]] = {}
    for word in words:
        for text, kind, _ in engine.suggest(word, "latin", True):
            if kind == "manglish":
                out.setdefault(text, []).append(word)
                break
    return out


def load_loanwords() -> dict[str, str]:
    """`loanwords_from` the Aksharantar train split, SMC's borrowed words and the bundled
    lexicons and transliterator."""
    en = mlengine.malayalam_words(str(LEXICON / "en.fst"), 10**8)
    total = sum(c for _, c in en)
    english: dict[str, int] = {}
    for w, c in en:
        if math.log(c / total) > LOANWORD_MIN_LOG_PROB:
            english[w.lower()] = english.get(w.lower(), 0) + c
    malayalam = {w for w, _ in mlengine.malayalam_words(str(LEXICON / "ml.fst"), 10**8)}
    borrowed = [human.normalize(w.strip()) for w in SMC_BORROWED.read_text(encoding="utf-8").splitlines() if w.strip()]
    plain = sorted((w for w in english if w.isascii() and w.isalpha() and len(w) > 2), key=lambda w: -english[w])
    reverse = transliterated(plain[:TRANSLITERATED_WORDS])
    return loanwords_from(human.aksharantar_rows(AKSHARANTAR_TRAIN), english, malayalam, borrowed, reverse)


@lru_cache(maxsize=1)
def _rules():
    return calibrate.load()


@lru_cache(maxsize=1 << 21)
def _word_slots(word: str):
    return _slots(word, _rules())


def romanise_word(word: str, rng: random.Random, loanwords: dict[str, str], p_loan: float) -> str:
    """The loanword's English with probability `p_loan`, else one of the word's spellings as
    people type them (the calibrated M3a romaniser)."""
    if word in loanwords and rng.random() < p_loan:
        return loanwords[word]
    spellings = variants_of(_word_slots(word), rng, _rules())
    return rng.choice(spellings) if spellings else ""


def _tidy(text: str) -> str:
    return _SPACES.sub(" ", text).strip()


def romanise_sentence(sentence: str, rng: random.Random, loanwords: dict[str, str], p_loan: float) -> str:
    """Every Malayalam word romanised (`romanise_word`); other tokens and punctuation kept."""
    return _tidy(_MALAYALAM.sub(lambda m: romanise_word(m.group(), rng, loanwords, p_loan), sentence))


def mix_sentence(sentence: str, rng: random.Random, loanwords: dict[str, str], frac: float, p_loan: float) -> str:
    """A loanword becomes English with probability `p_loan`; any other Malayalam word is
    romanised with probability `frac`, else kept in script."""

    def one(m: re.Match) -> str:
        word = m.group()
        if word in loanwords and rng.random() < p_loan:
            return loanwords[word]
        if rng.random() < frac:
            return romanise_word(word, rng, {}, 0.0)
        return word

    return _tidy(_MALAYALAM.sub(one, sentence))


def _chat(text: str, rng: random.Random) -> str:
    return strip_punct(text) if rng.random() < NO_PUNCT_SHARE else text


def _is_valid(normalized: str) -> bool:
    h = hashlib.blake2b(normalized[:64].encode("utf-8"), digest_size=8).digest()
    return int.from_bytes(h, "little") % VALID_EVERY == 0


class Reservoir:
    """A uniform sample of `size` items from a stream, seeded."""

    def __init__(self, size: int, rng: random.Random):
        self.size, self.rng, self.seen, self.items = size, rng, 0, []

    def add(self, item: str) -> None:
        self.seen += 1
        if len(self.items) < self.size:
            self.items.append(item)
        elif (j := self.rng.randrange(self.seen)) < self.size:
            self.items[j] = item


def _lines(path: Path) -> Iterator[str]:
    with open(path, encoding="utf-8") as f:
        for line in f:
            if line.strip():
                yield line.rstrip("\n")


def _filter_training(norm: str, templates: dict[int, int], stats: dict) -> str:
    """`cut_runs`, then `keep_template` on each sentence; counts what goes into `stats`."""
    cut = cut_runs(norm)
    if len(cut) < len(norm):
        stats["runs_cut"] += sum(len(m.group()) > MAX_RUN for m in _RUN.finditer(norm))
        stats["run_chars"] += len(norm) - len(cut)
    parts = sentences(cut)
    kept = [s for s in parts if keep_template(s, templates)]
    if len(kept) < len(parts):
        stats["template_sentences"] += len(parts) - len(kept)
        stats["template_words"] += sum(len(s.split()) for s in parts) - sum(len(s.split()) for s in kept)
        return " ".join(kept)
    return cut


def clean_source(
    bucket: str, path: Path, cap: int | None, seen: set[int], leaks: set[int], rng: random.Random,
    train_out, valid_out, on_valid: Callable[[str, str], None] | None = None,
    templates: dict[int, int] | None = None,
) -> dict:
    """Dedupes, cuts leaks, splits off validation and applies the chat transform for one source.
    Training paragraphs then lose long runs (`cut_runs`) and templated sentences past the cap
    (`keep_template`, counted in `templates`, shared across sources); held-out ones stay as they
    were, so validation losses compare across corpus versions. `train_out` / `valid_out` get the
    cleaned paragraph (with its punctuation) and the bucket's transformed text. Returns the
    bucket's counts."""
    stats = {"paragraphs": 0, "words": 0, "valid_paragraphs": 0, "valid_words": 0,
             "duplicates": 0, "leaks_removed": 0, "raw_words": 0, "runs_cut": 0, "run_chars": 0,
             "template_sentences": 0, "template_words": 0, "template_paragraphs": 0}
    templates = {} if templates is None else templates
    previous = None
    repeat: tuple[int, str] | None = None  # the last training paragraph's key and filtered text
    for raw in _lines(path):
        if cap is not None and stats["raw_words"] >= cap:
            break
        stats["raw_words"] += len(raw.split())
        if _MARKUP.search(raw):
            stats["markup"] = stats.get("markup", 0) + 1
            continue
        norm = mlengine.lm_normalize(raw)
        key = _key(norm)
        if key in seen and not (bucket in REPEATS_KEPT and key == previous):
            stats["duplicates"] += 1
            continue
        seen.add(key)
        previous = key
        parts = sentences(norm)
        kept = [s for s in parts if not is_leak(s, leaks)]
        if len(kept) < len(parts):
            stats["leaks_removed"] += len(parts) - len(kept)
            if not kept:
                continue
            norm = " ".join(kept)
        valid = _is_valid(norm)
        if not valid:
            if repeat is not None and repeat[0] == key:  # a kept repeat (`REPEATS_KEPT`) stays a copy
                norm = repeat[1]
            else:
                norm = _filter_training(norm, templates, stats)
                repeat = (key, norm)
            if not norm:
                stats["template_paragraphs"] += 1
                rng.random()  # the draw `_chat` would have made, so the held-out samples don't move
                continue
        (valid_out if valid else train_out)(norm, _chat(norm, rng))
        prefix = "valid_" if valid else ""
        stats[prefix + "paragraphs"] += 1
        stats[prefix + "words"] += len(norm.split())
        if valid and on_valid:
            on_valid(raw, norm)
    return stats


class _Writer:
    """Writes the transformed text to `<bucket>.txt` and, optionally, the clean text to a work file."""

    def __init__(self, text_path: Path, clean_path: Path | None = None):
        self.text = open(text_path.with_name(text_path.name + ".tmp"), "w", encoding="utf-8")
        self.clean = open(clean_path, "w", encoding="utf-8") if clean_path else None
        self.path = text_path

    def __call__(self, clean: str, text: str) -> None:
        self.text.write(text + "\n")
        if self.clean:
            self.clean.write(clean + "\n")

    def close(self) -> None:
        self.text.close()
        Path(self.text.name).replace(self.path)
        if self.clean:
            self.clean.close()


_LOANWORDS: dict[str, str] = {}


def _init_worker(loanwords: dict[str, str]) -> None:
    global _LOANWORDS
    _LOANWORDS = loanwords


def synthesise(job: tuple[str, str, str, int]) -> tuple[str, dict]:
    """One synthetic file from the clean Malayalam text: (kind, source, destination, seed)."""
    kind, src, dst, seed = job
    rng = random.Random(seed)
    tmp = Path(dst + ".tmp")
    stats = {"paragraphs": 0, "words": 0, "skipped_not_malayalam": 0}
    with open(tmp, "w", encoding="utf-8") as out:
        for para in _lines(Path(src)):
            words = para.split()
            if sum(bool(_MALAYALAM.search(w)) for w in words) < SYNTHETIC_MIN_MALAYALAM * len(words):
                stats["skipped_not_malayalam"] += 1
                continue
            if kind == "manglish":
                text = romanise_sentence(para, rng, _LOANWORDS, MANGLISH_P_LOAN)
            else:
                text = " ".join(
                    mix_sentence(s, rng, _LOANWORDS, rng.uniform(*MIXED_FRAC), MIXED_P_LOAN) for s in sentences(para)
                )
            text = _chat(text, rng)
            if text:
                out.write(text + "\n")
                stats["paragraphs"] += 1
                stats["words"] += len(text.split())
    tmp.replace(dst)
    return Path(dst).name, stats


def is_tune_half(sentence: str) -> bool:
    """Which half of the held-out Malayalam sentences `sentence` belongs to: tune, else gate."""
    return hashlib.blake2b(sentence.encode("utf-8"), digest_size=8).digest()[0] % 2 == 0


def sentence_keys(paragraphs: Iterable[str]) -> set[int]:
    """Keys of every sentence of some `lm_normalize`d paragraphs, with and without punctuation."""
    keys = set()
    for para in paragraphs:
        for s in sentences(para):
            keys.add(_key(s))
            keys.add(_key(strip_punct(s)))
    return keys


def not_in(candidates: list[str], keys: set[int]) -> list[str]:
    """The candidates whose sentence, with or without punctuation, has no key in `keys`."""
    return [s for s in candidates if dedupe_key(s) not in keys and dedupe_key(strip_punct(s)) not in keys]


def _tune_candidates(raw: str, pattern: re.Pattern, leaks: set[int]) -> Iterator[str]:
    for s in sentences(raw):
        n = len(s.split())
        if TUNE_WORDS[0] <= n <= TUNE_WORDS[1] and pattern.fullmatch(s) and not is_leak(s, leaks):
            yield s


TUNE_EN_HEADER = """# Next-word eval set, English (tune split, for weight sweeps only): one sentence per line.
# {n} sentences of 4-20 words sampled (seed {seed}) from held-out paragraphs, never trained on, of
# Simple English Wikipedia (CC BY-SA 4.0, by Wikipedia contributors, https://simple.wikipedia.org/) and
# English Wikinews (CC BY 2.5 and CC BY 4.0, by Wikinews contributors; this work may be attributed to
# 'Wikinews', https://en.wikinews.org/). Built by training/mlkb/lm/corpus.py.
"""
TUNE_ML_HEADER = """# Next-word eval set, Malayalam (tune split, for weight sweeps only): one sentence per line.
# {n} sentences of 4-20 words sampled (seed {seed}) from held-out paragraphs, never trained on, of
# Malayalam Wikipedia and Malayalam Wikisource (CC BY-SA 4.0, by their contributors,
# https://ml.wikipedia.org/, https://ml.wikisource.org/). Built by training/mlkb/lm/corpus.py.
"""
GATE_ML_HEADER = """# Next-word eval set, Malayalam (gate split, held-out half; read with next-ml.txt): one sentence per line.
# {n} sentences of 4-20 words sampled (seed {seed}) from held-out paragraphs, never trained on, of
# Malayalam Wikipedia and Malayalam Wikisource (CC BY-SA 4.0, by their contributors,
# https://ml.wikipedia.org/, https://ml.wikisource.org/), disjoint from the tune split, and none of
# them a sentence of the training text. Built by training/mlkb/lm/corpus.py.
"""


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", type=Path, default=Path("build/lm/text"))
    parser.add_argument("--seed", type=int, default=SEED)
    parser.add_argument("--workers", type=int, default=8)
    args = parser.parse_args(argv)
    out: Path = args.out
    work = out.parent / "work"
    out.mkdir(parents=True, exist_ok=True)
    work.mkdir(parents=True, exist_ok=True)
    start = time.time()

    leaks = leak_keys()
    print(f"{len(leaks):,} leak keys", file=sys.stderr)
    loanwords = load_loanwords()
    print(f"{len(loanwords):,} loanwords", file=sys.stderr)
    summary: dict[str, dict] = {}
    seen: set[int] = set()
    templates: dict[int, int] = {}
    # memorise.py's record of where Wikisource begins in ml.txt: the paragraph count moves.
    (out / "sources.tsv").unlink(missing_ok=True)

    # Malayalam first: the synthetic buckets are built from its clean text while English runs.
    rng = random.Random(args.seed)
    heldout_ml = Reservoir(HELDOUT_SENTENCES, rng)
    tune_ml = Reservoir(TUNE_SENTENCES_ML, random.Random(args.seed + 1))
    gate_ml = Reservoir(GATE_SENTENCES_ML, random.Random(args.seed + 3))

    def on_valid_ml(raw: str, norm: str) -> None:
        for s in sentences(norm):
            heldout_ml.add(s)
        for s in _tune_candidates(human.normalize(raw), _TUNE_ML, leaks):
            (tune_ml if is_tune_half(s) else gate_ml).add(s)

    train, valid = _Writer(out / "ml.txt", work / "ml-clean.txt"), _Writer(out / "valid-ml.txt", work / "valid-ml-clean.txt")
    summary["ml"] = clean_source("ml", RAW_TEXT / "ml.txt", None, seen, leaks, rng, train, valid, on_valid_ml,
                                 templates)
    train.close()
    valid.close()
    print(f"ml: {summary['ml']}  ({time.time() - start:.0f}s)", file=sys.stderr)
    # A held-out paragraph can share a sentence with a training one: those sentences go.
    with open(work / "ml-clean.txt", encoding="utf-8") as f:
        trained = sentence_keys(line.rstrip("\n") for line in f)
    for r in (tune_ml, gate_ml):
        before = len(r.items)
        r.items = not_in(r.items, trained)
        summary.setdefault("next_ml_sets", {})[f"in_training_{'tune' if r is tune_ml else 'gate'}"] = before - len(r.items)
    del trained

    jobs = [("manglish", str(work / "ml-clean.txt"), str(out / f"manglish-{i}.txt"), args.seed * 10 + i)
            for i in range(MANGLISH_PASSES)]
    jobs += [("mixed", str(work / "ml-clean.txt"), str(out / f"mixed-{i}.txt"), args.seed * 10 + 5 + i)
             for i in range(MIXED_PASSES)]
    jobs += [("manglish", str(work / "valid-ml-clean.txt"), str(out / "valid-manglish.txt"), args.seed * 10 + 8),
             ("mixed", str(work / "valid-ml-clean.txt"), str(out / "valid-mixed.txt"), args.seed * 10 + 9)]
    with Pool(args.workers, initializer=_init_worker, initargs=(loanwords,)) as pool:
        pending = pool.map_async(synthesise, jobs)

        heldout_en, tune_en = Reservoir(HELDOUT_SENTENCES, rng), Reservoir(TUNE_SENTENCES, random.Random(args.seed + 2))
        for bucket, name, cap in ENGLISH:

            def on_valid_en(raw: str, norm: str, bucket: str = bucket) -> None:
                for s in sentences(norm):
                    heldout_en.add(s)
                if bucket in TUNE_SOURCES_EN:
                    for s in _tune_candidates(raw, _TUNE_EN, leaks):
                        tune_en.add(s)

            train, valid = _Writer(out / f"{bucket}.txt"), _Writer(out / f"valid-{bucket}.txt")
            summary[bucket] = clean_source(bucket, RAW_TEXT / name, cap, seen, leaks, rng, train, valid,
                                           on_valid_en, templates)
            train.close()
            valid.close()
            print(f"{bucket}: {summary[bucket]}  ({time.time() - start:.0f}s)", file=sys.stderr)

        for name, stats in pending.get():
            summary[name.removesuffix(".txt")] = stats
            print(f"{name}: {stats}", file=sys.stderr)

    (out.parent / "heldout-en.txt").write_text("\n".join(heldout_en.items) + "\n", encoding="utf-8")
    (out.parent / "heldout-ml.txt").write_text("\n".join(heldout_ml.items) + "\n", encoding="utf-8")
    (EVAL / "next-en.tune.txt").write_text(
        TUNE_EN_HEADER.format(n=len(tune_en.items), seed=args.seed + 2) + "\n".join(tune_en.items) + "\n", encoding="utf-8")
    (EVAL / "next-ml.tune.txt").write_text(
        TUNE_ML_HEADER.format(n=len(tune_ml.items), seed=args.seed + 1) + "\n".join(tune_ml.items) + "\n", encoding="utf-8")
    (EVAL / "next-ml.heldout.txt").write_text(
        GATE_ML_HEADER.format(n=len(gate_ml.items), seed=args.seed + 3) + "\n".join(gate_ml.items) + "\n", encoding="utf-8")
    summary["loanwords"] = {"count": len(loanwords)}
    (out / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    print(f"done in {time.time() - start:.0f}s; summary in {out / 'summary.json'}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
