"""Held-out accuracy of a pack language's transliterator, the model alone (beam search in
PyTorch: no lexicon search, no ranking, no LM). What the keyboard gives for the same rows, through
the engine, is `python -m mlkb.eval.pack --lang <lang>`.

    uv run --group train python -m mlkb.langs.eval_translit --lang as \\
        --model training/build/as/translit.mlkbnn

Reads `data/eval/<lang>/translit-human.tsv` (context, latin, native; Aksharantar's manual test
rows, never trained on). For each row it beam-decodes the Latin (`--beam` wide) from the int8
weights as the engine sees them, and reports exact-match top-1 and top-3 ("model alone"); then
the same with the beam's spellings that are words of the pack's lexicon moved first
("lexicon-first"), a rough stand-in for the engine's lexicon-constrained search. Both over all
rows and for each section of the file (`# section:` lines: AK-Freq common words, AK-NEI Indian
names, AK-NEF foreign names).
"""

import argparse
from pathlib import Path

import mlengine
import torch

from mlkb.eval.translit import load_sections
from mlkb.translit.decode import beam
from mlkb.translit.export import dequantised_model
from mlkb.translit.vocab import native

ROOT = Path(__file__).resolve().parents[3]


KEYS = ("n", "top1", "top3", "lex1", "lex3", "in_lexicon")


def rows(path: Path) -> list[tuple[str, str, str]]:
    """`(section, latin, native)` for every row; the section is "" in a file without sections."""
    return [(section, row.latin, row.malayalam) for section, row in load_sections(path)]


def tally(stats: dict[str, int], out: list[str], target: str, lexicon: set[str]) -> None:
    """Adds one row's beam output `out` (best first) to `stats`."""
    ranked = [w for w in out if w in lexicon] + [w for w in out if w not in lexicon]
    stats["n"] += 1
    stats["top1"] += out[:1] == [target]
    stats["top3"] += target in out[:3]
    stats["lex1"] += ranked[:1] == [target]
    stats["lex3"] += target in ranked[:3]
    stats["in_lexicon"] += target in lexicon


def report(label: str, stats: dict[str, int]) -> list[str]:
    n = max(stats["n"], 1)
    return [
        f"{label:8} {stats['n']:6,} pairs ({stats['in_lexicon'] / n:.1%} of targets in the lexicon)  "
        f"model alone: top-1 {stats['top1'] / n:.1%}  top-3 {stats['top3'] / n:.1%}  "
        f"lexicon-first: top-1 {stats['lex1'] / n:.1%}  top-3 {stats['lex3'] / n:.1%}"
    ]


def main(argv: list[str] | None = None) -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--lang", default="as")
    p.add_argument("--model", type=Path, required=True)
    p.add_argument("--beam", type=int, default=8)
    p.add_argument("--limit", type=int, default=0, help="first N rows only (0: all)")
    args = p.parse_args(argv)
    torch.set_num_threads(4)
    vocab = native(args.lang)
    model = dequantised_model(args.model)
    data = rows(ROOT / f"data/eval/{args.lang}/translit-human.tsv")
    if args.limit:
        data = data[: args.limit]
    lexicon = {w for w, _ in mlengine.malayalam_words(str(ROOT / f"packs/{args.lang}.neopack/{args.lang}.fst"), 10**7)}
    stats: dict[str, dict[str, int]] = {"all": dict.fromkeys(KEYS, 0)}
    for section, latin, target in data:
        out = [w for w, _ in beam(model, latin, args.beam, vocab=vocab)]
        tally(stats["all"], out, target, lexicon)
        if section:
            tally(stats.setdefault(section, dict.fromkeys(KEYS, 0)), out, target, lexicon)
    print(f"{args.model}: {len(data):,} held-out pairs, the model alone (beam {args.beam})")
    for section, s in stats.items():
        print("\n".join(report(section, s)))
    print(f"through the engine, with and without the model: python -m mlkb.eval.pack --lang {args.lang}")

if __name__ == "__main__":
    main()
