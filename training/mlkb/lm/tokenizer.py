"""The LM vocabulary: a 16,384-piece SentencePiece unigram model over the corpus (design §3).

    uv run --group train python -m mlkb.lm.tokenizer --text build/lm/text --out build/lm
    uv run --group train python -m mlkb.lm.tokenizer --lang hi --text build/lm-hi/text --out build/lm-hi

Samples `SAMPLE_SENTENCES` sentences (40% English by bucket weight, 35% Malayalam script, 25%
Latin Manglish; for `--lang hi`, Hindi script and Hinglish), trains SentencePiece (a training-time tool only, never shipped), and writes
`pieces.tsv` (`piece<TAB>score` in id order), which the Rust tokeniser and the export read.
Then checks that the Rust tokeniser (`mlengine.Tokenizer`) encodes `CHECK_LINES` validation
lines exactly as `sentencepiece` does; exits 1 if fewer than `MIN_AGREEMENT` agree.
"""
import argparse
import random
import sys
import time
from pathlib import Path

import mlengine
import sentencepiece as spm

from mlkb.lm import lang as lm_lang
from mlkb.lm.corpus import SEED, sentences

VOCAB = 16384
SAMPLE_SENTENCES = 10_000_000
# Bucket weights from the design's mix (tokens seen, millions); English shares 40% by these.
ENGLISH_WEIGHTS = {"en-wiki.txt": 300, "en-simple.txt": 100, "en-news.txt": 40, "en-gutenberg.txt": 110}
SHARES = {"english": 0.40, "ml.txt": 0.35, "manglish-0.txt": 0.25}
CHECK_LINES = 10_000
MIN_AGREEMENT = 0.999
VALID_FILES = ("valid-en-wiki.txt", "valid-en-simple.txt", "valid-en-news.txt", "valid-en-gutenberg.txt",
               "valid-ml.txt", "valid-manglish.txt", "valid-mixed.txt")


def valid_files(lang: lm_lang.LmLang = lm_lang.ML) -> tuple[str, ...]:
    """`VALID_FILES` with the language's native and romanised buckets."""
    names = {"valid-ml.txt": f"valid-{lang.native}.txt", "valid-manglish.txt": f"valid-{lang.romanised}.txt"}
    return tuple(names.get(n, n) for n in VALID_FILES)


def targets(lang: lm_lang.LmLang = lm_lang.ML) -> dict[str, int]:
    """Sentences wanted from each file (`SHARES`, with the language's file names)."""
    out = {f"{lang.native}.txt": int(SAMPLE_SENTENCES * SHARES["ml.txt"]),
           f"{lang.romanised}-0.txt": int(SAMPLE_SENTENCES * SHARES["manglish-0.txt"])}
    total = sum(ENGLISH_WEIGHTS.values())
    for name, w in ENGLISH_WEIGHTS.items():
        out[name] = int(SAMPLE_SENTENCES * SHARES["english"] * w / total)
    return out


def _sentence_count(path: Path, probe: int = 5000) -> int:
    """Lines in the file times the mean sentences per line of its first `probe` lines."""
    lines = 0
    with open(path, "rb") as f:
        while chunk := f.read(1 << 24):
            lines += chunk.count(b"\n")
    with open(path, encoding="utf-8") as f:
        head = [len(sentences(line)) for _, line in zip(range(probe), f)]
    return int(lines * (sum(head) / max(len(head), 1)))


def sample(text: Path, dst: Path, rng: random.Random, lang: lm_lang.LmLang = lm_lang.ML) -> dict[str, int]:
    """Writes the sentence sample to `dst`; returns sentences taken per file."""
    taken = {}
    with open(dst, "w", encoding="utf-8") as out:
        for name, want in targets(lang).items():
            path = text / name
            p = min(1.0, want / max(_sentence_count(path), 1))
            n = 0
            with open(path, encoding="utf-8") as f:
                for line in f:
                    if n >= want:
                        break
                    if rng.random() < p:
                        for s in sentences(line):
                            out.write(s + "\n")
                            n += 1
            taken[name] = n
            print(f"sample: {name}: {n:,} sentences (wanted {want:,})", file=sys.stderr)
    return taken


def train(sample_path: Path, prefix: Path, sentences_in: int) -> None:
    spm.SentencePieceTrainer.train(
        input=str(sample_path), model_prefix=str(prefix), model_type="unigram", vocab_size=VOCAB,
        pad_id=0, unk_id=1, bos_id=2, eos_id=3, byte_fallback=True, split_digits=True,
        normalization_rule_name="identity", character_coverage=0.9999,
        input_sentence_size=min(SAMPLE_SENTENCES, sentences_in), shuffle_input_sentence=True, num_threads=16,
        minloglevel=1,
    )


def write_pieces(model: Path, dst: Path) -> tuple[list[str], list[float]]:
    sp = spm.SentencePieceProcessor(model_file=str(model))
    pieces = [sp.id_to_piece(i) for i in range(sp.get_piece_size())]
    scores = [sp.get_score(i) for i in range(sp.get_piece_size())]
    dst.write_text("".join(f"{p}\t{s}\n" for p, s in zip(pieces, scores)), encoding="utf-8")
    return pieces, scores


def agreement(model: Path, pieces: list[str], scores: list[float], text: Path, rng: random.Random,
              lang: lm_lang.LmLang = lm_lang.ML) -> float:
    """Share of validation lines that the Rust tokeniser encodes exactly as sentencepiece does."""
    sp = spm.SentencePieceProcessor(model_file=str(model))
    ours = mlengine.Tokenizer(pieces, scores)
    lines = []
    for name in valid_files(lang):
        with open(text / name, encoding="utf-8") as f:
            lines += [line.strip() for line in f if line.strip()]
    lines = rng.sample(lines, min(CHECK_LINES, len(lines)))
    same = 0
    shown = 0
    for line in lines:
        a, b = sp.encode(line, out_type=int), ours.encode(line)
        if a == b:
            same += 1
        elif shown < 5:
            shown += 1
            print(f"differs: {line[:80]!r}\n  spm  {[pieces[i] for i in a][:20]}\n  rust {[pieces[i] for i in b][:20]}",
                  file=sys.stderr)
    return same / len(lines)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--lang", default=lm_lang.DEFAULT, choices=sorted(lm_lang.LANGS))
    parser.add_argument("--text", type=Path, default=None, help="default: <the language's build>/text")
    parser.add_argument("--out", type=Path, default=None, help="default: the language's build directory")
    parser.add_argument("--seed", type=int, default=SEED)
    args = parser.parse_args(argv)
    lang = lm_lang.load(args.lang)
    args.out = args.out or Path(lang.build)
    args.text = args.text or args.out / "text"
    rng = random.Random(args.seed)
    start = time.time()
    sample_path = args.out / "spm-sample.txt"
    taken = sample(args.text, sample_path, rng, lang)
    prefix = args.out / "spm"
    train(sample_path, prefix, sum(taken.values()))
    print(f"trained in {time.time() - start:.0f}s", file=sys.stderr)
    pieces, scores = write_pieces(prefix.with_suffix(".model"), args.out / "pieces.tsv")
    share = agreement(prefix.with_suffix(".model"), pieces, scores, args.text, rng, lang)
    print(f"{len(pieces)} pieces; Rust and sentencepiece agree on {share:.4%} of {CHECK_LINES:,} lines",
          file=sys.stderr)
    return 0 if share >= MIN_AGREEMENT else 1


if __name__ == "__main__":
    sys.exit(main())
