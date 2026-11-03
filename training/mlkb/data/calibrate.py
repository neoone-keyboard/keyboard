"""Fits the romaniser's rule probabilities to human romanisations.

    uv run python -m mlkb.data.calibrate   # writes training/mlkb/data/rules-calibrated.json

Each human pair (Malayalam word, Latin as a person typed it) is aligned to the word's slots
(`romanise._slots`): the most likely choice of one spelling per slot that spells the Latin
exactly, under the current probabilities. Each slot then gives evidence for the rules it
depends on: an *opportunity* (the rule could have fired) and whether it *fired* (the spelling
the person used is the rule's). A rule's new probability is fired / opportunities, smoothed
towards the hand-set value with `PRIOR_WEIGHT` pseudo-counts. Alignment and counting repeat
`ITERATIONS` times (hard EM). Pairs no slot spelling can produce are skipped and counted.

Not fitted: the medial vowel drop, `variant_counts` and `canonical_share` (how many spellings a
draw gives), which aren't spelling rules a pair can show.

Calibrated on the Dakshina train split only: its romanisations are typed by people. The
Aksharantar IndicCorp rows are mined with a model and would fit the model's habits instead.
"""

import argparse
import json
import math
from collections import defaultdict
from collections.abc import Iterable
from dataclasses import asdict, fields, replace
from pathlib import Path

from mlkb.data import human
from mlkb.data.romanise import CONSONANTS, RULES, Rules, Slot, _slots

CALIBRATED = Path(__file__).with_name("rules-calibrated.json")

# Pseudo-counts of the hand-set value mixed into each estimate, so rare rules stay near it.
PRIOR_WEIGHT = 20.0
# Every probability is at least this while aligning, so every spelling stays possible.
ALIGN_FLOOR = 0.02
# A fitted probability is kept in [FIT_MIN, FIT_MAX]: no spelling becomes impossible or certain.
FIT_MIN, FIT_MAX = 0.01, 0.95
ITERATIONS = 3

NOT_FITTED = frozenset({"medial_vowel_drop", "variant_counts", "canonical_share"})

# Malayalam source of each vowel slot → the rules its spelling shows: (rule, spellings that fire it).
VOWEL_RULES: dict[str, list[tuple[str, frozenset[str]]]] = {}
for _src, _rules in {
    "ാആ": [("long_a_short", {"a"})],
    "ീഈൟ": [("long_i_short", {"i"})],
    "ൂഊ": [("long_u_short", {"u"})],
    "േഏ": [("long_e_short", {"e", "ey"}), ("e_as_ae", {"ae"})],
    "ോഓ": [("long_o_short", {"o"})],
    "ൃഋ": [("ru_as_ri", {"ri"}), ("ru_as_r", {"r"})],
    "ൈഐ": [("ai_as_ei", {"ei"})],
    "ൌൗഔ": [("au_as_ou", {"ou"})],
}.items():
    for _c in _src:
        VOWEL_RULES[_c] = [(name, frozenset(s)) for name, s in _rules]

# Two-letter conjuncts → their rules, as for vowels.
SPECIAL_RULES: dict[str, list[tuple[str, frozenset[str]]]] = {
    k: [(name, frozenset(s)) for name, s in v]
    for k, v in {
        "നറ": [("nt_as_nd", {"nd"}), ("nt_as_nth", {"nth"})],
        "ററ": [("geminate_single", {"t"})],
        "നത": [("aspiration_drop", {"nt"})],
        "നധ": [("aspiration_drop", {"nd"})],
        "കഷ": [("ksh_as_x", {"x"}), ("ksh_as_ks", {"ks"})],
        "കസ": [("ks_as_x", {"x"})],
        "ഞച": [("nch_as_nj", {"nj"})],
        "ങക": [("nk_as_ng", {"ng"})],
        "ണട": [("nd_as_nt", {"nt"})],
        "മപ": [("mp_as_mb", {"mb"})],
        "ചച": [("cch_double", {"cch"}), ("cch_chch", {"chch"})],
        "ചഛ": [("chh_as_ch", {"ch"})],
        "തത": [("tth_as_th", {"th"}), ("tth_as_tt", {"tt"})],
        "തഥ": [("tth_as_th", {"th"}), ("tth_as_tt", {"tt"})],
        "ദധ": [("geminate_single", {"dh"})],
        "ശശ": [("shsh_as_sh", {"sh"}), ("shsh_as_ssh", {"ssh"}), ("shsh_as_s", {"s"})],
        "ജഞ": [("jn_as_gn", {"gn"}), ("jn_as_nj", {"nj"})],
    }.items()
}

Evidence = dict[str, list[float]]  # rule → [fired, opportunities]


