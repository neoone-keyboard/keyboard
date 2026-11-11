"""Runs the eval sets through the production engine on the real lexicon bundle.

    uv run python -m mlkb.eval [--record] [--translit PATH] [--lm PATH] [--tune] [--gate m3b]
        [--weights F=V,…]

Prints top-1/top-3 on `data/eval/chat-golden.tsv` (and on
`data/eval/translit-human.tsv` when it exists), the English wrong-replacement
rate, top-1/top-3 on the English typo set `data/eval/english-typos.tsv` (same
`context<TAB>typed<TAB>intended` format), and every miss. `--record` writes `data/eval/baselines.json`; without
it, the run exits 1 when a metric is worse than the recorded baseline.

Also: next-word top-1/top-3 and keystroke savings (`NEXT_SETS`, see `nextword.py`), and two
sets scored like chat_golden (`CONTEXT_SETS`): English typos in context and language choice.
`--tune` runs the `.tune` files of these sets instead (for weight sweeps): it prints, never
records and exits 0. `--gate m3b` also checks the M3b ship gates (`ship_gate`) against
`data/eval/baselines-m3a.json`. `--weights` sets `LmWeights` fields (`en=0.5,clip=4`) in place
of rank.rs's, to check a sweep point on the gate files; it can't be recorded.

`--lang hi` runs the Hindi pack (packs/hi.neopack) with English instead, on Hindi's sets
(`LANGS`): English no-harm (wrong replacements, typos, typos in context), the Hindi
transliteration set `data/eval/hi/translit-human.tsv`, and next words in English, Hindi script
(`hi/next-hi.heldout.txt`, from `mlkb.lm.corpus --lang hi`) and Hinglish
(`hi/next-hinglish.tsv`, from `mlkb.eval.next_sets --lang hi`). Its baselines are
`data/eval/hi/baselines.json`; `--record-nolm` records the run without an LM to
`hi/baselines-nolm.json`, and `--gate lm` checks the LM's ship gates against that record (as
`--gate m3b` does against M3a's for Malayalam).

`--lang as` and `--lang ta` run those packs the same way (`_pack_lang`), on
`data/eval/<id>/`: next words in the script (`next-<id>.heldout.txt`) and romanised
(`as/next-romanised.tsv`, `ta/next-tanglish.tsv`; `mlkb.eval.next_sets` says where each is from).
"""

import argparse
import json
import sys
from dataclasses import dataclass
from pathlib import Path

import mlengine

from mlkb.eval import english, nextword, translit

ROOT = Path(__file__).resolve().parents[3]
LEX = ROOT / "apps/ios/KeyboardExtension/Lexicon"
PACK = LEX.parents[3] / "packs/ml.neopack"
EVAL = ROOT / "data/eval"
BASELINES = EVAL / "baselines.json"
# The new metrics with the LM off, recorded before any model existed; never re-recorded.
BASELINES_M3A = EVAL / "baselines-m3a.json"

# How much worse than the baseline a metric may get before the run fails.
ACCURACY_TOLERANCE = 0.005
WRONG_RATE_TOLERANCE = 0.001

NEXT_SETS = {  # name: (gate files, tune file, layout)
    "next_en": (("english-sentences.txt",), "next-en.tune.txt", "latin"),
    # The SMC conversations and held-out Malayalam Wikipedia and Wikisource sentences.
    "next_ml": (("next-ml.txt", "next-ml.heldout.txt"), "next-ml.tune.txt", "malayalam"),
    "next_manglish": (("next-manglish.tsv",), "next-manglish.tune.tsv", "latin"),
}
CONTEXT_SETS = {  # scored like chat_golden: name: (gate file, tune file)
    "english_typos_context": ("english-typos-context.tsv", "english-typos-context.tune.tsv"),
    "language_choice": ("language-choice.tsv", "language-choice.tune.tsv"),
}
# Sets gated on top-1 and top-3 (missing on either side: skipped).
ACCURACY_SETS = ("chat_golden", "human", "english_typos", *CONTEXT_SETS)


