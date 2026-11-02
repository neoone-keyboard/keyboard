"""Builds the synthetic transliteration pairs: `latin<TAB>malayalam` rows for training.

    uv run python -m mlkb.data.pairs [--out training/build/pairs] [--rows 3000000]

Reads the most frequent Malayalam forms from `ml.fst`, samples each in proportion to
`count ** 0.5` (flattened, so rare forms appear too), and writes the spellings that
`romanise.variants` gives each draw. 1% of the words (not rows) go to `valid.tsv`, so no
word is in both files. Every word that is a target in an eval set is left out.
"""

import argparse
import random
import time
from collections.abc import Iterable
from pathlib import Path

import mlengine
import numpy as np

from mlkb.data.romanise import RULES, Rules, canonical, variants

ROOT = Path(__file__).resolve().parents[3]
LEXICON = ROOT / "apps/ios/KeyboardExtension/Lexicon/ml.fst"
EVAL_SETS = [ROOT / "data/eval/chat-golden.tsv", ROOT / "data/eval/translit-human.tsv"]
OUT = ROOT / "training/build/pairs"

# How strongly frequency drives sampling: each word is drawn in proportion to count ** this.
FREQUENCY_POWER = 0.5

Pair = tuple[str, str]


def excluded_words(paths: Iterable[Path]) -> set[str]:
    """Every Malayalam target (and each word of a multi-word target) in the eval files that exist."""
    out: set[str] = set()
    for path in paths:
        if not path.exists():
            continue
        for line in path.read_text(encoding="utf-8").splitlines():
            if not line.strip() or line.startswith("#"):
                continue
            target = line.split("\t")[-1].strip()
            out.add(target)
            out.update(target.split())
    return out


def build(
    words: list[tuple[str, int]],
    rows: int,
    valid_frac: float,
    excluded: set[str],
    seed: int,
    rules: Rules = RULES,
) -> tuple[list[Pair], list[Pair]]:
    """About `rows` (latin, malayalam) pairs split into train and valid by word, both shuffled."""
    words = [(w, c) for w, c in words if w not in excluded and canonical(w)]
    rng = random.Random(seed)
    is_valid = [rng.random() < valid_frac for _ in words]
    weights = np.array([c for _, c in words], dtype=np.float64) ** FREQUENCY_POWER
    weights /= weights.sum()
    np_rng = np.random.default_rng(seed)

    # Spellings per draw, measured on a pilot sample, sets how many draws give `rows`.
    pilot = np_rng.choice(len(words), size=min(2000, rows), p=weights)
    per_draw = np.mean([len(variants(words[k][0], rng, rules)) for k in pilot])
    draws = np_rng.multinomial(round(rows / per_draw), weights)

    train: list[Pair] = []
    valid: list[Pair] = []
    for (word, _), k, to_valid in zip(words, draws.tolist(), is_valid, strict=True):
        out = valid if to_valid else train
        for _ in range(k):
            out.extend((latin, word) for latin in variants(word, rng, rules))
    rng.shuffle(train)
    rng.shuffle(valid)
    return train, valid


def write(path: Path, pairs: list[Pair]) -> None:
    """Writes `latin<TAB>malayalam` lines."""
    with path.open("w", encoding="utf-8") as f:
        f.writelines(f"{latin}\t{word}\n" for latin, word in pairs)


def main(argv: list[str] | None = None) -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--out", type=Path, default=OUT)
    p.add_argument("--rows", type=int, default=3_000_000)
    p.add_argument("--words", type=int, default=300_000)
    p.add_argument("--valid-frac", type=float, default=0.01)
    p.add_argument("--seed", type=int, default=0)
    p.add_argument("--lexicon", type=Path, default=LEXICON)
    args = p.parse_args(argv)

    start = time.monotonic()
    words = mlengine.malayalam_words(str(args.lexicon), args.words)
    excluded = excluded_words(EVAL_SETS)
    left_out = sum(w in excluded for w, _ in words)
    train, valid = build(words, args.rows, args.valid_frac, excluded, args.seed)
    args.out.mkdir(parents=True, exist_ok=True)
    write(args.out / "train.tsv", train)
    write(args.out / "valid.tsv", valid)

    for name, pairs in (("train", train), ("valid", valid)):
        lengths = [len(latin) for latin, _ in pairs]
        print(
            f"{name}: {len(pairs):,} rows, {len({w for _, w in pairs}):,} words, "
            f"{len(set(pairs)):,} distinct pairs, latin length mean {np.mean(lengths):.1f} "
            f"max {max(lengths)}"
        )
    print(f"lexicon words read {len(words):,}; eval targets left out {left_out}")
    print(f"wrote {args.out} in {time.monotonic() - start:.0f}s")


if __name__ == "__main__":
    main()