def _note(ev: Evidence, rule: str, fired: bool, weight: float) -> None:
    ev[rule][0] += weight * fired
    ev[rule][1] += weight


def _consonant_evidence(letter: str, latin: str, ev: Evidence, w: float, after_vowel: bool, options: int) -> None:
    """Evidence from one consonant letter spelled `latin` (undoubled)."""
    match letter:
        case "ഖ":
            _note(ev, "aspiration_drop", latin in ("k", "g"), w)
            _note(ev, "kh_as_gh", latin in ("gh", "g"), w)
        case "ഘ" | "ഛ" | "ഝ" | "ഢ" | "ധ" | "ഭ" | "ത":
            _note(ev, "aspiration_drop", latin == CONSONANTS[letter][:-1], w)
        case "ഠ":
            _note(ev, "aspiration_drop", latin in ("t", "d"), w)
            if latin in ("t", "d"):
                _note(ev, "ttha_voiced", latin == "d", w)
        case "ഥ":
            _note(ev, "aspiration_drop", latin in ("t", "d"), w)
            _note(ev, "tha_as_dha", latin in ("dh", "d"), w)
        case "ഫ":
            _note(ev, "pha_as_f", latin == "f", w)
            if latin != "f":
                _note(ev, "aspiration_drop", latin == "p", w)
        case "ട" if after_vowel and options > 1:
            _note(ev, "tta_voiced", latin == "d", w)
        case "ഴ":
            _note(ev, "zh_as_l", latin == "l", w)
            _note(ev, "zh_as_z", latin == "z", w)
        case "വ":
            _note(ev, "v_as_w", latin == "w", w)
        case "ശ":
            _note(ev, "sha_as_s", latin == "s", w)
        case "ഷ":
            _note(ev, "ssa_as_s", latin == "s", w)


def slot_evidence(slots: tuple[Slot, ...], picked: list[str], ev: Evidence, w: float) -> None:
    """Adds the evidence of one aligned word: `picked[k]` is how slot k was spelled."""
    last = len(slots) - 1
    for k, (slot, latin) in enumerate(zip(slots, picked, strict=True)):
        if slot.tag == "e-drop":
            _note(ev, "initial_e_drop", latin == "", w)
            continue
        if slot.tag == "y-drop":
            _note(ev, "y_drop_between_vowels", latin == "", w)
            if latin == "":
                continue
        if k == last and slot.kind == "V" and latin in ("e", "ey"):
            _note(ev, "final_e_ey", latin == "ey", w)
        src = slot.src
        if slot.kind == "V":
            for rule, fire in VOWEL_RULES.get(src, []):
                _note(ev, rule, latin in fire, w)
            if src == "a" and len(slot.spellings) > 1:
                _note(ev, "sha_vowel_e", latin == "e", w)
            if src == "ി" and len(slot.spellings) > 1:
                _note(ev, "final_i_y", latin == "y", w)
        elif slot.kind == "U":
            rule = (
                "final_u_drop_sibilant"
                if len(src) == 1 and src in "ശഷസ"
                else "final_u_drop_single"
                if len(src) == 1
                else "final_u_drop_cluster"
            )
            _note(ev, rule, latin == "", w)
        elif slot.kind == "F":
            if src == "ം" and len(slot.spellings) > 1:
                _note(ev, "anusvara_as_n", latin == "n", w)
            elif src == "ഃ":
                _note(ev, "visarga_drop", latin == "", w)
            elif src == "ൖ":
                _consonant_evidence("ഴ", latin, ev, w, False, len(slot.spellings))
        elif slot.kind == "C":
            after_vowel = k > 0 and slots[k - 1].kind == "V"
            if len(src) == 1:
                _consonant_evidence(src, latin, ev, w, after_vowel, len(slot.spellings))
            elif src in SPECIAL_RULES:
                for rule, fire in SPECIAL_RULES[src]:
                    _note(ev, rule, latin in fire, w)
            elif len(src) == 2 and src[0] == src[1]:  # a geminate: doubled or single
                doubled = len(latin) >= 2 and latin[0] == latin[1]
                _note(ev, "geminate_single", not doubled, w)
                _consonant_evidence(src[0], latin[1:] if doubled else latin, ev, w, False, 2)


def _options(slot: Slot, k: int, last: int, r: Rules) -> list[tuple[str, float]]:
    """(spelling, log probability) for one slot, including the cross-slot rules `_sample` applies."""
    probs = [b - a for a, b in zip((0.0, *slot.cumulative[:-1]), slot.cumulative, strict=True)]
    out = [(s, p) for s, p in zip(slot.spellings, probs, strict=True)]
    drop = {"e-drop": r.initial_e_drop, "y-drop": r.y_drop_between_vowels}.get(slot.tag)
    if drop is not None:
        out = [(s, p * (1 - drop)) for s, p in out] + [("", drop)]
    if k == last and slot.kind == "V":
        extra = [(s + "y", p * r.final_e_ey) for s, p in out if s == "e"]
        out = [(s, p * (1 - r.final_e_ey) if s == "e" else p) for s, p in out] + extra
    return [(s, math.log(p)) for s, p in out if p > 0]


