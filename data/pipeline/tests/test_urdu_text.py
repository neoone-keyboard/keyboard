import json
from collections import Counter
from pathlib import Path

from urdu_text import Urdu, count, cut, fold_presentation_forms, main, rewrite

LANG = Path(__file__).resolve().parents[2] / "langs/ur/lang.json"


def urdu() -> Urdu:
    return Urdu(json.loads(LANG.read_text(encoding="utf-8")))


def test_presentation_forms_fold_to_letters_and_nothing_else_changes():
    # ﻛﺘﺎﺏ: kaf, teh, alef, beh in their shaped forms.
    assert fold_presentation_forms("ﻛﺘﺎﺏ") == "كتاب"
    # Outside the ranges NFKC would change these; they stay.
    assert fold_presentation_forms("x² ﬁ ٢") == "x² ﬁ ٢"


def test_cleaning_applies_the_lang_files_replacements():
    u = urdu()
    # Arabic kaf, yeh and heh become Urdu's; the zabar and the tatweel go.
    assert u.clean("كيا هے") == "کیا ہے"
    assert u.clean("کَـتاب") == "کتاب"
    assert u.clean("ﻛﺘﺎﺏ") == "کتاب"


def test_tokens_are_runs_of_letters():
    u = urdu()
    line = "لیکن، وہ ۲۰۲۰ میں آیا۔"
    assert [line[a:b] for a, b in u.tokens(line)] == ["لیکن", "وہ", "میں", "آیا"]


def test_run_together_words_are_cut_after_a_non_joining_letter():
    counts = Counter({"کے": 5000, "ساتھ": 2000, "کےساتھ": 30, "دو": 3000, "ران": 200, "دوران": 900})
    assert cut("کےساتھ", counts, 10, 50) == ["کے", "ساتھ"]
    # A real word much more frequent than a tenth of a half stays whole.
    assert cut("دوران", counts, 10, 50) == ["دوران"]
    # Never after a letter that joins the next: ک + تاب is not a place to cut.
    assert cut("کتاب", Counter({"ک": 10**6, "تاب": 10**6, "کتاب": 1}), 10, 50) == ["کتاب"]
    # Halves rarer than min-count are not words to cut into.
    assert cut("کےبل", Counter({"کے": 5000, "بل": 20}), 10, 50) == ["کےبل"]
    # Three words run together; ساتھ + ہے is not a cut, as ھ joins the ہ after it.
    counts.update({"لیے": 3000, "اور": 6000, "ہے": 4000})
    assert cut("کےلیےاور", counts, 10, 50) == ["کے", "لیے", "اور"]
    assert cut("ساتھہے", counts, 10, 50) == ["ساتھہے"]


def test_main_counts_then_rewrites(tmp_path):
    lines = ["وہ کے ساتھ گیا اور کے ساتھ آیا\n"] * 60 + ["وہ کےساتھ گیا\n"]
    text = tmp_path / "in.txt"
    text.write_text("".join(lines), encoding="utf-8")
    out = tmp_path / "out.txt"
    assert main([str(text), "--lang", str(LANG), "--out", str(out)]) == 0
    got = out.read_text(encoding="utf-8").splitlines()
    assert got[-1] == "وہ کے ساتھ گیا"
    assert got[0] == lines[0].strip()
    assert count(["کےساتھ"], urdu()) == Counter({"کےساتھ": 1})
    assert list(rewrite(["a ك\n"], urdu(), Counter(), 10, 50)) == ["a ک\n"]
