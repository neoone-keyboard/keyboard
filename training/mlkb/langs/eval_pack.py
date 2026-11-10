"""Held-out accuracy of a pack as the engine uses it, beside its transliterator alone, and for
each word the engine gets wrong, the stage that dropped it.

    uv run --group train python -m mlkb.langs.eval_pack --lang ta
    uv run --group train python -m mlkb.langs.eval_pack --lang as --misses build/as/misses.tsv

Reads `data/eval/<lang>/translit-human.tsv` (context, latin, native) and types each Latin word
into the engine loaded from `packs/<lang>.neopack` as its manifest lists the files (English
beside it, as on the keyboard; no deadline, so the numbers don't depend on the machine).

Engine numbers:
  first native  the first native-script suggestion of the three shown is the target
  in the bar    the target is one of the three
  middle        the target is the middle slot, what space commits (`mlkb.eval`'s top-1)
  in 20         the target is anywhere in the scrolling bar

Model numbers are `eval_translit`'s: the model alone, beam `--beam`, no lexicon.

Stages, for a row whose first native suggestion is not the target (first match wins):
  normal-form        the target is not in the engine's normal form: it can never be offered
  model-miss         the model alone doesn't have it in its beam either
  shown-second       it is in the bar, behind another native word
  off-bar            it is a candidate, but past the three slots
  english-word       the typed letters are an English word: the transliterator isn't asked
  below-floor        a lexicon word the constrained beam found, under `rank::MANGLISH_MODEL_FLOOR`
  beam-width         a lexicon word the constrained beam finds only when wider than the engine's
  beam-miss          a lexicon word the constrained beam doesn't find at any width
  free-not-offered   not a lexicon word; the free beam spells it, but a lexicon word was found
                     for the letters, so the free spelling isn't offered
  free-differs       not a lexicon word, and the engine's free beam (one hypothesis) spells
                     another word than the model's beam does
  other              none of the above
"""

import argparse
import hashlib
import json
from collections import Counter
from pathlib import Path

import mlengine

ROOT = Path(__file__).resolve().parents[3]
ENGLISH = ROOT / "apps/ios/KeyboardExtension/Lexicon/en.fst"
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
    "below-floor",
    "beam-width",
    "beam-miss",
    "free-not-offered",
    "free-differs",
    "other",
)


def rows(path: Path) -> list[tuple[str, str, str]]:
    out = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.startswith("#") or not line.strip():
            continue
        context, latin, target = line.split("\t")
        out.append((context, latin, target))
    return out


def load_engine(pack: Path, model: bool = True, lm: bool = True) -> "mlengine.Engine":
    """The engine on English and the pack at `pack`, each file where its manifest lists it."""
    manifest = json.loads((pack / "manifest.json").read_text(encoding="utf-8"))
    files = manifest["files"]

    def role(name: str) -> str | None:
        return str(pack / files[name]) if name in files else None

    engine = mlengine.Engine(
        english=str(ENGLISH),
        malayalam=role("lexicon"),
        manglish=role("romanisedLexicon"),
        romanisation=role("romanisation"),
        suffixes=role("suffixes"),
        chat_forms=role("chatForms"),
        layout=role("layout"),
        next_words=role("nextWords"),
        translit=role("translit") if model else None,
        lm=role("lm") if lm else None,
        lang=role("lang"),
        pack=manifest["id"],
        script=manifest["script"],
    )
    engine.set_max_suggestions(20)
    return engine


def model_alone(lang: str, model_path: Path, words: list[str], width: int) -> dict[str, list[str]]:
    """The model's beam for each word, without the lexicon; cached beside the build by the
    model file's hash."""
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


def stage(engine, latin: str, target: str, bar: list[tuple[str, str]], beam: list[str]) -> str:
    """Why the first native suggestion for `latin` isn't `target` (see the module's list)."""
    if engine.normalize(target) != target:
        return "normal-form"
    if target not in beam:
        return "model-miss"
    texts = [text for text, _ in bar]
    if target in texts[:3]:
        return "shown-second"
    if target in texts:
        return "off-bar"
    if engine.is_english(latin):
        return "english-word"
    held = engine.model(latin, MODEL_WIDTH, True)
    if engine.lexicon_log_prob(target) is not None:
        score = next((s for text, s, known in held if known and text == target), None)
        if score is not None:
            return "below-floor" if score < MODEL_FLOOR else "other"
        wide = engine.model(latin, WIDEST, False)
        return "beam-width" if any(text == target for text, _, _ in wide) else "beam-miss"
    free = next((text for text, _, known in held if not known), None)
    if free != target:
        return "free-differs"
    whole = any(missing == 0 for _, _, missing, _, _ in engine.lookup(latin))
    read = any(known and s >= MODEL_FLOOR for _, s, known in held)
    return "free-not-offered" if whole or read else "other"