@dataclass(frozen=True)
class EvalLang:
    """What `main` runs for one pack: its next-word and context sets, its transliteration set
    (`human`), whether it has a chat set, and where its baselines are."""

    id: str
    pack: Path
    script: str
    next_sets: dict
    context_sets: dict
    human: Path
    chat_golden: bool
    baselines: Path
    gate_baselines: Path  # what `--gate` compares against
    gate_label: str


LANGS = {
    "ml": EvalLang("ml", PACK, "mlym", NEXT_SETS, CONTEXT_SETS, EVAL / "translit-human.tsv", True, BASELINES,
                   BASELINES_M3A, "M3a"),
    "hi": EvalLang(
        "hi", ROOT / "packs/hi.neopack", "deva",
        {
            "next_en": (("english-sentences.txt",), "next-en.tune.txt", "latin"),
            "next_hi": (("hi/next-hi.heldout.txt",), "hi/next-hi.tune.txt", "native"),
            "next_hinglish": (("hi/next-hinglish.tsv",), "hi/next-hinglish.tune.tsv", "latin"),
        },
        {"english_typos_context": CONTEXT_SETS["english_typos_context"]},
        EVAL / "hi/translit-human.tsv", False, EVAL / "hi/baselines.json", EVAL / "hi/baselines-nolm.json",
        "baseline",
    ),
}


def _pack_lang(lang: str, script: str, romanised: str) -> EvalLang:
    """A pack of `data/langs/<id>/` with English: Hindi's sets, with the pack's own next-word
    files (`<id>/next-<id>.heldout.txt` from `mlkb.lm.corpus --lang <id>`, and the romanised
    rows `<id>/next-<romanised>.tsv` from `mlkb.eval.next_sets --lang <id>`)."""
    return EvalLang(
        lang, ROOT / f"packs/{lang}.neopack", script,
        {
            "next_en": (("english-sentences.txt",), "next-en.tune.txt", "latin"),
            f"next_{lang}": ((f"{lang}/next-{lang}.heldout.txt",), f"{lang}/next-{lang}.tune.txt", "native"),
            f"next_{romanised}": ((f"{lang}/next-{romanised}.tsv",), f"{lang}/next-{romanised}.tune.tsv", "latin"),
        },
        {"english_typos_context": CONTEXT_SETS["english_typos_context"]},
        EVAL / f"{lang}/translit-human.tsv", False, EVAL / f"{lang}/baselines.json",
        EVAL / f"{lang}/baselines-nolm.json", "baseline",
    )


LANGS["as"] = _pack_lang("as", "beng", "romanised")
LANGS["ta"] = _pack_lang("ta", "taml", "tanglish")


def load_engine(translit_model: str | None, lm_model: str | None = None, lang: str = "ml"):
    """The engine on the committed bundle, plus the transliterator and the LM when given."""
    if lang != "ml":
        pack = LANGS[lang].pack
        manifest = json.loads((pack / "manifest.json").read_text(encoding="utf-8"))
        files = manifest["files"]
        return mlengine.Engine(
            english=str(LEX / "en.fst"),
            malayalam=str(pack / files["lexicon"]),
            manglish=str(pack / files["romanisedLexicon"]),
            romanisation=str(pack / files["romanisation"]),
            layout=str(pack / files["layout"]),
            next_words=str(pack / files["nextWords"]),
            lang=str(pack / files["lang"]) if "lang" in files else None,
            translit=translit_model,
            lm=lm_model,
            pack=lang,
            script=manifest["script"],
        )
    return mlengine.Engine(
        english=str(LEX / "en.fst"),
        malayalam=str(PACK / "ml.fst"),
        manglish=str(PACK / "manglish.fst"),
        romanisation=str(PACK / "romanisation.json"),
        suffixes=str(PACK / "suffixes.tsv"),
        chat_forms=str(PACK / "chat-forms.tsv"),
        layout=str(PACK / "layout.json"),
        next_words=str(PACK / "next.fst"),
        translit=translit_model,
        lm=lm_model,
    )


