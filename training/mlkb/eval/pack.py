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

`--stages` (needs torch: `uv run --group train`) says where the engine loses words. It decodes
each word with the model alone (beam `--beam`, cached under `training/build/<id>/`), and files
every row whose first native suggestion is not the target under the first stage that fits:
  normal-form        the target is not in the engine's normal form: it can never be offered
  model-miss         the model alone doesn't have it in its beam either
  shown-second       it is in the bar, behind another native word
  off-bar            it is in the 20 suggestions the bar scrolls through, past the three shown
  english-word       the typed letters are an English word: the transliterator isn't asked
  past-20            a candidate (a lexicon word the model read, or its free spelling) that
                     more than 20 others outscored
  below-floor        a lexicon word the constrained beam found, under `rank::MANGLISH_MODEL_FLOOR`
  beam-width         a lexicon word the constrained beam finds only when wider than the engine's
  beam-miss          a lexicon word the constrained beam doesn't find at any width
  free-not-offered   not a lexicon word; the free beam spells it, but a lexicon word was found
                     for the letters, so the free spelling isn't offered
  free-differs       not a lexicon word, and the engine's free beam (one hypothesis) spells
                     another word than the model's beam does
The counts are for all such rows, for those the model alone has first (what the engine loses
to it), and for those it has in its first three. `--misses OUT` writes each row and its stage.
"""

import argparse
import hashlib
import json
import sys
from collections import Counter
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
# `engine::MODEL_WIDTH`, `rank::MANGLISH_MODEL_FLOOR` and `nn::translit::MAX_CANDIDATES - 1`.
MODEL_WIDTH = 4
MODEL_FLOOR = -1.0
WIDEST = 14
STAGES = (
    "normal-form",
    "model-miss",
    "shown-second",
    "off-bar",
    "english-word",
    "past-20",
    "below-floor",
    "beam-width",
    "beam-miss",
    "free-not-offered",
    "free-differs",
)


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


def model_alone(lang: str, model_path: Path, words: list[str], width: int) -> dict[str, list[str]]:
    """The model's beam for each word, without the lexicon; cached under `training/build/<lang>/`
    by the model file's hash."""
    digest = hashlib.sha256(model_path.read_bytes()).hexdigest()[:16]
    cache = ROOT / f"training/build/{lang}/model-alone-{digest}-{width}.json"
    found: dict[str, list[str]] = json.loads(cache.read_text(encoding="utf-8")) if cache.exists() else {}
    todo = [w for w in dict.fromkeys(words) if w not in found]
    if todo:
        import torch

        from mlkb.translit.decode import beam
        from mlkb.translit.export import dequantised_model
        from mlkb.translit.vocab import native

        torch.set_num_threads(4)
        vocab = native(lang)
        model = dequantised_model(model_path)
        for word in todo:
            found[word] = [text for text, _ in beam(model, word, width, vocab=vocab)]
        cache.parent.mkdir(parents=True, exist_ok=True)
        cache.write_text(json.dumps(found, ensure_ascii=False), encoding="utf-8")
    return found


def stage(engine, latin: str, target: str, bar: list[str], beam: list[str]) -> str:
    """Why the first native suggestion for `latin` isn't `target` (the module's list); `bar` is
    the engine's 20 suggestions, `beam` the model's own."""
    if engine.normalize(target) != target:
        return "normal-form"
    if target not in beam:
        return "model-miss"
    if target in bar[:3]:
        return "shown-second"
    if target in bar:
        return "off-bar"
    if engine.is_english(latin):
        return "english-word"
    held = engine.model(latin, MODEL_WIDTH, True)
    if engine.lexicon_log_prob(target) is not None:
        score = next((s for text, s, known in held if known and text == target), None)
        if score is not None:
            return "below-floor" if score < MODEL_FLOOR else "past-20"
        wide = engine.model(latin, WIDEST, False)
        return "beam-width" if any(text == target for text, _, _ in wide) else "beam-miss"
    free = next((text for text, _, known in held if not known), None)
    if free != target:
        return "free-differs"
    whole = any(missing == 0 for _, _, missing, _, _ in engine.lookup(latin))
    read = any(known and s >= MODEL_FLOOR for _, s, known in held)
    return "free-not-offered" if whole or read else "past-20"


