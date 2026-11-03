"""Sweeps the LM weights (`LmWeights`, design §6) on the tune sets:

    uv run python -m mlkb.eval.sweep --lm PATH --translit PATH [--fields f,…] [--start f=v,…]

One field at a time, in `GRID` order, around the best point so far: every value of a field is
run (in parallel, one engine per process) with the other fields at the best point, and the best
of them is kept for the next field. A point is ranked by `objective`: the mean of next-word
top-3 and KS over the three next-word sets, `english_typos_context` top-1 and `language_choice`
top-1, all on the `.tune` files. Points with more English wrong replacements than M3a (on
`english-sentences.txt`, the only English no-harm set) or with chat_golden worse than M3a's
past the usual tolerance are not eligible; both are printed for every point.
"""
import argparse
import json
import sys
from concurrent.futures import ProcessPoolExecutor

from mlkb.eval import english, nextword, translit
from mlkb.eval.__main__ import (ACCURACY_TOLERANCE, BASELINES_M3A, CONTEXT_SETS, EVAL,
                                NEXT_SETS, load_engine, load_sentences)

GRID = {
    "lang_offset": [-2.0, -1.5, -1.0, -0.5, 0.0],
    "lang_weight": [0.5, 1.0, 1.5, 2.0],
    "en": [0.0, 0.25, 0.5, 0.75, 1.0],
    "ml": [0.0, 0.25, 0.5, 0.75, 1.0],
    "clip": [2.0, 3.0, 4.0, 6.0],
    # A mixture weight (`rank::NEXT_BIGRAM_WEIGHT`): next.fst's probabilities, normalised over
    # the few followers it keeps, dwarf the LM's, so the useful range is small.
    "next_bigram": [0.0, 0.03, 0.07, 0.15, 0.3],
}

_engine = None


def _init(translit_model: str, lm_model: str) -> None:
    global _engine
    _engine = load_engine(translit_model, lm_model)


def measure(point: dict) -> dict:
    """The tune-set metrics, English wrong replacements and chat_golden at `point`."""
    _engine.set_lm_weights(**point)
    out = {"point": point}
    for name, (_, tune, layout) in NEXT_SETS.items():
        sentences = load_sentences(EVAL / tune)
        s = nextword.next_word(_engine, sentences, layout)
        out[name] = {"top1": s.top1, "top3": s.top3,
                     "ks": nextword.keystroke_savings(_engine, sentences, layout)}
    for name, (_, tune) in CONTEXT_SETS.items():
        s = translit.score(_engine, translit.load(EVAL / tune))
        out[name] = {"top1": s.top1, "top3": s.top3}
    s = translit.score(_engine, translit.load(EVAL / "chat-golden.tsv"))
    out["chat_golden"] = {"top1": s.top1, "top3": s.top3}
    lines = (EVAL / "english-sentences.txt").read_text(encoding="utf-8").splitlines()
    out["english_wrong"], _ = english.wrong_replacements(_engine, [l for l in lines if l.strip()])
    return out


def objective(m: dict) -> float:
    """The mean of the gated tune metrics (higher is better)."""
    values = [m[name][k] for name in NEXT_SETS for k in ("top3", "ks")]
    values += [m[name]["top1"] for name in CONTEXT_SETS]
    return sum(values) / len(values)


def eligible(m: dict, m3a: dict) -> bool:
    """No more English wrong replacements than M3a, and chat_golden within the tolerance."""
    base = m3a["chat_golden"]
    return (m["english_wrong"] <= m3a["english_wrong"]
            and all(m["chat_golden"][k] >= base[k] - ACCURACY_TOLERANCE for k in ("top1", "top3")))


def row(m: dict, m3a: dict) -> str:
    nxt = "  ".join(f"{m[n]['top3']:.4f}/{m[n]['ks']:.4f}" for n in NEXT_SETS)
    ctx = "  ".join(f"{m[n]['top1']:.4f}" for n in CONTEXT_SETS)
    chat = f"{m['chat_golden']['top1']:.4f}/{m['chat_golden']['top3']:.4f}"
    flag = "" if eligible(m, m3a) else "  (not eligible)"
    point = " ".join(f"{k}={v:g}" for k, v in m["point"].items())
    return f"{objective(m):.4f}  {nxt}  {ctx}  chat {chat}  wrong {m['english_wrong']}  {point}{flag}"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="python -m mlkb.eval.sweep", description=__doc__.splitlines()[0])
    parser.add_argument("--lm", required=True, metavar="PATH")
    parser.add_argument("--translit", required=True, metavar="PATH")
    parser.add_argument("--fields", default=",".join(GRID), help="fields to sweep, in order")
    parser.add_argument("--start", default="", help="starting point, e.g. ml=0.25,en=0.5")
    parser.add_argument("--jobs", type=int, default=6)
    args = parser.parse_args(argv)

    m3a = json.loads(BASELINES_M3A.read_text(encoding="utf-8"))
    best = {k: float(v) for k, v in (kv.split("=") for kv in args.start.split(",") if kv)}
    best_m = None
    print("objective  next_en/ml/manglish top3/ks  typos_ctx  lang_choice  chat top1/top3  wrong  point")
    with ProcessPoolExecutor(args.jobs, initializer=_init, initargs=(args.translit, args.lm)) as pool:
        for field in args.fields.split(","):
            points = [{**best, field: v} for v in GRID[field]]
            results = list(pool.map(measure, points))
            for m in results:
                print(row(m, m3a), flush=True)
            # Eligible points first; among the others, the one nearest to eligible.
            best_m = max(results, key=lambda m: (
                eligible(m, m3a), -max(0, m["english_wrong"] - m3a["english_wrong"]),
                min(0.0, m["chat_golden"]["top1"] - m3a["chat_golden"]["top1"] + ACCURACY_TOLERANCE),
                objective(m)))
            best = best_m["point"]
            print(f"-- after {field}: {' '.join(f'{k}={v:g}' for k, v in best.items())}", flush=True)
    print(f"best: {row(best_m, m3a)}")
    return 0 if eligible(best_m, m3a) else 1


if __name__ == "__main__":
    sys.exit(main())