def regressions(current: dict, baseline: dict, next_sets: dict = NEXT_SETS) -> list[str]:
    """Every metric in `current` that is worse than `baseline` by more than its tolerance."""
    found = []
    gated = [(name, ("top1", "top3")) for name in ACCURACY_SETS]
    gated += [(name, ("top1", "top3", "ks")) for name in next_sets]
    for set_name, metrics in gated:
        now, base = current.get(set_name), baseline.get(set_name)
        if not now or not base:
            continue
        for metric in metrics:
            if now[metric] < base[metric] - ACCURACY_TOLERANCE:
                found.append(f"{set_name} {metric}: {now[metric]:.4f} < baseline {base[metric]:.4f}")
    now, base = current["english_wrong_rate"], baseline["english_wrong_rate"]
    if now > base + WRONG_RATE_TOLERANCE:
        found.append(f"english_wrong_rate: {now:.4f} > baseline {base:.4f}")
    return found


def ship_gate(current: dict, m3a: dict, lang: EvalLang = LANGS["ml"]) -> list[str]:
    """The M3b ship gates (design §9) that `current` fails against the M3a baseline (or another
    pack's run without an LM): next-word top-3 and KS above the baseline's on every next-word set
    present, no more English wrong replacements, and the context sets' top-1 (typos in context,
    language choice) above the baseline's."""
    found = []
    label = lang.gate_label
    if current["english_wrong"] > m3a["english_wrong"]:
        found.append(f"english_wrong: {current['english_wrong']} > {label} {m3a['english_wrong']}")
    gated = [(name, ("top3", "ks")) for name in lang.next_sets]
    gated += [(name, ("top1",)) for name in lang.context_sets]
    for set_name, metrics in gated:
        now, base = current.get(set_name), m3a.get(set_name)
        if not now or not base:
            continue
        for metric in metrics:
            if now[metric] <= base[metric]:
                found.append(f"{set_name} {metric}: {now[metric]:.4f} not above {label} {base[metric]:.4f}")
    return found


def run_translit(engine, name: str, path: Path, quiet: bool = False) -> dict:
    """Scores one transliteration set, prints it with its misses, and returns its metrics."""
    s = translit.score(engine, translit.load(path))
    print(f"{name}: top1 {s.top1:.4f}  top3 {s.top3:.4f}  n {s.n}")
    for row in [] if quiet else s.misses:
        bar = [text for text, _, _ in engine.suggest(translit.query(row), "latin", True)][:3]
        print(f"  miss  {translit.query(row)!r:32} want {row.malayalam:14} got {bar}")
    return {"top1": round(s.top1, 4), "top3": round(s.top3, 4), "n": s.n}


def load_sentences(path: Path) -> list[nextword.Sentence]:
    """Aligned `roman<TAB>native` rows from a `.tsv`, else one plain sentence per line."""
    if path.suffix == ".tsv":
        return nextword.load_aligned(path)
    lines = path.read_text(encoding="utf-8").splitlines()
    return [nextword.plain(line) for line in lines if line.strip() and not line.startswith("#")]


def run_next(engine, name: str, paths: Path | list[Path], layout: str) -> dict | None:
    """Next-word top-1/top-3 and keystroke savings on one set (one file or several); None if a
    file is missing."""
    paths = [paths] if isinstance(paths, Path) else paths
    if missing := [p.name for p in paths if not p.exists()]:
        print(f"{name}: {', '.join(missing)} missing, skipped")
        return None
    sentences = [s for p in paths for s in load_sentences(p)]
    s = nextword.next_word(engine, sentences, layout)
    ks = nextword.keystroke_savings(engine, sentences, layout)
    print(f"{name}: top1 {s.top1:.4f}  top3 {s.top3:.4f}  ks {ks:.4f}  n {s.n}")
    return {"top1": round(s.top1, 4), "top3": round(s.top3, 4), "ks": round(ks, 4), "n": s.n}


def run_english(engine, path: Path) -> tuple[float, int]:
    """Prints the English wrong-replacement rate and every replacement; returns the rate and
    the number of wrong replacements."""
    sentences = [line for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]
    wrong = total = 0
    lines = []
    for sentence in sentences:
        replaced, n_wrong, n_total = english.check(engine, sentence)
        wrong += n_wrong
        total += n_total
        lines += [f"  wrong {typed!r} -> {got!r}  in {sentence!r}" for typed, got in replaced]
    rate = wrong / total
    print(f"english: wrong {wrong}/{total} = {rate:.4f}  ({len(sentences)} sentences)")
    print("\n".join(lines))
    return round(rate, 4), wrong


