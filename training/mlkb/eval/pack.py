"""A pack's held-out transliteration accuracy as the keyboard gives it: every Latin word of the
pack's held-out set typed into the production engine, with and without the pack's transliterator.

    uv run python -m mlkb.eval.pack --lang ta
    uv run python -m mlkb.eval.pack --pack ../build/xx.neopack --set ../data/eval/xx/translit-human.tsv

Works for any pack: the engine is opened from the pack's `manifest.json` (every role it lists),
with the app's English lexicon beside it, as on the phone. The set is
`data/eval/<id>/translit-human.tsv` (`data/eval/translit-human.tsv` for Malayalam):
`context<TAB>latin<TAB>native` rows, never trained on. `# section: NAME` lines split it
(`mlkb.langs.pairs` writes one per Aksharantar source: AK-Freq common words, AK-NEI Indian
names, AK-NEF foreign names); a set without them is one section.

Two runs, each over all rows and per section:
- lexicons only: the pack without its `translit` file, so only the lexicon lookup answers;
- with the transliterator: the pack as shipped. Skipped when the pack has none.
A pack's language model, when it has one, is loaded in both (`--no-lm` leaves it out).

Columns:
- top-1: the first native-script suggestion in the bar is the target. The bar's other slots are
  the typed text and English words, so this is the pack's best guess whatever English does;
- in bar: the target is one of the bar's three suggestions;
- first slot: the target is the first suggestion, which is what space commits;
- none: the bar had no native-script suggestion at all.

This measures the pack through the engine's lexicon search and ranking. The model alone (beam
search, no lexicon, no ranking) is `mlkb.langs.eval_translit`; the two differ, and the engine
can be the lower one. `python -m mlkb.eval` is the gated eval with baselines (ml, hi).
"""

import argparse
import json
import sys
from dataclasses import dataclass
from pathlib import Path

import mlengine

from mlkb.eval import translit

ROOT = Path(__file__).resolve().parents[3]
ENGLISH = ROOT / "apps/ios/KeyboardExtension/Lexicon/en.fst"
# Manifest role -> `mlengine.Engine` argument.
ROLES = {
    "lexicon": "malayalam",
    "romanisedLexicon": "manglish",
    "romanisation": "romanisation",
    "suffixes": "suffixes",
    "chatForms": "chat_forms",
    "layout": "layout",
    "nextWords": "next_words",
    "lang": "lang",
    "translit": "translit",
    "lm": "lm",
}
ALL = "all"


def manifest(pack: Path) -> dict:
    """The pack's manifest; stops when it lists a file the folder lacks (the engine would load
    the pack without that file and say nothing)."""
    m = json.loads((pack / "manifest.json").read_text(encoding="utf-8"))
    absent = sorted(name for name in m["files"].values() if not (pack / name).is_file())
    if absent:
        raise SystemExit(f"{pack}: listed in the manifest but missing: {', '.join(absent)}")
    return m


def open_pack(pack: Path, without: tuple[str, ...] = ()):
    """The engine on `pack` and the app's English lexicon, leaving out the roles in `without`."""
    m = manifest(pack)
    files = m["files"]
    args = {arg: str(pack / files[role]) for role, arg in ROLES.items() if role in files and role not in without}
    return mlengine.Engine(english=str(ENGLISH), pack=m["id"], script=m["script"], **args)


@dataclass
class Tally:
    n: int = 0
    top1: int = 0
    in_bar: int = 0
    first: int = 0
    none: int = 0

    def add(self, bar: list[str], target: str) -> None:
        native = [t for t in bar if not t.isascii()]
        self.n += 1
        self.top1 += native[:1] == [target]
        self.in_bar += target in bar
        self.first += bar[:1] == [target]
        self.none += not native

    def rates(self) -> dict[str, float]:
        n = max(self.n, 1)
        return {
            "n": self.n,
            "top1": round(self.top1 / n, 4),
            "in_bar": round(self.in_bar / n, 4),
            "first": round(self.first / n, 4),
            "none": round(self.none / n, 4),
        }


def score(engine, rows: list[tuple[str, translit.Row]]) -> dict[str, Tally]:
    """Tallies for all rows (`ALL`) and for each section, in the order sections first appear."""
    out = {ALL: Tally()}
    for section, row in rows:
        bar = [text for text, _, _ in engine.suggest(translit.query(row), "latin", True)][:3]
        out[ALL].add(bar, row.malayalam)
        if section:
            out.setdefault(section, Tally()).add(bar, row.malayalam)
    return out


def _print(label: str, tallies: dict[str, Tally]) -> None:
    for section, tally in tallies.items():
        r = tally.rates()
        print(
            f"{label:24} {section:8} {r['n']:6,} pairs  top-1 {r['top1']:.1%}  in bar {r['in_bar']:.1%}  "
            f"first slot {r['first']:.1%}  none {r['none']:.1%}"
        )


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--lang", help="the pack's id: packs/<id>.neopack and its held-out set")
    p.add_argument("--pack", type=Path, help="a pack folder, instead of packs/<lang>.neopack")
    p.add_argument("--set", type=Path, dest="rows", help="the held-out set (default: the pack id's, in data/eval)")
    p.add_argument("--limit", type=int, default=0, help="first N rows only (0: all)")
    p.add_argument("--no-lm", action="store_true", help="leave the pack's language model out of both runs")
    p.add_argument("--json", type=Path, metavar="OUT", help="also write the numbers as JSON")
    args = p.parse_args(argv)
    if not args.lang and not args.pack:
        p.error("give --lang or --pack")
    pack = args.pack or ROOT / f"packs/{args.lang}.neopack"
    m = manifest(pack)
    default = "data/eval/translit-human.tsv" if m["id"] == "ml" else f"data/eval/{m['id']}/translit-human.tsv"
    path = args.rows or ROOT / default
    if not path.is_file():
        raise SystemExit(
            f"{path}: no held-out set. `python -m mlkb.langs.pairs --lang {m['id']} --eval-only` writes it, "
            "or give --set."
        )
    rows = translit.load_sections(path)
    if args.limit:
        rows = rows[: args.limit]
    print(f"{pack.name} {m['version']}: {len(rows):,} held-out pairs of {path.name}, typed into the engine")
    drop = ("lm",) if args.no_lm else ()
    runs = [("lexicons only", (*drop, "translit"))]
    if "translit" in m["files"]:
        runs.append(("with the transliterator", drop))
    else:
        print("the pack has no transliterator: one run")
    results = {}
    for label, without in runs:
        engine = open_pack(pack, without)
        print(f"{label}: sources {engine.loaded_sources()}")
        tallies = score(engine, rows)
        _print(label, tallies)
        results[label] = {section: t.rates() for section, t in tallies.items()}
    if args.json:
        out = {"pack": m["id"], "version": m["version"], "set": path.name, **results}
        args.json.write_text(json.dumps(out, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main())
