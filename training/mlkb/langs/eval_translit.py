"""Held-out accuracy of a pack language's transliterator, the model alone (no lexicon search, no
LM: the engine's lexicon-constrained decoder is Rust, and has no plug-in for this script yet).

    uv run --group train python -m mlkb.langs.eval_translit --lang as \\
        --model training/build/as/translit.mlkbnn

Reads `data/eval/<lang>/translit-human.tsv` (context, latin, native; Aksharantar's manual test
rows, never trained on). For each row it beam-decodes the Latin (`--beam` wide) from the int8
weights as the engine sees them, and reports exact-match top-1 and top-3, overall and by
source section; then the same with the beam's spellings that are words of the pack's lexicon
moved first ("lexicon-first"), a rough stand-in for the engine's lexicon-constrained search.
"""

import argparse
from pathlib import Path

import mlengine
import torch

from mlkb.translit.decode import beam
from mlkb.translit.export import dequantised_model
from mlkb.translit.vocab import native

ROOT = Path(__file__).resolve().parents[3]


def rows(path: Path) -> list[tuple[str, str]]:
    out = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.startswith("#") or not line.strip():
            continue
        _, latin, target = line.split("\t")
        out.append((latin, target))
    return out


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
    stats = {k: 0 for k in ("top1", "top3", "lex1", "lex3", "in_lexicon")}
    for latin, target in data:
        out = [w for w, _ in beam(model, latin, args.beam, vocab=vocab)]
        stats["top1"] += out[:1] == [target]
        stats["top3"] += target in out[:3]
        ranked = [w for w in out if w in lexicon] + [w for w in out if w not in lexicon]
        stats["lex1"] += ranked[:1] == [target]
        stats["lex3"] += target in ranked[:3]
        stats["in_lexicon"] += target in lexicon
    n = len(data)
    print(f"{args.model}: {n:,} held-out pairs ({stats['in_lexicon'] / n:.1%} of targets in the lexicon)")
    print(f"model alone:   top-1 {stats['top1'] / n:.1%}  top-3 {stats['top3'] / n:.1%}")
    print(f"lexicon-first: top-1 {stats['lex1'] / n:.1%}  top-3 {stats['lex3'] / n:.1%}")


if __name__ == "__main__":
    main()
