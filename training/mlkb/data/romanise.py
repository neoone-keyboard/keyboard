"""Malayalam → Manglish: one canonical romanisation, and random spellings the way people type.

`canonical` spells every unit fully: long vowels doubled (aa ee oo, ോ is "oo" and േ is
"ee"), doubled consonants doubled, a final virama as "u". `variants` samples the
spellings people actually type in chat (see `data/eval/chat-golden.tsv`): short vowels
for long ones, dropped aspiration and doubling, ട as "d", ന്റ as "nd", a dropped final
"u", and so on. Every probability is a field of `Rules`, so the human-pair calibration
can replace them.

A word is first split into slots (a consonant piece, a vowel, a final letter such as a
chillu or anusvara, or the "u" of a final virama). Each slot has weighted spellings,
canonical first; a variant picks one spelling per slot, then applies the few rules that
look across slots (a dropped initial e, a dropped medial vowel, y between vowels, final
e as "ey").
"""

import random
from dataclasses import dataclass
from functools import lru_cache
from typing import NamedTuple


@dataclass(frozen=True)
class Rules:
    """How often each spelling rule fires. Defaults are set by hand from chat-golden."""

    # Vowel length dropped: ാ aa→a, ീ ee→i, ൂ oo→u, േ ee→e, ോ oo→o.
    long_a_short: float = 0.75
    long_i_short: float = 0.45
    long_u_short: float = 0.45
    long_e_short: float = 0.9
    long_o_short: float = 0.88
    # േ / ഏ written "ae" (aedo, chaechi).
    e_as_ae: float = 0.03
    # Aspiration dropped: kh→k, gh→g, th→t, dh→d, ph→p, bh→b, chh→ch, jh→j.
    aspiration_drop: float = 0.3
    # ഖ written "gh", as in sugham.
    kh_as_gh: float = 0.15
    # ഫ written "f".
    pha_as_f: float = 0.35
    # ഥ written "dh", as in kadha.
    tha_as_dha: float = 0.15
    # A doubled consonant made single (enikku→eniku, kollam→kolam).
    geminate_single: float = 0.5
    # ത്ത: "th" and "tt" for the canonical "tth".
    tth_as_th: float = 0.55
    tth_as_tt: float = 0.15
    # ച്ച: "cch" and "chch" for the canonical "ch".
    cch_double: float = 0.15
    cch_chch: float = 0.03
    # ച്ഛ written "ch" (achan) instead of "chh".
    chh_as_ch: float = 0.5
    # ശ്ശ: "ss" canonical (thrissur), else "sh", "ssh" or "s".
    shsh_as_sh: float = 0.2
    shsh_as_ssh: float = 0.15
    shsh_as_s: float = 0.1
    # ന്റ: "nt" canonical; "nd" (ende) and "nth" (enthe).
    nt_as_nd: float = 0.4
    nt_as_nth: float = 0.05
    # ണ്ട: "nd" canonical (undu); "nt".
    nd_as_nt: float = 0.15
    # മ്പ: "mp" or "mb" (ambalam).
    mp_as_mb: float = 0.5
    # ങ്ക: "nk" or "ng".
    nk_as_ng: float = 0.35
    # ഞ്ച: "nch" or "nj" (anju).
    nch_as_nj: float = 0.3
    # ക്ഷ: "ksh" canonical; "x" (laxmi) and "ks".
    ksh_as_x: float = 0.08
    ksh_as_ks: float = 0.08
    # ക്സ written "x".
    ks_as_x: float = 0.3
    # ജ്ഞ: "jn" canonical; "gn" and "nj".
    jn_as_gn: float = 0.2
    jn_as_nj: float = 0.3
    # A lone ട after a vowel written "d" (veedu, padam); anywhere else, a quarter of that.
    tta_voiced: float = 0.6
    # ഠ written "d" rather than "t" when its aspiration is dropped (padikkanam).
    ttha_voiced: float = 0.5
    # Final െ written "ey" (entey, athey).
    final_e_ey: float = 0.15
    # A final virama's "u" dropped: after a cluster (enik, und), a lone consonant (ath),
    # or a lone sibilant (suresh).
    final_u_drop_cluster: float = 0.3
    final_u_drop_single: float = 0.2
    final_u_drop_sibilant: float = 0.6
    # ഴ written "l" or "z".
    zh_as_l: float = 0.15
    zh_as_z: float = 0.05
    # ൃ: "ru" canonical; "ri" (krishnan, thrissur) and "r".
    ru_as_ri: float = 0.5
    ru_as_r: float = 0.08
    # വ written "w" (wayanad).
    v_as_w: float = 0.2
    # A lone യ between o / oo / aa and i / ee dropped (poyi→poi, aayi→aai).
    y_drop_between_vowels: float = 0.2
    # The inherent a after ശ / ഷ written "e" (sheri), and ശ / ഷ written "s".
    sha_vowel_e: float = 0.2
    sha_as_s: float = 0.3
    ssa_as_s: float = 0.15
    # ഐ / ൈ written "ei"; ഔ / ൌ written "ou".
    ai_as_ei: float = 0.08
    au_as_ou: float = 0.3
    # Final ി after a consonant other than യ written "y" (mammootty).
    final_i_y: float = 0.04
    # ം before a non-labial consonant written "n" (inglish, sangham).
    anusvara_as_n: float = 0.5
    # Visarga ഃ dropped.
    visarga_drop: float = 0.5
    # Initial എ dropped before ന്റ / ന്ത (nte, ntha).
    initial_e_drop: float = 0.12
    # One short vowel between consonants dropped, once per word (evde, njn).
    medial_vowel_drop: float = 0.05
    # How many spellings one `variants` call returns: weights for 1, 2, 3, 4.
    variant_counts: tuple[float, float, float, float] = (0.4, 0.3, 0.2, 0.1)
    # Chance that a call includes the canonical spelling outright.
    canonical_share: float = 0.1


