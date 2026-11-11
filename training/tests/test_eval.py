from mlkb.eval.__main__ import regressions
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


def test_next_word_metrics_are_gated():
    base = {"english_wrong_rate": 0.001, "next_en": {"top1": 0.2, "top3": 0.4, "ks": 0.1, "n": 9}}
    now = {"english_wrong_rate": 0.001, "next_en": {"top1": 0.2, "top3": 0.39, "ks": 0.1, "n": 9}}
    assert regressions(now, base) == ["next_en top3: 0.3900 < baseline 0.4000"]


def test_ship_gate_needs_gains_and_no_new_wrong_replacements():
    from mlkb.eval.__main__ import ship_gate

    m3a = {"english_wrong": 3, "next_en": {"top3": 0.30, "ks": 0.10},
           "english_typos_context": {"top1": 0.60}, "language_choice": {"top1": 0.70}}
    good = {"english_wrong": 3, "next_en": {"top3": 0.40, "ks": 0.15},
            "english_typos_context": {"top1": 0.75}, "language_choice": {"top1": 0.80}}
    assert ship_gate(good, m3a) == []
    bad = {**good, "english_wrong": 4, "next_en": {"top3": 0.30, "ks": 0.15}}
    assert ship_gate(bad, m3a) == ["english_wrong: 4 > M3a 3", "next_en top3: 0.3000 not above M3a 0.3000"]


def test_hindi_runs_its_own_sets_on_its_own_pack():
    from mlkb.eval.__main__ import LANGS, load_engine

    hi = LANGS["hi"]
    assert set(hi.next_sets) == {"next_en", "next_hi", "next_hinglish"}
    assert hi.next_sets["next_hi"][2] == "native"
    assert set(hi.context_sets) == {"english_typos_context"}
    engine = load_engine(None, None, "hi")
    assert "hi:lexicon" in engine.loaded_sources() and "hi:romanised" in engine.loaded_sources()


def test_assamese_and_tamil_run_their_own_sets_on_their_own_packs():
    from mlkb.eval.__main__ import LANGS, load_engine

    for lang, romanised in (("as", "next_romanised"), ("ta", "next_tanglish")):
        config = LANGS[lang]
        assert set(config.next_sets) == {"next_en", f"next_{lang}", romanised}
        assert config.next_sets[f"next_{lang}"][2] == "native"
        assert config.gate_baselines.name == "baselines-nolm.json" and config.gate_baselines.parent.name == lang
        sources = load_engine(None, None, lang).loaded_sources()
        assert f"{lang}:lexicon" in sources and f"{lang}:romanised" in sources


def test_the_lm_gate_wants_every_next_set_and_context_set_above_the_baseline():
    from mlkb.eval.__main__ import LANGS, ship_gate

    hi = LANGS["hi"]
    row = {"top1": 0.5, "top3": 0.5, "ks": 0.5}
    base = {"english_wrong": 3, **{n: row for n in hi.next_sets}, **{n: row for n in hi.context_sets}}
    better = {"english_wrong": 3, **{n: {k: 0.6 for k in row} for n in [*hi.next_sets, *hi.context_sets]}}
    assert ship_gate(better, base, hi) == []
    worse = {**better, "next_hinglish": {"top1": 0.6, "top3": 0.5, "ks": 0.6}, "english_wrong": 4}
    assert ship_gate(worse, base, hi) == ["english_wrong: 4 > baseline 3",
                                          "next_hinglish top3: 0.5000 not above baseline 0.5000"]


def test_sections_come_from_the_files_section_lines(tmp_path):
    from mlkb.eval.translit import load, load_sections

    path = tmp_path / "set.tsv"
    path.write_text("# header\n\tmoi\tমই\n# section: AK-Freq\n\ttumi\tতুমি\n# section: AK-NEI\nx\tjon\tজন\n", encoding="utf-8")
    assert [(s, r.latin) for s, r in load_sections(path)] == [("", "moi"), ("AK-Freq", "tumi"), ("AK-NEI", "jon")]
    assert [r for _, r in load_sections(path)] == load(path)


def test_pack_eval_scores_the_first_native_suggestion_by_section():
    from mlkb.eval import pack

    rows = [("AK-Freq", Row("", "tumi", "তুমি")), ("AK-Freq", Row("", "naan", "নান")),
            ("AK-NEI", Row("", "jon", "জন")), ("AK-NEI", Row("", "hafiz", "হাফিজ"))]
    eng = FakeEngine({
        "tumi": [("তুমি", "manglish", 0), ("tumi", "verbatim", 0), ("time", "correction", 0)],
        "naan": [("naan", "verbatim", 0), ("নান", "manglish", 0), ("man", "correction", 0)],  # English leads
        "jon": [("জোন", "manglish", 0), ("jon", "verbatim", 0), ("জন", "manglish", 0)],  # in the bar, not first
        "hafiz": [("hafiz", "verbatim", 0), ("Haifa", "correction", 0)],  # nothing native
    })
    got = {k: t.rates() for k, t in pack.score(eng, rows).items()}
    assert list(got) == ["all", "AK-Freq", "AK-NEI"]
    assert got["all"] == {"n": 4, "top1": 0.5, "in_bar": 0.75, "first": 0.25, "none": 0.25}
    assert got["AK-Freq"] == {"n": 2, "top1": 1.0, "in_bar": 1.0, "first": 0.5, "none": 0.0}
    assert got["AK-NEI"] == {"n": 2, "top1": 0.0, "in_bar": 0.5, "first": 0.0, "none": 0.5}


def test_pack_eval_opens_any_pack_from_its_manifest(tmp_path):
    from mlkb.eval import pack

    for lang in ("ml", "hi", "as"):
        sources = pack.open_pack(pack.ROOT / f"packs/{lang}.neopack", without=("translit", "lm")).loaded_sources()
        assert f"{lang}:lexicon" in sources and f"{lang}:translit" not in sources, sources
    assert "as:translit" in pack.open_pack(pack.ROOT / "packs/as.neopack").loaded_sources()
    bar = pack.open_pack(pack.ROOT / "packs/as.neopack").suggest("tumi", "latin", True)
    assert bar[0][0] == "তুমি"
    # A manifest that lists a file the folder lacks stops the run: the engine would say nothing.
    broken = tmp_path / "xx.neopack"
    broken.mkdir()
    (broken / "manifest.json").write_text('{"id": "xx", "script": "beng", "files": {"lexicon": "xx.fst"}}')
    import pytest

    with pytest.raises(SystemExit, match="missing: xx.fst"):
        pack.open_pack(broken)