def main(argv: list[str] | None = None) -> int:
    """Entry point; returns the process exit code."""
    parser = argparse.ArgumentParser(prog="python -m mlkb.eval", description=__doc__.splitlines()[0])
    parser.add_argument("--lang", default="ml", choices=sorted(LANGS), help="the pack to run (with English)")
    parser.add_argument("--record", action="store_true", help="write data/eval/baselines.json (the pack's)")
    parser.add_argument("--record-nolm", action="store_true",
                        help="write the pack's baselines-nolm.json, the run without an LM that --gate lm reads")
    parser.add_argument("--quiet", action="store_true", help="leave out the transliteration misses")
    parser.add_argument("--translit", metavar="PATH", help="transliterator model file")
    parser.add_argument("--lm", metavar="PATH", help="language model file")
    parser.add_argument("--tune", action="store_true", help="run the .tune files; never record or gate")
    parser.add_argument("--gate", choices=["m3b", "lm"],
                        help="also check the ship gates: against baselines-m3a.json (m3b, Malayalam) or the "
                             "pack's baselines-nolm.json (lm)")
    parser.add_argument("--weights", default="", metavar="F=V,…",
                        help="override LmWeights fields (rank.rs defaults otherwise); never with --record")
    args = parser.parse_args(argv)
    if args.weights and (args.record or args.record_nolm):
        parser.error("--record takes the rank.rs weights")
    if args.record_nolm and args.lm:
        parser.error("--record-nolm records the run without an LM")
    lang = LANGS[args.lang]

    engine = load_engine(args.translit, args.lm, args.lang)
    if args.weights:
        engine.set_lm_weights(**{k: float(v) for k, v in (kv.split("=") for kv in args.weights.split(","))})
    print(f"sources: {engine.loaded_sources()}")
    if args.lm and f"{args.lang}:lm" not in engine.loaded_sources():
        print(f"lm: {args.lm} did not load (missing, or built against other lexicons)")
        return 1
    if args.tune:
        for name, (_, tune, layout) in lang.next_sets.items():
            run_next(engine, name, EVAL / tune, layout)
        for name, (_, tune) in lang.context_sets.items():
            run_translit(engine, name, EVAL / tune, args.quiet)
        return 0

    human = lang.human
    wrong_rate, wrong = run_english(engine, EVAL / "english-sentences.txt")
    current = {
        "chat_golden": run_translit(engine, "chat_golden", EVAL / "chat-golden.tsv", args.quiet)
        if lang.chat_golden else None,
        "human": run_translit(engine, "human", human, args.quiet) if human.exists() else None,
        "english_wrong_rate": wrong_rate,
        "english_wrong": wrong,
        "english_typos": run_translit(engine, "english_typos", EVAL / "english-typos.tsv", args.quiet),
    }
    for name, (gate, _) in lang.context_sets.items():
        current[name] = run_translit(engine, name, EVAL / gate, args.quiet) if (EVAL / gate).exists() else None
    for name, (gates, _, layout) in lang.next_sets.items():
        current[name] = run_next(engine, name, [EVAL / g for g in gates], layout)
    current["engine"] = "m3b" if args.lm else "m3a" if args.translit else "m2"

    if args.record or args.record_nolm:
        path = lang.gate_baselines if args.record_nolm else lang.baselines
        path.write_text(json.dumps(current, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        print(f"recorded {path.relative_to(ROOT)}")
        return 0
    found = []
    if lang.baselines.exists():
        baseline = json.loads(lang.baselines.read_text(encoding="utf-8"))
        found = regressions(current, baseline, lang.next_sets)
        for line in found:
            print(f"REGRESSION {line}")
        if not found:
            print(f"no regressions against the {baseline['engine']} baseline")
    else:
        print(f"no baselines at {lang.baselines.relative_to(ROOT)} yet: nothing to compare")
    failed = []
    if args.gate:
        failed = ship_gate(current, json.loads(lang.gate_baselines.read_text(encoding="utf-8")), lang)
        for line in failed:
            print(f"GATE {line}")
        if not failed:
            print("M3b ship gates pass" if args.gate == "m3b" else "LM ship gates pass")
    return 1 if found or failed else 0


if __name__ == "__main__":
    sys.exit(main())