RULES = Rules()
"""The default rule probabilities."""

VIRAMAS = frozenset("്഻഼")  # virama, vertical bar virama, circular virama
ZWJ, ZWNJ = "‍", "‌"

CONSONANTS = {
    "ക": "k", "ഖ": "kh", "ഗ": "g", "ഘ": "gh", "ങ": "ng",
    "ച": "ch", "ഛ": "chh", "ജ": "j", "ഝ": "jh", "ഞ": "nj",
    "ട": "t", "ഠ": "th", "ഡ": "d", "ഢ": "dh", "ണ": "n",
    "ത": "th", "ഥ": "th", "ദ": "d", "ധ": "dh", "ന": "n", "ഩ": "n",
    "പ": "p", "ഫ": "ph", "ബ": "b", "ഭ": "bh", "മ": "m",
    "യ": "y", "ര": "r", "റ": "r", "ല": "l", "ള": "l", "ഴ": "zh", "വ": "v",
    "ശ": "sh", "ഷ": "sh", "സ": "s", "ഹ": "h", "ഺ": "t",
}  # fmt: skip

# Vowel signs and independent vowels, by the key `_vowel` spells.
VOWEL_SIGNS = {
    "ാ": "aa", "ി": "i", "ീ": "ee", "ു": "u", "ൂ": "oo",
    "ൃ": "ru", "ൄ": "roo", "െ": "e", "േ": "EE", "ൈ": "ai",
    "ൊ": "o", "ോ": "OO", "ൌ": "au", "ൗ": "au",
    "ൢ": "lu", "ൣ": "loo",
}  # fmt: skip
INDEPENDENT_VOWELS = {
    "അ": "a", "ആ": "aa", "ഇ": "i", "ഈ": "ee", "ഉ": "u", "ഊ": "oo", "ഋ": "ru", "ൠ": "roo",
    "ഌ": "lu", "ൡ": "loo", "എ": "e", "ഏ": "EE", "ഐ": "ai", "ഒ": "o", "ഓ": "OO", "ഔ": "au",
    "ൟ": "ee",
}  # fmt: skip
# Two-part vowel signs written as their parts.
SIGN_PAIRS = {("െ", "ാ"): "ൊ", ("േ", "ാ"): "ോ", ("െ", "ൗ"): "ൌ"}

