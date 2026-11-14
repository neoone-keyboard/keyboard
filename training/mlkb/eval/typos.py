"""Romanised words typed with one slip: how often the keyboard still offers the native word.

    uv run python -m mlkb.eval.typos --write            # (re)writes the sets, from the human sets
    uv run python -m mlkb.eval.typos                    # scores ml hi ta as, against the baselines
    uv run python -m mlkb.eval.typos --lang hi --misses neighbour
    uv run python -m mlkb.eval.typos --record           # writes data/eval/typos-baselines.json

The sets are made from each pack's held-out human set (`data/eval/translit-human.tsv` for
Malayalam, `data/eval/<id>/translit-human.tsv` for the others), rows whose Latin is four or more
plain letters, with one slip put in each, seeded so every run writes the same files:
  neighbour      one letter replaced by a key next to it on QWERTY (the engine's `keys.rs`
                 adjacency), at a random position: NEIGHBOUR rows
  transposition  two neighbouring letters swapped: OTHER rows
  deletion       one letter left out: OTHER rows
  doubling       one letter typed twice: OTHER rows
written to `translit-typos.tsv` beside the human set, `context<TAB>latin<TAB>native` with a
`# section: KIND` line before each kind (`mlkb.eval.translit.load_sections`). A slip can make
another spelling of the same word (`veettil` → `vettil`); the set keeps it, since the keyboard
should find the word either way.

Scored like `mlkb.eval.pack` (English and the pack, as the phone opens them; first three slots):
top-1 is the first native suggestion being the target, top-3 the target in the bar, first slot
the target in the middle (what space commits). Baselines in `data/eval/typos-baselines.json`;
a run fails when a number falls more than `TOLERANCE` below its baseline.
"""

import argparse
import json
import random
import sys
from pathlib import Path

from mlkb.eval import translit

ROOT = Path(__file__).resolve().parents[3]
EVAL = ROOT / "data/eval"
BASELINES = EVAL / "typos-baselines.json"
LANGS = ("ml", "hi", "ta", "as")
KINDS = ("neighbour", "transposition", "deletion", "doubling")
NEIGHBOUR = 1000
OTHER = 400
MIN_LETTERS = 4
TOLERANCE = 0.005

# `engine/core/src/keys.rs`: key centres in key widths, neighbours within 1.6 of each other.
ROWS = (("qwertyuiop", 0.0), ("asdfghjkl", 0.5), ("zxcvbnm", 1.5))
CENTRES = {k: (col + offset, row) for row, (keys, offset) in enumerate(ROWS) for col, k in enumerate(keys)}


def adjacent(a: str, b: str) -> bool:
    if a == b or a not in CENTRES or b not in CENTRES:
        return False
    (ax, ay), (bx, by) = CENTRES[a], CENTRES[b]
    return (ax - bx) ** 2 + (ay - by) ** 2 <= 1.6 * 1.6


NEIGHBOURS = {a: sorted(b for b in CENTRES if adjacent(a, b)) for a in CENTRES}


def human_set(lang: str) -> Path:
    return EVAL / ("translit-human.tsv" if lang == "ml" else f"{lang}/translit-human.tsv")


def typo_set(lang: str) -> Path:
    return EVAL / ("translit-typos.tsv" if lang == "ml" else f"{lang}/translit-typos.tsv")


def slip(kind: str, word: str, rng: random.Random) -> str | None:
    """`word` with one slip of `kind`, or None when it has no place for one."""
    if kind == "neighbour":
        i = rng.randrange(len(word))
        return word[:i] + rng.choice(NEIGHBOURS[word[i]]) + word[i + 1:]
    if kind == "transposition":
        places = [i for i in range(len(word) - 1) if word[i] != word[i + 1]]
        if not places:
            return None
        i = rng.choice(places)
        return word[:i] + word[i + 1] + word[i] + word[i + 2:]
    if kind == "deletion":
        i = rng.randrange(len(word))
        return word[:i] + word[i + 1:]
    i = rng.randrange(len(word))
    return word[:i] + word[i] + word[i:]


