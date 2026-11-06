"""Transliteration pairs for a pack language: `latin<TAB>native` rows, as `mlkb.data.pairs` writes
them for Malayalam.

    uv run python -m mlkb.langs.pairs --lang as [--out training/build/as/pairs] [--rows 3000000]

Synthetic rows: the most frequent words of the pack's lexicon (`packs/<lang>.neopack/<lang>.fst`),
each drawn in proportion to `count ** 0.5`, spelled by `Tables.variants` from the fitted tables
(`romanisation-weights.json`, fitted to Aksharantar's manual rows) or, with probability `--chat-share`,
the hand-set ones (`romanisation.hand.json`), whose weights allow more chat shortcuts.

Human rows make up `--human-share` of `train.tsv`: Aksharantar `<code>_train.json`, filtered
(`mlkb.langs.human.aksharantar`).

1% of the lexicon words (not rows) go to `valid.tsv`. `valid-human.tsv` is Aksharantar's manual
valid rows (AK-Uni, AK-Freq). The manual test rows are the held-out eval set, written to
`data/eval/<lang>/translit-human.tsv`; neither they nor the valid-human words are in `train.tsv`.
"""

import argparse
import random
import time
from collections import Counter
from pathlib import Path

import mlengine
import numpy as np

from mlkb.langs import human
from mlkb.langs.romanisation import LANGS, Tables

ROOT = Path(__file__).resolve().parents[3]
FREQUENCY_POWER = 0.5

Pair = tuple[str, str]


def synthetic(
    words: list[tuple[str, int]],
    rows: int,
    valid_frac: float,
    excluded: set[str],
    seed: int,
    tables: Tables,
    alt: Tables | None = None,
    alt_share: float = 0.0,
) -> tuple[list[Pair], list[Pair]]:
    """About `rows` (latin, native) pairs split into train and valid by word, both shuffled."""
    words = [(w, c) for w, c in words if w not in excluded and tables.canonical(w)]
    rng = random.Random(seed)
    is_valid = [rng.random() < valid_frac for _ in words]
    weights = np.array([c for _, c in words], dtype=np.float64) ** FREQUENCY_POWER
    weights /= weights.sum()
    np_rng = np.random.default_rng(seed)
    pilot = np_rng.choice(len(words), size=min(2000, rows), p=weights)
    per_draw = np.mean([len(tables.variants(words[k][0], rng)) for k in pilot])
    draws = np_rng.multinomial(round(rows / per_draw), weights)
    train: list[Pair] = []
    valid: list[Pair] = []
    for (word, _), k, to_valid in zip(words, draws.tolist(), is_valid, strict=True):
        out = valid if to_valid else train
        for _ in range(k):
            t = alt if alt is not None and rng.random() < alt_share else tables
            out.extend((latin, word) for latin in t.variants(word, rng))
    rng.shuffle(train)
    rng.shuffle(valid)
    return train, valid


def write(path: Path, pairs: list[Pair]) -> None:
    with path.open("w", encoding="utf-8") as f:
        f.writelines(f"{latin}\t{word}\n" for latin, word in pairs)


def write_eval(lang: str, rows: list[human.HumanPair], path: Path) -> None:
    """`context<TAB>latin<TAB>native` with an empty context, as `data/eval/translit-human.tsv`."""
    code = human.load(lang).data["aksharantar"]
    seen = set()
    body = []
    for h in sorted(rows, key=lambda h: (h.source, h.native, h.latin)):
        if (h.latin, h.native) not in seen:
            seen.add((h.latin, h.native))
            body.append(f"\t{h.latin}\t{h.native}\n")
    header = (
        "# Human transliteration eval set: context<TAB>latin<TAB>native (context empty).\n"
        f"# {len(body)} distinct manual pairs (AK-Freq, AK-NEI, AK-NEF; in that order) of the Aksharantar\n"
        f"# {code}_test.json split, AI4Bharat (Madhani et al., 2022), CC BY. Built by\n"
        "# `python -m mlkb.langs.pairs`. Never train on these words.\n"
    )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(header + "".join(body), encoding="utf-8")


def main(argv: list[str] | None = None) -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--lang", default="as")
    p.add_argument("--out", type=Path)
    p.add_argument("--rows", type=int, default=3_000_000)
    p.add_argument("--words", type=int, default=300_000)
    p.add_argument("--valid-frac", type=float, default=0.01)
    p.add_argument("--seed", type=int, default=0)
    p.add_argument("--lexicon", type=Path)
    p.add_argument("--chat-share", type=float, default=0.5, help="draws that use the hand-set tables")
    p.add_argument("--human-share", type=float, default=0.3, help="share of train rows that are human pairs")
    args = p.parse_args(argv)
    out = args.out or ROOT / f"training/build/{args.lang}/pairs"
    lexicon = args.lexicon or ROOT / f"packs/{args.lang}.neopack/{args.lang}.fst"

    start = time.monotonic()
    tables = Tables.load(args.lang)
    hand = Tables.load(LANGS / args.lang / "romanisation.hand.json")
    stats: Counter = Counter()
    test = [h for h in human.aksharantar(args.lang, "test", stats) if h.source.startswith("AK-")]
    valid_human = [h for h in human.aksharantar(args.lang, "valid") if h.source.startswith("AK-")]
    excluded = {h.native for h in test} | {h.native for h in valid_human}

    words = mlengine.malayalam_words(str(lexicon), args.words)  # any lexicon: UTF-8 keys by count
    left_out = sum(w in excluded for w, _ in words)
    synthetic_rows = round(args.rows * (1 - args.human_share))
    train, valid = synthetic(
        words, synthetic_rows, args.valid_frac, excluded, args.seed, tables, alt=hand, alt_share=args.chat_share
    )
    valid_words = {w for _, w in valid}
    mined = [(h.latin, h.native) for h in human.aksharantar(args.lang, "train", stats)]
    mined = [(latin, w) for latin, w in mined if w not in excluded and w not in valid_words]
    n_human = round(len(train) * args.human_share / (1 - args.human_share))
    rng = random.Random(args.seed + 1)
    # Fewer distinct human pairs than wanted: each is repeated, so they keep their share.
    humans = [mined[i % len(mined)] for i in range(n_human)] if mined else []
    train += humans
    rng.shuffle(train)

    out.mkdir(parents=True, exist_ok=True)
    write(out / "train.tsv", train)
    write(out / "valid.tsv", valid)
    write(out / "valid-human.tsv", [(h.latin, h.native) for h in valid_human])
    write_eval(args.lang, test, ROOT / f"data/eval/{args.lang}/translit-human.tsv")

    for key, value in sorted(stats.items()):
        print(f"  {key}: {value:,}")
    print(f"distinct human pairs {len(set(mined)):,}; human rows in train {len(humans):,} ({len(humans) / len(train):.1%})")
    for name, pairs in (("train", train), ("valid", valid), ("valid-human", [(h.latin, h.native) for h in valid_human])):
        lengths = [len(latin) for latin, _ in pairs]
        print(
            f"{name}: {len(pairs):,} rows, {len({w for _, w in pairs}):,} words, {len(set(pairs)):,} distinct pairs, "
            f"latin length mean {np.mean(lengths):.1f} max {max(lengths)}"
        )
    print(f"test (held out): {len(test):,} pairs; lexicon words read {len(words):,}; eval words left out {left_out}")
    print(f"wrote {out} in {time.monotonic() - start:.0f}s")


if __name__ == "__main__":
    main()