# Letters with no vowel of their own: chillus, anusvara, dot reph, visarga.
FINALS = {
    "ൺ": "n", "ൻ": "n", "ർ": "r", "ൽ": "l", "ൾ": "l", "ൿ": "k", "ൔ": "m", "ൕ": "y", "ൖ": "zh",
    "ം": "m", "ഀ": "m", "ഄ": "m", "ൎ": "r", "ഃ": "h",
}  # fmt: skip

SIBILANTS = frozenset("ശഷസ")

# The medial vowel drop is a chat abbreviation of short words (evde, njn): at most this many slots.
MEDIAL_DROP_MAX_SLOTS = 5

# Consonants after which ം keeps its "m"; before the others it may be written "n" (ingane, sankadam).
LABIALS = frozenset("പഫബഭമവ")

Options = tuple[tuple[str, float], ...]


class Slot(NamedTuple):
    """One unit of a word: its spellings (canonical first, with cumulative weights) and role."""

    kind: str  # "C" consonant piece, "V" vowel, "F" final letter, "U" a final virama's u
    spellings: tuple[str, ...]
    cumulative: tuple[float, ...]
    tag: str = ""  # "e-drop" (V), "y-drop" (C)
    lone: bool = False  # a C slot that is a single consonant letter, not part of a cluster
    src: str = ""  # the Malayalam letters or sign this slot spells (read by the calibration)


def _slot(kind: str, options: Options, tag: str = "", lone: bool = False, src: str = "") -> Slot:
    """A slot from (spelling, weight) options; zero-weight options after the first are dropped."""
    kept = [options[0]] + [(s, w) for s, w in options[1:] if w > 0]
    total = sum(max(w, 0.0) for _, w in kept) or 1.0
    acc, cumulative = 0.0, []
    for _, w in kept:
        acc += max(w, 0.0) / total
        cumulative.append(acc)
    return Slot(kind, tuple(s for s, _ in kept), tuple(cumulative), tag, lone, src)


def _consonant(letter: str, r: Rules, voicing: float = 0.0) -> Options:
    """Spellings of one consonant letter; `voicing` is the chance ട is written "d"."""
    a = r.aspiration_drop
    match letter:
        case "ഖ":
            g = r.kh_as_gh
            return (("kh", (1 - g) * (1 - a)), ("k", (1 - g) * a), ("gh", g * (1 - a)), ("g", g * a))
        case "ഘ" | "ഛ" | "ഝ" | "ഢ" | "ധ" | "ഭ" | "ത":
            full = CONSONANTS[letter]
            return ((full, 1 - a), (full[:-1], a))
        case "ഠ":
            v = r.ttha_voiced
            return (("th", 1 - a), ("t", a * (1 - v)), ("d", a * v))
        case "ഥ":
            q = r.tha_as_dha
            return (("th", (1 - q) * (1 - a)), ("t", (1 - q) * a), ("dh", q * (1 - a)), ("d", q * a))
        case "ഫ":
            f = r.pha_as_f
            return (("ph", (1 - f) * (1 - a)), ("p", (1 - f) * a), ("f", f))
        case "ട":
            return (("t", 1 - voicing), ("d", voicing))
        case "ഴ":
            return (("zh", 1 - r.zh_as_l - r.zh_as_z), ("l", r.zh_as_l), ("z", r.zh_as_z))
        case "വ":
            return (("v", 1 - r.v_as_w), ("w", r.v_as_w))
        case "ശ":
            return (("sh", 1 - r.sha_as_s), ("s", r.sha_as_s))
        case "ഷ":
            return (("sh", 1 - r.ssa_as_s), ("s", r.ssa_as_s))
        case _:
            return ((CONSONANTS[letter], 1.0),)


def _double(latin: str) -> str:
    """A spelling written doubled: k→kk, th→tth, sh→ssh."""
    return latin[0] + latin if latin else latin