def main(argv: list[str] | None = None) -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--lang", required=True)
    p.add_argument("--pack", type=Path, help="the pack folder (default packs/<lang>.neopack)")
    p.add_argument("--rows", type=Path, help="the eval set (default data/eval/<lang>/translit-human.tsv)")
    p.add_argument("--beam", type=int, default=8)
    p.add_argument("--limit", type=int, default=0, help="first N rows only (0: all)")
    p.add_argument("--no-lm", action="store_true", help="leave the pack's LM out")
    p.add_argument("--misses", type=Path, help="write each miss and its stage here (TSV)")
    args = p.parse_args(argv)
    pack = args.pack or ROOT / f"packs/{args.lang}.neopack"
    data = rows(args.rows or ROOT / f"data/eval/{args.lang}/translit-human.tsv")
    if args.limit:
        data = data[: args.limit]
    manifest = json.loads((pack / "manifest.json").read_text(encoding="utf-8"))
    engine = load_engine(pack, lm=not args.no_lm)
    without = load_engine(pack, model=False, lm=not args.no_lm)
    beams = model_alone(args.lang, pack / manifest["files"]["translit"], [r[1] for r in data], args.beam)

    n = len(data)
    count: Counter[str] = Counter()
    # Stage counts: all misses; those the model alone has first; those it has in its top three.
    stages = {k: Counter() for k in ("all", "model1", "model3")}
    misses = []
    for context, latin, target in data:
        query = f"{context} {latin}" if context else latin
        beam = beams[latin]
        count["model top-1"] += beam[:1] == [target]
        count["model top-3"] += target in beam[:3]
        count["in lexicon"] += engine.lexicon_log_prob(target) is not None
        for name, e in (("", engine), ("no model: ", without)):
            bar = [(text, kind) for text, kind, _ in e.suggest(query, "latin", True)]
            shown = bar[:3]
            native = [text for text, kind in shown if kind == "manglish"]
            count[name + "first native"] += native[:1] == [target]
            count[name + "in the bar"] += any(text == target for text, _ in shown)
            count[name + "middle"] += shown[:1] == [(target, "manglish")]
            count[name + "in 20"] += any(text == target for text, _ in bar)
            count[name + "no native shown"] += not native
        bar = [(text, kind) for text, kind, _ in engine.suggest(query, "latin", True)]
        native = [text for text, kind in bar[:3] if kind == "manglish"]
        if native[:1] == [target]:
            count["engine right, model wrong"] += beam[:1] != [target]
            continue
        s = stage(engine, latin, target, bar, beam)
        stages["all"][s] += 1
        if beam[:1] == [target]:
            stages["model1"][s] += 1
        if target in beam[:3]:
            stages["model3"][s] += 1
        misses.append((s, latin, target, " ".join(t for t, _ in bar[:3]), " ".join(beam[:3])))

    def pct(key: str) -> str:
        return f"{count[key] / n:.1%}"

    print(f"{pack.name}: {n:,} held-out pairs ({pct('in lexicon')} of targets in the lexicon)")
    print(f"model alone (beam {args.beam}):  top-1 {pct('model top-1')}  top-3 {pct('model top-3')}")
    for name, label in (("no model: ", "engine, no model"), ("", "engine")):
        print(
            f"{label + ':':<24} first native {pct(name + 'first native')}  in the bar {pct(name + 'in the bar')}"
            f"  middle {pct(name + 'middle')}  in 20 {pct(name + 'in 20')}"
            f"  no native shown {pct(name + 'no native shown')}"
        )
    print(f"engine right where the model alone is wrong: {count['engine right, model wrong']:,}")
    print(f"\n{'stage':<18}{'all misses':>12}{'model top-1':>13}{'model top-3':>13}")
    for s in STAGES:
        print(f"{s:<18}{stages['all'][s]:>12,}{stages['model1'][s]:>13,}{stages['model3'][s]:>13,}")
    totals = [sum(stages[k].values()) for k in ("all", "model1", "model3")]
    print(f"{'total':<18}{totals[0]:>12,}{totals[1]:>13,}{totals[2]:>13,}")
    if args.misses:
        args.misses.parent.mkdir(parents=True, exist_ok=True)
        lines = ["stage\tlatin\ttarget\tbar\tmodel"] + ["\t".join(m) for m in misses]
        args.misses.write_text("\n".join(lines) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
