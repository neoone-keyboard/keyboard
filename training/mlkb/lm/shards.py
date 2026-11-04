"""Token shards: every corpus file tokenised by the production Rust tokeniser, as `uint16`.

    uv run --group train python -m mlkb.lm.shards --text build/lm/text --vocab build/lm/pieces.tsv \\
        --out build/lm/shards

Each `<name>.txt` becomes `<name>.bin`: for each paragraph, `BOS` then its tokens, little-endian
`uint16` (16,384 ids fit), for the loader to memory-map. Training files (not `valid-*`) then lose
every token of a loop past `MAX_LOOP` (`cut_loops`). Prints tokens per file and tokens per word
per bucket, and writes them, with the tokens cut, to `summary.json`.
"""
import argparse
import json
import re
import sys
from multiprocessing import Pool
from pathlib import Path

import mlengine
import numpy as np

_TOKENIZER = None
# Loops (design §10): a run of tokens repeating with a period of up to `LOOP_PERIODS` keeps only
# the whole periods that fit in `MAX_LOOP` tokens (one period at least). `corpus.cut_runs` does
# most of this on the text, but the chat transform and romanising can leave new runs (`, , , ,`,
# `-m -m -m`), and a character with no piece of its own is 3 byte tokens. The memorisation check
# looks generated 16-token windows up in the training text, and every window of a one-token loop
# is the same window, so one 16-token loop anywhere let a model stuck on `0000…` match 64 tokens.
# Under 16 tokens per loop, no window of a loop is left.
LOOP_PERIODS, MAX_LOOP = 8, 12


def cut_loops(tokens: np.ndarray) -> tuple[np.ndarray, int]:
    """The tokens with every loop of period up to `LOOP_PERIODS` cut to its first whole periods
    within `MAX_LOOP` tokens, and how many tokens went. Shortest periods first, repeated until
    nothing changes (a cut can join two runs)."""
    n = len(tokens)
    while True:
        before = len(tokens)
        for p in range(1, LOOP_PERIODS + 1):
            eq = np.concatenate(([False], tokens[p:] == tokens[:-p], [False]))
            d = np.diff(eq.astype(np.int8))
            starts = np.flatnonzero(d == 1)
            spans = np.flatnonzero(d == -1) - starts + p
            long = spans > MAX_LOOP
            if not long.any():
                continue
            keep = np.ones(len(tokens), bool)
            k = max(1, MAX_LOOP // p) * p
            for s, span in zip(starts[long], spans[long]):
                keep[s + k:s + span] = False
            tokens = tokens[keep]
        if len(tokens) == before:
            return tokens, n - len(tokens)


def read_pieces(path: Path) -> tuple[list[str], list[float]]:
    pieces, scores = [], []
    for line in path.read_text(encoding="utf-8").splitlines():
        piece, score = line.rsplit("\t", 1)
        pieces.append(piece)
        scores.append(float(score))
    return pieces, scores


def _init(vocab: str) -> None:
    global _TOKENIZER
    _TOKENIZER = mlengine.Tokenizer(*read_pieces(Path(vocab)))


def _encode(job: tuple[str, str]) -> tuple[str, int, int]:
    src, dst = job
    tokens = _TOKENIZER.encode_file(src, dst)
    cut = 0
    if not Path(src).name.startswith("valid-"):
        cut_tokens, cut = cut_loops(np.fromfile(dst, np.uint16))
        if cut:
            cut_tokens.astype("<u2").tofile(dst)
            tokens -= cut
    words = 0
    with open(src, encoding="utf-8") as f:
        for line in f:
            words += len(line.split())
    return Path(src).name, tokens, words, cut


def bucket(name: str) -> str:
    """`manglish-2.txt` -> `manglish`, `valid-ml.txt` -> `valid-ml`."""
    return re.sub(r"-\d+$", "", name.removesuffix(".txt"))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--text", type=Path, default=Path("build/lm/text"))
    parser.add_argument("--vocab", type=Path, default=Path("build/lm/pieces.tsv"))
    parser.add_argument("--out", type=Path, default=Path("build/lm/shards"))
    parser.add_argument("--workers", type=int, default=8)
    args = parser.parse_args(argv)
    args.out.mkdir(parents=True, exist_ok=True)
    files = sorted(args.text.glob("*.txt"), key=lambda p: -p.stat().st_size)
    jobs = [(str(p), str(args.out / (p.stem + ".bin"))) for p in files]
    with Pool(args.workers, initializer=_init, initargs=(str(args.vocab),)) as pool:
        results = sorted(pool.map(_encode, jobs))
    per_file = {name: {"tokens": t, "words": w, "loop_tokens_cut": c} for name, t, w, c in results}
    per_bucket: dict[str, dict] = {}
    for name, t, w, c in results:
        b = per_bucket.setdefault(bucket(name), {"tokens": 0, "words": 0, "loop_tokens_cut": 0})
        b["tokens"] += t
        b["words"] += w
        b["loop_tokens_cut"] += c
    for b in per_bucket.values():
        b["tokens_per_word"] = round(b["tokens"] / max(b["words"], 1), 3)
    for name, row in per_file.items():
        print(f"{name:28} {row['tokens']:>14,} tokens  {row['words']:>13,} words  "
              f"{row['loop_tokens_cut']:>9,} cut from loops", file=sys.stderr)
    for name, row in per_bucket.items():
        print(f"{name:28} {row['tokens']:>14,} tokens  {row['tokens_per_word']:.3f} tokens/word", file=sys.stderr)
    (args.out / "summary.json").write_text(json.dumps({"files": per_file, "buckets": per_bucket}, indent=2) + "\n",
                                           encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main())