def _geminate(letter: str, r: Rules) -> Options:
    """A letter doubled by a virama (ക്ക, ല്ല, ട്ട): doubled, or made single."""
    g = r.geminate_single
    out: list[tuple[str, float]] = []
    for latin, w in _consonant(letter, r):
        out += [(_double(latin), w * (1 - g)), (latin, w * g)]
    return tuple(out)


def _special(pair: tuple[str, str], r: Rules) -> Options | None:
    """Two-letter conjuncts typed as a unit."""
    a, g = r.aspiration_drop, r.geminate_single
    table: dict[tuple[str, str], Options] = {
        ("ന", "റ"): (("nt", 1 - r.nt_as_nd - r.nt_as_nth), ("nd", r.nt_as_nd), ("nth", r.nt_as_nth)),
        ("റ", "റ"): (("tt", 1 - g), ("t", g)),
        ("ന", "ത"): (("nth", 1 - a), ("nt", a)),
        ("ന", "ദ"): (("nd", 1.0),),
        ("ന", "ധ"): (("ndh", 1 - a), ("nd", a)),
        ("ക", "ഷ"): (("ksh", 1 - r.ksh_as_x - r.ksh_as_ks), ("x", r.ksh_as_x), ("ks", r.ksh_as_ks)),
        ("ക", "സ"): (("ks", 1 - r.ks_as_x), ("x", r.ks_as_x)),
        ("ഞ", "ഞ"): (("nj", 1.0),),
        ("ങ", "ങ"): (("ng", 1.0),),
        ("ഞ", "ച"): (("nch", 1 - r.nch_as_nj), ("nj", r.nch_as_nj)),
        ("ഞ", "ജ"): (("nj", 1.0),),
        ("ങ", "ക"): (("nk", 1 - r.nk_as_ng), ("ng", r.nk_as_ng)),
        ("ണ", "ട"): (("nd", 1 - r.nd_as_nt), ("nt", r.nd_as_nt)),
        ("ണ", "ഡ"): (("nd", 1.0),),
        ("മ", "പ"): (("mp", 1 - r.mp_as_mb), ("mb", r.mp_as_mb)),
        ("ച", "ച"): (("ch", 1 - r.cch_double - r.cch_chch), ("cch", r.cch_double), ("chch", r.cch_chch)),
        ("ച", "ഛ"): (("chh", 1 - r.chh_as_ch), ("ch", r.chh_as_ch)),
        ("ത", "ത"): (("tth", 1 - r.tth_as_th - r.tth_as_tt), ("th", r.tth_as_th), ("tt", r.tth_as_tt)),
        ("ത", "ഥ"): (("tth", 1 - r.tth_as_th - r.tth_as_tt), ("th", r.tth_as_th), ("tt", r.tth_as_tt)),
        ("ദ", "ധ"): (("ddh", 1 - g), ("dh", g)),
        ("ശ", "ശ"): (
            ("ss", 1 - r.shsh_as_sh - r.shsh_as_ssh - r.shsh_as_s),
            ("sh", r.shsh_as_sh),
            ("ssh", r.shsh_as_ssh),
            ("s", r.shsh_as_s),
        ),
        ("ജ", "ഞ"): (("jn", 1 - r.jn_as_gn - r.jn_as_nj), ("gn", r.jn_as_gn), ("nj", r.jn_as_nj)),
    }
    return table.get(pair)


