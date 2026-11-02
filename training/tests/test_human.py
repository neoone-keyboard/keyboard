import json
import random
from collections import Counter
from dataclasses import replace
from pathlib import Path

from mlkb.data import calibrate, human
from mlkb.data.romanise import RULES, _slots, variants
from mlkb.eval.translit import load

REPO = Path(__file__).resolve().parents[2]


def test_normalize_matches_the_engine():
    assert human.normalize("അവന്‍") == "അവൻ"  # old chillu becomes atomic
    assert human.normalize("എന്റെ") == "എന്റെ"
    assert human.normalize("ക‍ട") == "കട"  # a joiner not after a virama is dropped
    assert human.normalize("ക്‌ട") == "ക്‌ട"  # kept after a virama
    assert human.normalize("ൊ") == "ൊ" and human.normalize("ൊ") == "ൊ"  # NFC
    assert human.normalize("\u0d46\u200c\u0d3e") == "\u0d4a"  # halves meet once the ZWNJ goes
    assert human.normalize("\u0d28\u0d4d\u200c\u200d\u0d15") == "\u0d7b\u0d15"  # leaves an old chillu


def test_clean_keeps_only_plain_latin_and_malayalam():
    assert human.clean(" Ente ", "എന്റെ") == ("ente", "എന്റെ")
    assert human.clean("ente1", "എന്റെ") is None
    assert human.clean("ente", "abc") is None
    assert human.clean("a" * 31, "എ") is None


def test_aksharantar_filter_drops_unlicensed_sources(tmp_path):
    rows = [
        {"native word": "കട", "english word": "kada", "source": "IndicCorp", "score": -0.05},
        {"native word": "കടൽ", "english word": "kadal", "source": "IndicCorp", "score": -0.5},
        {"native word": "മല", "english word": "mala", "source": "Dakshina", "score": None},
        {"native word": "വല", "english word": "vala", "source": "Existing", "score": None},
        {"native word": "ലണ്ടൻ", "english word": "london", "source": "Samanantar", "score": None},
        {"native word": "പാരിസ്", "english word": "paris", "source": "Wikidata", "score": None},
    ]
    (tmp_path / "mal_train.json").write_text("".join(json.dumps(r, ensure_ascii=False) + "\n" for r in rows))
    stats = Counter()
    got = human.aksharantar("train", tmp_path, stats)
    assert [(h.latin, h.source) for h in got] == [("kada", "IndicCorp"), ("london", "Samanantar"), ("paris", "Wikidata")]
    assert stats["dropped Dakshina (licence)"] == 1 and stats["dropped Existing (licence)"] == 1
    assert stats["dropped IndicCorp (low mining score)"] == 1


def test_eval_rows_are_seeded_and_unambiguous():
    test = [human.HumanPair(f"w{i}", f"ക{i}", 1, "Dakshina") for i in range(50)]
    test.append(human.HumanPair("w0", "മ", 1, "Dakshina"))  # w0 now maps to two words
    a, b = human.eval_rows(test, n=10), human.eval_rows(test, n=10)
    assert a == b and len(a) == 10
    assert all(r.latin != "w0" for r in a)


def test_align_finds_how_each_slot_was_spelled():
    probe = calibrate._floored(RULES)
    assert calibrate.align(_slots("എന്റെ", probe), "ende", probe) == ["e", "nd", "e"]
    assert calibrate.align(_slots("എന്റെ", probe), "entey", probe) == ["e", "nt", "ey"]
    assert calibrate.align(_slots("എന്റെ", probe), "xyz", probe) is None


def test_fit_recovers_the_probabilities_that_made_the_pairs():
    truth = replace(RULES, long_a_short=0.3, nt_as_nd=0.6, geminate_single=0.2, aspiration_drop=0.1)
    rng = random.Random(0)
    words = ["എന്റെ", "മലയാളം", "കാണാം", "പാടം", "എനിക്ക്", "ഭാരതം", "കല്ല്", "പന്റ്", "ആന", "ധനം"]
    pairs = [(v, w, 1.0) for _ in range(300) for w in words for v in variants(w, rng, truth)]
    fitted, ev, stats = calibrate.fit(pairs)
    assert stats["skipped"] < 0.05 * stats["pairs"]  # medial vowel drops aren't aligned
    for name in ("long_a_short", "nt_as_nd", "geminate_single", "aspiration_drop"):
        assert abs(getattr(fitted, name) - getattr(truth, name)) < 0.1, (name, getattr(fitted, name))


def test_calibrated_rules_round_trip(tmp_path):
    rules = replace(RULES, long_a_short=0.42)
    calibrate.save(rules, {"long_a_short": [42.0, 100.0]}, {"pairs": 1}, tmp_path / "r.json")
    assert calibrate.load(tmp_path / "r.json") == rules


def test_committed_calibration_loads():
    rules = calibrate.load()
    assert rules != RULES
    assert all(0 < getattr(rules, f) < 1 for f in ("long_a_short", "nt_as_nd", "geminate_single"))


def test_human_eval_set_shape():
    path = REPO / "data/eval/translit-human.tsv"
    rows = load(path)
    assert len(rows) == human.HUMAN_EVAL_ROWS
    assert all(r.context == "" and r.latin.isascii() and r.latin.isalpha() for r in rows)
    assert all(human.normalize(r.malayalam) == r.malayalam for r in rows)