def stages(pack: Path, m: dict, rows: list[tuple[str, translit.Row]], width: int, without: tuple[str, ...]):
    """The model alone beside the engine, and the stage of each engine miss: the summary lines,
    the stage counts (all misses, the model's top-1, its top-3) and the misses."""
    engine = open_pack(pack, without)
    engine.set_max_suggestions(20)
    beams = model_alone(m["id"], pack / m["files"]["translit"], [row.latin for _, row in rows], width)
    count: Counter[str] = Counter()
    by_stage = {k: Counter() for k in ("all", "model1", "model3")}
    misses = []
    for section, row in rows:
        target, beam = row.malayalam, beams[row.latin]
        bar = [text for text, _, _ in engine.suggest(translit.query(row), "latin", True)]
        native = [t for t in bar[:3] if not t.isascii()]
        count["model1"] += beam[:1] == [target]
        count["model3"] += target in beam[:3]
        count["lexicon"] += engine.lexicon_log_prob(target) is not None
        count["in20"] += target in bar
        if native[:1] == [target]:
            count["engine only"] += beam[:1] != [target]
            continue
        s = stage(engine, row.latin, target, bar, beam)
        by_stage["all"][s] += 1
        by_stage["model1"][s] += beam[:1] == [target]
        by_stage["model3"][s] += target in beam[:3]
        misses.append((s, section, row.latin, target, " ".join(bar[:3]), " ".join(beam[:3])))
    n = max(len(rows), 1)
    lines = [
        f"{count['lexicon'] / n:.1%} of the targets are lexicon words; the engine has {count['in20'] / n:.1%} "
        "in its 20 suggestions",
        f"model alone (beam {width}): top-1 {count['model1'] / n:.1%}  top-3 {count['model3'] / n:.1%}",
        f"engine right where the model alone is wrong: {count['engine only']:,}",
    ]
    return lines, by_stage, misses


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--lang", help="the pack's id: packs/<id>.neopack and its held-out set")
    p.add_argument("--pack", type=Path, help="a pack folder, instead of packs/<lang>.neopack")
    p.add_argument("--set", type=Path, dest="rows", help="the held-out set (default: the pack id's, in data/eval)")
    p.add_argument("--limit", type=int, default=0, help="first N rows only (0: all)")
    p.add_argument("--no-lm", action="store_true", help="leave the pack's language model out of both runs")
    p.add_argument("--json", type=Path, metavar="OUT", help="also write the numbers as JSON")
    p.add_argument("--stages", action="store_true", help="the model alone, and the stage that loses each miss")
    p.add_argument("--beam", type=int, default=8, help="the model alone's beam, for --stages")
    p.add_argument("--misses", type=Path, metavar="OUT", help="write each miss and its stage (TSV); implies --stages")
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
    if (args.stages or args.misses) and "translit" in m["files"]:
        lines, by_stage, misses = stages(pack, m, rows, args.beam, drop)
        print("\n".join(lines))
        print(f"{'stage':<18}{'all misses':>12}{'model top-1':>13}{'model top-3':>13}")
        for s in STAGES:
            print(f"{s:<18}{by_stage['all'][s]:>12,}{by_stage['model1'][s]:>13,}{by_stage['model3'][s]:>13,}")
        totals = [sum(by_stage[k].values()) for k in ("all", "model1", "model3")]
        print(f"{'total':<18}{totals[0]:>12,}{totals[1]:>13,}{totals[2]:>13,}")
        results["stages"] = {k: dict(v) for k, v in by_stage.items()}
        if args.misses:
            args.misses.parent.mkdir(parents=True, exist_ok=True)
            table = ["stage\tsection\tlatin\ttarget\tbar\tmodel"] + ["\t".join(row) for row in misses]
            args.misses.write_text("\n".join(table) + "\n", encoding="utf-8")
    if args.json:
        out = {"pack": m["id"], "version": m["version"], "set": path.name, **results}
        args.json.write_text(json.dumps(out, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main())