def _vowel(key: str, r: Rules, *, after_sha: bool = False, final_after_consonant: bool = False) -> Options:
    """Spellings of a vowel by its key (see VOWEL_SIGNS); canonical first."""
    match key:
        case "a":
            return (("a", 1 - r.sha_vowel_e), ("e", r.sha_vowel_e)) if after_sha else (("a", 1.0),)
        case "aa":
            return (("aa", 1 - r.long_a_short), ("a", r.long_a_short))
        case "i":
            return (("i", 1 - r.final_i_y), ("y", r.final_i_y)) if final_after_consonant else (("i", 1.0),)
        case "ee":
            return (("ee", 1 - r.long_i_short), ("i", r.long_i_short))
        case "oo":
            return (("oo", 1 - r.long_u_short), ("u", r.long_u_short))
        case "ru":
            return (("ru", 1 - r.ru_as_ri - r.ru_as_r), ("ri", r.ru_as_ri), ("r", r.ru_as_r))
        case "EE":
            return (("ee", 1 - r.long_e_short - r.e_as_ae), ("e", r.long_e_short), ("ae", r.e_as_ae))
        case "OO":
            return (("oo", 1 - r.long_o_short), ("o", r.long_o_short))
        case "ai":
            return (("ai", 1 - r.ai_as_ei), ("ei", r.ai_as_ei))
        case "au":
            return (("au", 1 - r.au_as_ou), ("ou", r.au_as_ou))
        case _:
            return ((key, 1.0),)


def _pieces(cluster: list[str], r: Rules) -> list[tuple[Options, str]]:
    """Splits a consonant cluster into spelled pieces, each with the letters it spells."""
    out: list[tuple[Options, str]] = []
    i = 0
    while i < len(cluster):
        pair = tuple(cluster[i : i + 2])
        special = _special(pair, r) if len(pair) == 2 else None  # type: ignore[arg-type]
        if special:
            out.append((special, "".join(pair)))
            i += 2
        elif len(pair) == 2 and pair[0] == pair[1]:
            out.append((_geminate(pair[0], r), "".join(pair)))
            i += 2
        else:
            out.append((_consonant(cluster[i], r), cluster[i]))
            i += 1
    return out


@lru_cache(maxsize=400_000)
def _slots(word: str, r: Rules) -> tuple[Slot, ...]:
    """The slots of `word`. Characters outside Malayalam letters and signs are skipped."""
    chars = list(word)
    n = len(chars)
    slots: list[Slot] = []
    i = 0
    while i < n:
        c = chars[i]
        if c in CONSONANTS:
            cluster = [c]
            j = i + 1
            while j + 1 < n and chars[j] in VIRAMAS and chars[j + 1] in CONSONANTS:
                cluster.append(chars[j + 1])
                j += 2
            pieces = _pieces(cluster, r)
            lone = len(cluster) == 1
            for options, letters in pieces:
                if lone and c == "ട":
                    after_vowel = bool(slots) and slots[-1].kind == "V"
                    options = _consonant("ട", r, r.tta_voiced if after_vowel else r.tta_voiced / 4)
                slots.append(_slot("C", options, lone=lone, src=letters))
            after_sha = cluster[-1] in "ശഷ" and (len(cluster) == 1 or cluster[-2] != cluster[-1])
            # The vowel this cluster carries.
            sign = chars[j] if j < n else ""
            if sign in VOWEL_SIGNS:
                if j + 1 < n and (sign, chars[j + 1]) in SIGN_PAIRS:
                    sign = SIGN_PAIRS[(sign, chars[j + 1])]
                    j += 1
                j += 1
                key = VOWEL_SIGNS[sign]
                final = j >= n and cluster[-1] != "യ"
                slots.append(_slot("V", _vowel(key, r, final_after_consonant=final), src=sign))
            elif sign in VIRAMAS:
                j += 1
                if j < n and chars[j] == ZWJ:  # old-style chillu: no vowel at all
                    j += 1
                else:
                    if lone and c in SIBILANTS:
                        drop = r.final_u_drop_sibilant
                    elif lone:
                        drop = r.final_u_drop_single
                    else:
                        drop = r.final_u_drop_cluster
                    slots.append(_slot("U", (("u", 1 - drop), ("", drop)), src=c if lone else "".join(cluster)))
            else:
                slots.append(_slot("V", _vowel("a", r, after_sha=after_sha), src="a"))
            # A lone യ between o / oo / aa and i / ee may be dropped (poyi→poi).
            if (
                lone
                and c == "യ"
                and len(slots) >= 3
                and slots[-3].spellings[0] in ("o", "oo", "aa")
                and slots[-1].kind == "V"
                and slots[-1].spellings[0] in ("i", "ee")
            ):
                slots[-2] = slots[-2]._replace(tag="y-drop")
            i = j
        elif c in INDEPENDENT_VOWELS:
            key = INDEPENDENT_VOWELS[c]
            # എന്റെ → nte, എന്താ → ntha.
            starts_nt = "".join(chars[i : i + 4]) in ("എന്റ", "എന്ത")
            tag = "e-drop" if not slots and starts_nt else ""
            slots.append(_slot("V", _vowel(key, r), tag, src=c))
            i += 1
        elif c in VOWEL_SIGNS:  # a stray sign: spell it as its vowel
            slots.append(_slot("V", _vowel(VOWEL_SIGNS[c], r), src=c))
            i += 1
        elif c in FINALS:
            latin = FINALS[c]
            if c == "ഃ":
                slots.append(_slot("F", (("h", 1 - r.visarga_drop), ("", r.visarga_drop)), src=c))
            elif c == "ം" and i + 1 < n and chars[i + 1] in CONSONANTS and chars[i + 1] not in LABIALS:
                slots.append(_slot("F", (("m", 1 - r.anusvara_as_n), ("n", r.anusvara_as_n)), src=c))
            elif c == "ൖ":
                slots.append(_slot("F", _consonant("ഴ", r), src=c))
            else:
                slots.append(_slot("F", ((latin, 1.0),), src=c))
            i += 1
        elif "a" <= c.lower() <= "z" and c.isascii():
            slots.append(_slot("F", ((c.lower(), 1.0),)))
            i += 1
        else:  # stray virama, joiner, digit, fraction, avagraha, unassigned
            i += 1
    return tuple(slots)


