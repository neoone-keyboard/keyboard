"""Builds the transliteration pairs: `latin<TAB>malayalam` rows for training.

    uv run python -m mlkb.data.pairs [--out training/build/pairs] [--rows 3000000]

Synthetic rows: reads the most frequent Malayalam forms from `ml.fst`, samples each in
proportion to `count ** 0.5` (flattened, so rare forms appear too), and writes the spellings
that `romanise.variants` gives each draw. Each draw uses the rules calibrated on Dakshina
(`rules-calibrated.json`, careful typing) or, with probability `--chat-share`, the hand-set
chat rules (`romanise.RULES`, tuned on chat-golden), since the two registers differ.

Human rows (`mlkb.data.human`) make up `--human-share` of `train.tsv`: Dakshina train pairs
(weighted by annotator count) for `--dakshina-share` of them, filtered Aksharantar for the rest.

1% of the lexicon words (not rows) go to `valid.tsv`, so no word is in both files; the
Dakshina dev pairs go to `valid-human.tsv`. Every eval target (chat-golden, translit-human)
and every Dakshina dev/test word is left out of `train.tsv`.
"""

import argparse
import random
import time
from collections import Counter
from collections.abc import Iterable
from pathlib import Path

import mlengine
import numpy as np

from mlkb.data import calibrate, human
from mlkb.data.romanise import RULES, Rules, canonical, variants

ROOT = Path(__file__).resolve().parents[3]
LEXICON = ROOT / "packs/ml.neopack/ml.fst"
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
    alt_rules: Rules | None = None,
    alt_share: float = 0.0,
) -> tuple[list[Pair], list[Pair]]:
    """About `rows` (latin, malayalam) pairs split into train and valid by word, both shuffled.

    Each draw uses `rules`, or `alt_rules` with probability `alt_share`.
    """
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
            r = alt_rules if alt_rules is not None and rng.random() < alt_share else rules
            out.extend((latin, word) for latin in variants(word, rng, r))
    rng.shuffle(train)
    rng.shuffle(valid)
    return train, valid


def human_rows(
    n: int, dakshina_share: float, excluded: set[str], seed: int, stats: dict[str, int] | None = None
) -> list[Pair]:
    """`n` human training pairs: Dakshina train (by annotator count) and filtered Aksharantar."""
    stats = stats if stats is not None else {}
    rng = random.Random(seed + 1)
    dak = [h for h in human.dakshina("train") if h.malayalam not in excluded]
    aks_stats: Counter = Counter()
    aks = [(h.latin, h.malayalam) for h in human.aksharantar("train", stats=aks_stats) if h.malayalam not in excluded]
    stats.update(aks_stats)
    stats["dakshina train pairs usable"] = len(dak)
    stats["aksharantar pairs usable"] = len(aks)
    n_dak = round(n * dakshina_share)
    out = [(h.latin, h.malayalam) for h in rng.choices(dak, weights=[h.count for h in dak], k=n_dak)]
    out += rng.sample(aks, min(n - n_dak, len(aks)))
    return out


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
    p.add_argument("--rules", type=Path, default=calibrate.CALIBRATED, help="calibrated rules JSON")
    p.add_argument("--chat-share", type=float, default=0.5, help="draws that use the hand-set chat rules")
    p.add_argument("--human-share", type=float, default=0.3, help="share of train rows that are human pairs")
    p.add_argument("--dakshina-share", type=float, default=0.25, help="share of human rows from Dakshina")
    args = p.parse_args(argv)

    start = time.monotonic()
    words = mlengine.malayalam_words(str(args.lexicon), args.words)
    excluded = excluded_words(EVAL_SETS) | human.held_out_words()
    left_out = sum(w in excluded for w, _ in words)
    synthetic_rows = round(args.rows * (1 - args.human_share))
    rules = calibrate.load(args.rules)
    train, valid = build(
        words, synthetic_rows, args.valid_frac, excluded, args.seed, rules, alt_rules=RULES, alt_share=args.chat_share
    )
    # Human rows: never an eval word, a held-out Dakshina word, or a synthetic valid word.
    valid_words = {w for _, w in valid}
    n_human = round(len(train) * args.human_share / (1 - args.human_share))
    stats: dict[str, int] = {}
    humans = human_rows(n_human, args.dakshina_share, excluded | valid_words, args.seed, stats)
    train += humans
    random.Random(args.seed + 2).shuffle(train)
    valid_human = [(h.latin, h.malayalam) for h in human.dakshina("dev")]
    args.out.mkdir(parents=True, exist_ok=True)
    write(args.out / "train.tsv", train)
    write(args.out / "valid.tsv", valid)
    write(args.out / "valid-human.tsv", valid_human)

    for key, value in sorted(stats.items()):
        print(f"  {key}: {value:,}")
    print(f"human rows in train: {len(humans):,} ({len(humans) / len(train):.1%})")
    for name, pairs in (("train", train), ("valid", valid), ("valid-human", valid_human)):
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