def align(slots: tuple[Slot, ...], latin: str, r: Rules) -> list[str] | None:
    """The most likely spelling of each slot that together spell `latin`, or None."""
    n, last = len(latin), len(slots) - 1
    best: list[dict[int, tuple[float, int, str]]] = [{0: (0.0, -1, "")}]
    for k, slot in enumerate(slots):
        nxt: dict[int, tuple[float, int, str]] = {}
        opts = _options(slot, k, last, r)
        for pos, (score, _, _) in best[k].items():
            for s, lp in opts:
                end = pos + len(s)
                if end <= n and latin.startswith(s, pos):
                    cand = score + lp
                    if end not in nxt or cand > nxt[end][0]:
                        nxt[end] = (cand, pos, s)
        if not nxt:
            return None
        best.append(nxt)
    if n not in best[-1]:
        return None
    picked, pos = [], n
    for k in range(len(slots), 0, -1):
        _, prev, s = best[k][pos]
        picked.append(s)
        pos = prev
    return picked[::-1]


def _floored(r: Rules) -> Rules:
    return replace(r, **{f.name: max(getattr(r, f.name), ALIGN_FLOOR) for f in fields(r) if f.type is float})


def fit(pairs: Iterable[tuple[str, str, float]], prior: Rules = RULES, iterations: int = ITERATIONS):
    """Calibrated rules from (latin, malayalam, weight) pairs, the evidence, and parse stats."""
    pairs = list(pairs)
    rules = prior
    ev: Evidence = {}
    stats: dict[str, float] = {}
    for _ in range(iterations):
        probe = _floored(rules)
        ev = defaultdict(lambda: [0.0, 0.0])
        parsed = skipped = 0
        for latin, word, w in pairs:
            slots = _slots(word, probe)
            picked = align(slots, latin, probe) if slots else None
            if picked is None:
                skipped += 1
                continue
            parsed += 1
            slot_evidence(slots, picked, ev, w)
        stats = {"pairs": len(pairs), "aligned": parsed, "skipped": skipped}
        updates = {}
        for f in fields(prior):
            if f.name in NOT_FITTED or f.type is not float:
                continue
            fired, opp = ev.get(f.name, (0.0, 0.0))
            p = (fired + PRIOR_WEIGHT * getattr(prior, f.name)) / (opp + PRIOR_WEIGHT)
            updates[f.name] = round(min(max(p, FIT_MIN), FIT_MAX), 4)
        rules = replace(prior, **updates)
    return rules, dict(ev), stats


def save(rules: Rules, ev: Evidence, stats: dict, path: Path = CALIBRATED) -> None:
    """Writes the rules with the evidence behind each one."""
    doc = {
        "about": "Romaniser rule probabilities fitted to Dakshina train romanisations by mlkb.data.calibrate.",
        "stats": stats,
        "rules": {k: list(v) if isinstance(v, tuple) else v for k, v in asdict(rules).items()},
        "evidence": {k: {"fired": round(v[0], 1), "opportunities": round(v[1], 1)} for k, v in sorted(ev.items())},
    }
    path.write_text(json.dumps(doc, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def load(path: Path = CALIBRATED) -> Rules:
    """The calibrated rules from `path`; fields it doesn't name keep their defaults."""
    data = json.loads(path.read_text(encoding="utf-8"))["rules"]
    known = {f.name for f in fields(Rules)}
    kwargs = {k: tuple(v) if isinstance(v, list) else v for k, v in data.items() if k in known}
    return Rules(**kwargs)


def main(argv: list[str] | None = None) -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--out", type=Path, default=CALIBRATED)
    args = p.parse_args(argv)
    train = human.dakshina("train")
    rules, ev, stats = fit((h.latin, h.malayalam, float(h.count)) for h in train)
    save(rules, ev, stats, args.out)
    print(f"aligned {stats['aligned']:,} of {stats['pairs']:,} pairs; skipped {stats['skipped']:,}")
    for f in fields(Rules):
        before, after = getattr(RULES, f.name), getattr(rules, f.name)
        if before != after:
            fired, opp = ev.get(f.name, (0, 0))
            print(f"  {f.name:24} {before:<6} -> {after:<6}  ({fired:.0f}/{opp:.0f})")
    print(f"wrote {args.out}")


if __name__ == "__main__":
    main()