def canonical(word: str) -> str:
    """The full romanisation of `word`: എന്റെ → ente, മലയാളം → malayaalam, വീട് → veetu."""
    return "".join(s.spellings[0] for s in _slots(word, RULES))


def _pick(slot: Slot, rng: random.Random) -> str:
    if len(slot.spellings) == 1:
        return slot.spellings[0]
    x = rng.random()
    for spelling, edge in zip(slot.spellings, slot.cumulative):
        if x < edge:
            return spelling
    return slot.spellings[-1]


def _sample(slots: tuple[Slot, ...], rng: random.Random, r: Rules) -> str:
    """One random spelling of a word's slots."""
    picked = [_pick(s, rng) for s in slots]
    drops = {"e-drop": r.initial_e_drop, "y-drop": r.y_drop_between_vowels}
    for k, s in enumerate(slots):
        if s.tag and rng.random() < drops[s.tag]:
            picked[k] = ""
    if len(slots) <= MEDIAL_DROP_MAX_SLOTS and rng.random() < r.medial_vowel_drop:
        last = len(slots) - 1
        medial = [
            k
            for k in range(1, last)
            if slots[k].kind == "V"
            and slots[k - 1].lone
            and (
                (slots[k + 1].lone and picked[k] in ("i", "e"))
                or (k + 1 == last and slots[k + 1].kind == "F" and picked[k] in ("a", "i", "u", "e"))
            )
        ]
        if medial:
            picked[rng.choice(medial)] = ""
    if slots and slots[-1].kind == "V" and picked[-1] == "e" and rng.random() < r.final_e_ey:
        picked[-1] = "ey"
    return "".join(picked)


def variants(word: str, rng: random.Random, rules: Rules = RULES) -> list[str]:
    """1–4 distinct Latin spellings of `word`, drawn with `rng`, weighted by how people type.

    Deterministic given the state of `rng`. Every spelling is lowercase a–z; the canonical
    spelling is always possible. A word with nothing to romanise (only digits, say) gives [].
    """
    slots = _slots(word, rules)
    canon = "".join(s.spellings[0] for s in slots)
    if not canon:
        return []
    want = rng.choices((1, 2, 3, 4), weights=rules.variant_counts)[0]
    out = [canon] if rng.random() < rules.canonical_share else []
    for _ in range(3 * want):
        if len(out) >= want:
            break
        v = _sample(slots, rng, rules)
        if v and v not in out:
            out.append(v)
    return out or [canon]
