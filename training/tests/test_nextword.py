from mlkb.eval.nextword import Sentence, keystroke_savings, load_aligned, next_word, plain


class FakeEngine:
    def __init__(self, bars): self.bars = bars
    def suggest(self, context, layout, autocorrect):
        return self.bars.get(context, [])


def test_plain_strips_punctuation():
    words = ("Hi", "are", "you", "free")
    assert plain("Hi, are you free?") == Sentence(words, words)


def test_next_word_counts_from_the_second_word():
    eng = FakeEngine({
        "I ": [("am", "next_word", 0), ("was", "next_word", 0)],
        "I am ": [("going", "next_word", 0), ("here", "next_word", 0), ("not", "next_word", 0)],
    })
    s = next_word(eng, [plain("I am here")], "latin")
    assert (s.top1, s.top3, s.n) == (0.5, 1.0, 2)


def test_keystroke_savings_take_the_first_prefix_whose_bar_holds_the_word():
    # "I" is typed (letter + space = 2); "am" is offered right after the space (1 tap).
    eng = FakeEngine({"I ": [("am", "next_word", 0)]})
    assert keystroke_savings(eng, [plain("I am")], "latin") == 1 - 3 / 5


def test_manglish_rows_type_latin_and_want_malayalam():
    eng = FakeEngine({"ente": [("എന്റെ", "manglish", 0)]})
    # Found only once fully typed: the space commits it, so nothing is saved.
    assert keystroke_savings(eng, [Sentence(("ente",), ("എന്റെ",))], "latin") == 0.0


def test_aligned_rows_need_the_same_word_count(tmp_path):
    p = tmp_path / "s.tsv"
    p.write_text("# header\nente veedu\tഎന്റെ വീട്\nente\tഎന്റെ വീട്\n", encoding="utf-8")
    assert load_aligned(p) == [Sentence(("ente", "veedu"), ("എന്റെ", "വീട്"))]


def test_next_sets_align_and_split_disjointly():
    from mlkb.eval import next_sets

    assert next_sets.aligned_pair("ente veedu.", "എന്റെ വീട്.") == ("ente veedu", "എന്റെ വീട്")
    assert next_sets.aligned_pair("ente", "എന്റെ വീട്") is None
    assert next_sets.aligned_pair("2,439 ki", "2,439 കി") is None
    roman = [f"w{'a' * i} x" for i in range(40)]
    native = [f"ന{'ക' * i} ക" for i in range(40)]
    gate, tune = next_sets.manglish_rows(roman, native)
    assert len(gate) == len(tune) == 20 and not set(gate) & set(tune)


def test_the_danda_is_punctuation():
    from mlkb.eval.nextword import plain

    assert plain("मैं घर गया।").words == ("मैं", "घर", "गया")


def test_hinglish_rows_align_with_the_engines_normal_form():
    import mlengine

    from mlkb.eval import next_sets

    got = next_sets.aligned_pair("ghar gaye.", "घर गए।", mlengine.lm_normalize)
    assert got == ("ghar gaye", "घर गए")
    assert next_sets.aligned_pair("michael kamen ne", "माइकल कामेन", mlengine.lm_normalize) is None
