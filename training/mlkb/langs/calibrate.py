"""Fits a language's romanisation weights to human pairs, and measures how well the tables match.

    uv run python -m mlkb.langs.calibrate --lang as            # report only
    uv run python -m mlkb.langs.calibrate --lang as --write    # write data/langs/as/romanisation-weights.json

The hand-set tables are `data/langs/<lang>/romanisation.hand.json`; the fitted ones are
`romanisation-weights.json` next to it. `mlkb.langs.format1` exports their canonical spellings
as the pack's `romanisation.json`.

Each pair (native word, Latin as a person typed it) is aligned to the word's slots
(`Tables.slots`): the most likely choice of one spelling per slot that spells the Latin exactly,
under the current weights (every weight floored at `ALIGN_FLOOR` while aligning). Each slot's
chosen spelling is a count for its (table, letter); a doubled letter's choice counts towards
`geminateSingle`. New weights are the counts smoothed towards the old ones with `PRIOR_WEIGHT`
pseudo-counts, floored at `FIT_MIN`, and each option list is reordered best first, so the
canonical spelling is the one people type most. Alignment and fitting repeat `ITERATIONS` times
(hard EM). Every spelling is kept, so a rare one stays possible. The skeleton table is not fitted.

The report, on the calibration pairs and on the held-out test pairs:
- canonical: the canonical romanisation is exactly what was typed;
- producible: the tables can spell what was typed (it aligns);
- skeleton: the typed Latin has the word's lookup key as its skeleton (`Tables.key`), so the
  romanised lookup finds it as an exact match;
- prefix: the typed skeleton is a prefix of the word's key, so the lookup finds it as a completion.

Pairs: Aksharantar's manual rows (AK-Uni, AK-Freq: typed by people) in `<lang>_valid.json`
calibrate; the manual rows of `<lang>_test.json` (AK-Freq, AK-NEI, AK-NEF) are the test.
"""

import argparse
import json
import math
from collections import Counter, defaultdict
from pathlib import Path

from mlkb.langs import human
from mlkb.langs.romanisation import LANGS, Slot, Tables

PRIOR_WEIGHT = 20.0
ALIGN_FLOOR = 0.02
FIT_MIN = 0.01
ITERATIONS = 3


def align(slots: tuple[Slot, ...], latin: str, floor: float = ALIGN_FLOOR) -> list[int] | None:
    """The option index chosen in each slot by the most likely alignment, or None."""
    n, m = len(latin), len(slots)
    neg = -math.inf
    best = [[neg] * (n + 1) for _ in range(m + 1)]
    back: list[list[tuple[int, int] | None]] = [[None] * (n + 1) for _ in range(m + 1)]
    best[0][0] = 0.0
    for k, slot in enumerate(slots):
        total = sum(max(w, floor) for _, w in slot.options)
        for p in range(n + 1):
            if best[k][p] == neg:
                continue
            for o, (spelling, w) in enumerate(slot.options):
                if latin.startswith(spelling, p):
                    q = p + len(spelling)
                    score = best[k][p] + math.log(max(w, floor) / total)
                    if score > best[k + 1][q]:
                        best[k + 1][q] = score
                        back[k + 1][q] = (p, o)
    if best[m][n] == neg:
        return None
    out = [0] * m
    p = n
    for k in range(m, 0, -1):
        prev, o = back[k][p]  # type: ignore[misc]
        out[k - 1] = o
        p = prev
    return out


def counts(tables: Tables, pairs: list[tuple[str, str]]) -> tuple[dict, Counter, int]:
    """Chosen-option counts per (table, key), geminate double/single counts, and unaligned pairs."""
    chosen: dict[tuple[str, str], Counter] = defaultdict(Counter)
    geminate: Counter = Counter()
    failed = 0
    for native, latin in pairs:
        slots = tables.slots(native)
        picks = align(slots, latin)
        if picks is None:
            failed += 1
            continue
        for slot, o in zip(slots, picks, strict=True):
            spelling = slot.options[o][0]
            if slot.table == "geminate":
                geminate["single" if o % 2 else "double"] += 1
            elif slot.table != "latin":
                chosen[(slot.table, slot.key)][spelling] += 1
    return chosen, geminate, failed


def _fit(options: list, seen: Counter) -> list:
    """Smoothed weights for one option list, best first."""
    total = sum(seen.values())
    prior = sum(w for _, w in options) or 1.0
    fitted = []
    for spelling, w in options:
        p = (seen.get(spelling, 0) + PRIOR_WEIGHT * w / prior) / (total + PRIOR_WEIGHT)
        fitted.append([spelling, round(max(p, FIT_MIN), 3)])
    fitted.sort(key=lambda x: -x[1])
    return fitted


