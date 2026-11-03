"""Runs the eval sets through the production engine on the real lexicon bundle.

    uv run python -m mlkb.eval [--record] [--translit PATH]

Prints top-1/top-3 on `data/eval/chat-golden.tsv` (and on
`data/eval/translit-human.tsv` when it exists), the English wrong-replacement
rate, and every miss. `--record` writes `data/eval/baselines.json`; without
it, the run exits 1 when a metric is worse than the recorded baseline.
"""

import argparse
import json
import sys
from pathlib import Path

import mlengine

from mlkb.eval import english, translit

ROOT = Path(__file__).resolve().parents[3]
LEX = ROOT / "apps/ios/KeyboardExtension/Lexicon"
EVAL = ROOT / "data/eval"
BASELINES = EVAL / "baselines.json"

# How much worse than the baseline a metric may get before the run fails.
ACCURACY_TOLERANCE = 0.005
WRONG_RATE_TOLERANCE = 0.001


def load_engine(translit_model: str | None):
    """The engine on the committed bundle, plus the transliterator model when given."""
    return mlengine.Engine(
        english=str(LEX / "en.fst"),
        malayalam=str(LEX / "ml.fst"),
        manglish=str(LEX / "manglish.fst"),
        next_words=str(LEX / "next.fst"),
        translit=translit_model,
    )


def regressions(current: dict, baseline: dict) -> list[str]:
    """Every metric in `current` that is worse than `baseline` by more than its tolerance."""
    found = []
    for set_name in ("chat_golden", "human"):
        now, base = current.get(set_name), baseline.get(set_name)
        if not now or not base:
            continue
        for metric in ("top1", "top3"):
            if now[metric] < base[metric] - ACCURACY_TOLERANCE:
                found.append(f"{set_name} {metric}: {now[metric]:.4f} < baseline {base[metric]:.4f}")
    now, base = current["english_wrong_rate"], baseline["english_wrong_rate"]
    if now > base + WRONG_RATE_TOLERANCE:
        found.append(f"english_wrong_rate: {now:.4f} > baseline {base:.4f}")
    return found


def run_translit(engine, name: str, path: Path) -> dict:
    """Scores one transliteration set, prints it with its misses, and returns its metrics."""
    s = translit.score(engine, translit.load(path))
    print(f"{name}: top1 {s.top1:.4f}  top3 {s.top3:.4f}  n {s.n}")
    for row in s.misses:
        bar = [text for text, _, _ in engine.suggest(translit.query(row), "latin", True)][:3]
        print(f"  miss  {translit.query(row)!r:32} want {row.malayalam:14} got {bar}")
    return {"top1": round(s.top1, 4), "top3": round(s.top3, 4), "n": s.n}


def run_english(engine, path: Path) -> float:
    """Prints the English wrong-replacement rate and every replacement; returns the rate."""
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
    return round(rate, 4)


def main(argv: list[str] | None = None) -> int:
    """Entry point; returns the process exit code."""
    parser = argparse.ArgumentParser(prog="python -m mlkb.eval", description=__doc__.splitlines()[0])
    parser.add_argument("--record", action="store_true", help="write data/eval/baselines.json")
    parser.add_argument("--translit", metavar="PATH", help="transliterator model file")
    args = parser.parse_args(argv)

    engine = load_engine(args.translit)
    print(f"sources: {engine.loaded_sources()}")
    human = EVAL / "translit-human.tsv"
    current = {
        "chat_golden": run_translit(engine, "chat_golden", EVAL / "chat-golden.tsv"),
        "human": run_translit(engine, "human", human) if human.exists() else None,
        "english_wrong_rate": run_english(engine, EVAL / "english-sentences.txt"),
        "engine": "m3a" if args.translit else "m2",
    }

    if args.record:
        BASELINES.write_text(json.dumps(current, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        print(f"recorded {BASELINES.relative_to(ROOT)}")
        return 0
    baseline = json.loads(BASELINES.read_text(encoding="utf-8"))
    found = regressions(current, baseline)
    for line in found:
        print(f"REGRESSION {line}")
    if not found:
        print(f"no regressions against the {baseline['engine']} baseline")
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
