"""Token shards: every corpus file tokenised by the production Rust tokeniser, as `uint16`.

    uv run --group train python -m mlkb.lm.shards --text build/lm/text --vocab build/lm/pieces.tsv \\
        --out build/lm/shards

Each `<name>.txt` becomes `<name>.bin`: for each paragraph, `BOS` then its tokens, little-endian
`uint16` (16,384 ids fit), for the loader to memory-map. Prints tokens per file and tokens per
word per bucket, and writes them to `summary.json`.
"""
import argparse
import json
import re
import sys
from multiprocessing import Pool
from pathlib import Path

import mlengine

_TOKENIZER = None


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
    words = 0
    with open(src, encoding="utf-8") as f:
        for line in f:
            words += len(line.split())
    return Path(src).name, tokens, words


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
    per_file = {name: {"tokens": t, "words": w} for name, t, w in results}
    per_bucket: dict[str, dict] = {}
    for name, t, w in results:
        b = per_bucket.setdefault(bucket(name), {"tokens": 0, "words": 0})
        b["tokens"] += t
        b["words"] += w
    for b in per_bucket.values():
        b["tokens_per_word"] = round(b["tokens"] / max(b["words"], 1), 3)
    for name, row in per_file.items():
        print(f"{name:28} {row['tokens']:>14,} tokens  {row['words']:>13,} words", file=sys.stderr)
    for name, row in per_bucket.items():
        print(f"{name:28} {row['tokens']:>14,} tokens  {row['tokens_per_word']:.3f} tokens/word", file=sys.stderr)
    (args.out / "summary.json").write_text(json.dumps({"files": per_file, "buckets": per_bucket}, indent=2) + "\n",
                                           encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main())
