from mlkb.eval.english import wrong_replacements
from mlkb.eval.translit import Row, score


class FakeEngine:
    def __init__(self, bars):
        self.bars = bars

    def suggest(self, context, layout, autocorrect):
        return self.bars[context]


def test_top1_and_top3():
    rows = [Row("", "ente", "എന്റെ"), Row("", "nanni", "നന്ദി")]
    eng = FakeEngine({
        "ente": [("എന്റെ", "manglish", 0.0), ("ente", "verbatim", 0.0)],
        "nanni": [("nanni", "verbatim", 0), ("bank", "correction", 0), ("നന്ദി", "manglish", 0)],
    })
    s = score(eng, rows)
    assert s.top1 == 0.5 and s.top3 == 1.0 and s.n == 2
    assert s.misses == (rows[1],)


def test_context_is_joined_with_a_space():
    rows = [Row("ഞാൻ", "veedu", "വീട്")]
    eng = FakeEngine({"ഞാൻ veedu": [("വീട്", "manglish", 0)]})
    assert score(eng, rows).top1 == 1.0


def test_wrong_replacement_counts_middle_slot_changes():
    eng = FakeEngine({"I said": [("said", "verbatim", 0)], "I said hello": [("hello", "verbatim", 0)],
                      "I": [("I", "verbatim", 0)]})
    assert wrong_replacements(eng, ["I said hello"]) == (0, 3)


def test_a_correction_of_a_correct_word_is_wrong():
    eng = FakeEngine({"I": [("I", "verbatim", 0)], "I sed": [("said", "correction", 0)]})
    assert wrong_replacements(eng, ["I sed"]) == (1, 2)


def test_punctuation_is_stripped_from_the_typed_word():
    # The keyboard sees the word before the punctuation is typed.
    eng = FakeEngine({"Hi": [("Hi", "verbatim", 0)], "Hi there": [("there", "correction", 0)]})
    assert wrong_replacements(eng, ["Hi there!"]) == (0, 2)


def test_a_capitalised_first_word_may_differ_only_in_case():
    eng = FakeEngine({"Ok": [("OK", "correction", 0)], "Ok i": [("I", "correction", 0)]})
    # Sentence start: case-only change is fine. Mid-sentence: i -> I is a change.
    assert wrong_replacements(eng, ["Ok i"]) == (1, 2)


def test_regressions_respect_the_tolerances():
    from mlkb.eval.__main__ import regressions

    base = {"chat_golden": {"top1": 0.5, "top3": 0.8, "n": 10}, "human": None,
            "english_wrong_rate": 0.01, "engine": "m2"}
    same = {**base, "chat_golden": {"top1": 0.496, "top3": 0.8, "n": 10}, "english_wrong_rate": 0.0109}
    assert regressions(same, base) == []
    worse = {**base, "chat_golden": {"top1": 0.49, "top3": 0.8, "n": 10}, "english_wrong_rate": 0.012}
    assert len(regressions(worse, base)) == 2


def test_the_english_typo_set_is_gated_too():
    from mlkb.eval.__main__ import regressions

    base = {"chat_golden": None, "human": None, "english_typos": {"top1": 0.8, "top3": 0.9, "n": 10},
            "english_wrong_rate": 0.01, "engine": "m3a"}
    worse = {**base, "english_typos": {"top1": 0.79, "top3": 0.9, "n": 10}}
    assert regressions(worse, base) == ["english_typos top1: 0.7900 < baseline 0.8000"]
    # A baseline recorded before the set existed gates nothing.
    assert regressions(worse, {**base, "english_typos": None}) == []
