"""Held-out top-k accuracy of an exported transliterator, without the engine.

    uv run --group train python -m mlkb.translit.accuracy --lang hi \\
        --model build/hi/translit.mlkbnn [--eval data/eval/hi/translit-human.tsv]

Runs the model exactly as the file stores it (int8 weights dequantised, `export.dequantised_model`)
with beam search (`decode.beam`) on each `context<TAB>latin<TAB>native` row of the eval set, and
reports exact-match top-1 and top-3:

- **model**: the beam's own best spellings;
- **in lexicon**: the beam (width `--lexicon-beam`) filtered to words of the language's lexicon,
  roughly what the keyboard's lexicon-guided search sees. A target missing from the lexicon can't
  be found this way; their share is reported.

`mlkb.eval` measures Malayalam through the engine; this is for a language the engine can't load yet.
"""

import argparse
import json
import sys
from pathlib import Path

import mlengine
import torch

from mlkb import langs
from mlkb.translit.decode import beam
from mlkb.translit.export import dequantised_model
from mlkb.translit.vocab import native


def rows(path: Path) -> list[tuple[str, str]]:
    out = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.strip() and not line.startswith("#"):
            _, latin, target = line.split("\t")
            out.append((latin, target))
    return out


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--lang", default="ml")
    p.add_argument("--model", type=Path, required=True)
    p.add_argument("--eval", type=Path)
    p.add_argument("--beam", type=int, default=4)
    p.add_argument("--lexicon-beam", type=int, default=8)
    p.add_argument("--out", type=Path, help="also write the numbers as JSON")
    args = p.parse_args(argv)
    lang = langs.load(args.lang)
    vocab = native(args.lang)
    data = rows(args.eval or lang.eval_dir / "translit-human.tsv")
    lexicon = {w for w, _ in mlengine.malayalam_words(str(lang.lexicon), 10**8)}
    torch.set_num_threads(8)
    model = dequantised_model(args.model)
    hits = {"model_top1": 0, "model_top3": 0, "lexicon_top1": 0, "lexicon_top3": 0}
    in_lexicon = 0
    for latin, target in data:
        own = [w for w, _ in beam(model, latin, args.beam, vocab=vocab)]
        wide = [w for w, _ in beam(model, latin, args.lexicon_beam, vocab=vocab)]
        known = [w for w in wide if w in lexicon]
        hits["model_top1"] += own[:1] == [target]
        hits["model_top3"] += target in own[:3]
        hits["lexicon_top1"] += known[:1] == [target]
        hits["lexicon_top3"] += target in known[:3]
        in_lexicon += target in lexicon
    n = len(data)
    result = {k: round(v / n, 4) for k, v in hits.items()}
    result |= {"pairs": n, "targets_in_lexicon": round(in_lexicon / n, 4), "model": str(args.model)}
    print(json.dumps(result, indent=2))
    if args.out:
        args.out.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main())