def fit(tables: Tables, pairs: list[tuple[str, str]]) -> tuple[Tables, int]:
    """New tables with weights fitted to `pairs` (one EM iteration), and unaligned pairs."""
    chosen, geminate, failed = counts(tables, pairs)
    data = json.loads(json.dumps(tables.data))
    for (table, key), seen in chosen.items():
        if table.startswith("inherent."):
            node, name = data["inherent"], table.split(".", 1)[1]
        elif table == "apostropheVowel":
            data["apostropheVowel"] = _fit(data["apostropheVowel"], seen)
            continue
        else:
            node, name = data[table], key
        node[name] = _fit(node[name], seen)
    if geminate:
        g = float(data.get("geminateSingle", 0.0))
        total = sum(geminate.values())
        data["geminateSingle"] = round((geminate["single"] + PRIOR_WEIGHT * g) / (total + PRIOR_WEIGHT), 3)
    return Tables(data, tables.path), failed


def report(tables: Tables, pairs: list[tuple[str, str]]) -> dict[str, float]:
    n = max(len(pairs), 1)
    canonical = producible = skeleton = prefix = 0
    for native, latin in pairs:
        key = tables.key(native)
        typed = tables.skeleton(latin)
        canonical += tables.canonical(native) == latin
        producible += align(tables.slots(native), latin) is not None
        skeleton += typed == key
        prefix += bool(typed) and key.startswith(typed)
    return {
        "pairs": len(pairs),
        "canonical": canonical / n,
        "producible": producible / n,
        "skeleton": skeleton / n,
        "prefix": prefix / n,
    }


def _print(label: str, r: dict) -> None:
    print(
        f"{label:28} {r['pairs']:6,} pairs  canonical {r['canonical']:.1%}  producible {r['producible']:.1%}  "
        f"skeleton {r['skeleton']:.1%}  skeleton or prefix {r['prefix']:.1%}"
    )


def main(argv: list[str] | None = None) -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--lang", default="as")
    p.add_argument("--tables", type=Path, help="hand-set tables (default data/langs/<lang>/romanisation.hand.json)")
    p.add_argument("--write", action="store_true", help="write the fitted weights back")
    p.add_argument("--iterations", type=int, default=ITERATIONS)
    args = p.parse_args(argv)
    hand = args.tables or LANGS / args.lang / "romanisation.hand.json"
    tables = Tables.load(hand)
    calib = [(h.native, h.latin) for h in human.aksharantar(args.lang, "valid") if h.source.startswith("AK-")]
    test = [h for h in human.aksharantar(args.lang, "test") if h.source.startswith("AK-")]
    by_source: dict[str, list[tuple[str, str]]] = defaultdict(list)
    for h in test:
        by_source[h.source].append((h.native, h.latin))
    all_test = [pair for v in by_source.values() for pair in v]

    _print("calibration (before)", report(tables, calib))
    _print("test (before)", report(tables, all_test))
    for i in range(args.iterations):
        tables, failed = fit(tables, calib)
        print(f"iteration {i + 1}: {failed:,} of {len(calib):,} calibration pairs did not align")
    _print("calibration (after)", report(tables, calib))
    _print("test (after)", report(tables, all_test))
    for source, pairs in sorted(by_source.items()):
        _print(f"  test {source}", report(tables, pairs))
    if args.write:
        path = hand.with_name("romanisation-weights.json")
        tables.data["about"] = tables.data["about"].replace("hand-set", "fitted")
        path.write_text(_dumps(tables.data) + "\n", encoding="utf-8")
        print(f"wrote {path}")


def _dumps(data: dict) -> str:
    """JSON with one table entry per line, like the hand-written file."""
    lines = ["{"]
    items = list(data.items())
    for i, (k, v) in enumerate(items):
        comma = "," if i + 1 < len(items) else ""
        if isinstance(v, dict) and v and all(isinstance(x, list) for x in v.values()):
            inner = ",\n".join(f"    {json.dumps(kk, ensure_ascii=False)}: {json.dumps(vv, ensure_ascii=False)}" for kk, vv in v.items())
            lines.append(f"  {json.dumps(k)}: {{\n{inner}\n  }}{comma}")
        else:
            body = json.dumps(v, ensure_ascii=False, indent=2).replace("\n", "\n  ")
            lines.append(f"  {json.dumps(k)}: {body}{comma}")
    lines.append("}")
    return "\n".join(lines)


if __name__ == "__main__":
    main()