def make(lang: str) -> dict[str, list[translit.Row]]:
    """The typo rows of `lang`, by kind; the same every run."""
    rows = [r for r in translit.load(human_set(lang)) if len(r.latin) >= MIN_LETTERS and r.latin.isascii()
            and r.latin.isalpha() and r.latin.islower()]
    out = {}
    for kind in KINDS:
        rng = random.Random(f"typos-{lang}-{kind}")
        picked = rng.sample(rows, min(len(rows), NEIGHBOUR if kind == "neighbour" else OTHER))
        made = []
        for row in picked:
            typo = slip(kind, row.latin, rng)
            if typo and typo != row.latin:
                made.append(translit.Row(row.context, typo, row.malayalam))
        out[kind] = made
    return out


def write(lang: str) -> Path:
    path = typo_set(lang)
    sets = make(lang)
    source = human_set(lang).relative_to(ROOT)
    lines = [
        "# Romanised typo set: context<TAB>latin<TAB>native (context empty), each Latin word of",
        f"# {source} (four letters or more) with one slip put in, by kind (see the sections).",
        "# Built by `python -m mlkb.eval.typos --write` (seeded); never train on these words.",
    ]
    for kind, rows in sets.items():
        lines.append(f"# section: {kind}")
        lines += [f"{r.context}\t{r.latin}\t{r.malayalam}" for r in rows]
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return path


def score(lang: str, misses: str | None = None) -> dict[str, dict]:
    """Per kind (and all): top-1, top-3 and first slot, as the module says."""
    from mlkb.eval import pack

    engine = pack.open_pack(ROOT / f"packs/{lang}.neopack")
    tallies: dict[str, pack.Tally] = {"all": pack.Tally()}
    for section, row in translit.load_sections(typo_set(lang)):
        bar = [text for text, _, _ in engine.suggest(translit.query(row), "latin", True)][:3]
        tallies["all"].add(bar, row.malayalam)
        tallies.setdefault(section, pack.Tally()).add(bar, row.malayalam)
        native = [t for t in bar if not t.isascii()]
        if misses == section and native[:1] != [row.malayalam]:
            print(f"  miss {lang} {row.latin!r:22} want {row.malayalam:14} got {bar}")
    out = {}
    for section, t in tallies.items():
        r = t.rates()
        out[section] = {"n": r["n"], "top1": r["top1"], "top3": r["in_bar"], "first": r["first"]}
    return out


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--lang", nargs="+", default=list(LANGS), help="pack ids (default: ml hi ta as)")
    p.add_argument("--write", action="store_true", help="write the typo sets, then stop")
    p.add_argument("--record", action="store_true", help="write the scores as the baselines")
    p.add_argument("--misses", metavar="KIND", help="print the rows of KIND whose first native word is wrong")
    p.add_argument("--json", type=Path, metavar="OUT", help="also write the scores as JSON")
    args = p.parse_args(argv)
    if args.write:
        for lang in args.lang:
            print(f"wrote {write(lang).relative_to(ROOT)}")
        return 0
    results = {}
    for lang in args.lang:
        results[lang] = score(lang, args.misses)
        for section, r in results[lang].items():
            print(f"{lang:3} {section:14} {r['n']:5}  top-1 {r['top1']:.4f}  top-3 {r['top3']:.4f}  "
                  f"first slot {r['first']:.4f}")
    if args.json:
        args.json.write_text(json.dumps(results, indent=2) + "\n", encoding="utf-8")
    if args.record:
        recorded = json.loads(BASELINES.read_text(encoding="utf-8")) if BASELINES.exists() else {}
        recorded.update(results)
        BASELINES.write_text(json.dumps(recorded, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(f"recorded {BASELINES.relative_to(ROOT)}")
        return 0
    if not BASELINES.exists():
        print(f"no baselines at {BASELINES.relative_to(ROOT)} yet: nothing to compare")
        return 0
    baseline = json.loads(BASELINES.read_text(encoding="utf-8"))
    found = []
    for lang, sections in results.items():
        for section, r in sections.items():
            base = baseline.get(lang, {}).get(section)
            for metric in ("top1", "top3", "first"):
                if base and r[metric] < base[metric] - TOLERANCE:
                    found.append(f"{lang} {section} {metric}: {r[metric]:.4f} < baseline {base[metric]:.4f}")
    for line in found:
        print(f"REGRESSION {line}")
    if not found:
        print("no regressions against the typo baselines")
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
