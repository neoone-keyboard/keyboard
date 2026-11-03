import random
from pathlib import Path

import pytest

from mlkb.lm import corpus
from mlkb.lm.corpus import (TEMPLATE_CAP, cut_runs, dedupe_key, is_leak, keep_template, loanwords_from,
                            mix_sentence, romanise_sentence, sentences)

LOAN = {"കമ്പ്യൂട്ടർ": "computer"}


def test_romanised_sentences_are_latin_and_use_english_loanwords():
    rng = random.Random(0)
    out = romanise_sentence("ഞാൻ കമ്പ്യൂട്ടർ നോക്കി.", rng, LOAN, p_loan=1.0)
    words = out.split()
    assert words[1] == "computer"
    assert all(c.isascii() for c in out)
    assert out.endswith(".")


def test_mixed_sentences_switch_some_words_only():
    rng = random.Random(1)
    out = mix_sentence("ഞാൻ ഇന്ന് വീട്ടിൽ പോകും", rng, LOAN, frac=0.5, p_loan=0.0).split()
    latin = sum(w.isascii() for w in out)
    assert 0 < latin < 4 and len(out) == 4


def test_dedupe_masks_digits():
    assert dedupe_key("X is a village with 1,203 people.") == dedupe_key("X is a village with 87 people.")


def test_sentences_split_on_end_marks():
    assert sentences("One. Two? Three!") == ["One.", "Two?", "Three!"]


def test_eval_sentences_are_leaks_in_either_form():
    leaks = {dedupe_key("എന്റെ വീട്.")}
    assert is_leak("എന്റെ  വീട്.", leaks)
    assert not is_leak("വേറെ വീട്.", leaks)


def test_long_runs_of_one_unit_are_cut():
    assert cut_runs("between 1" + "0" * 30 + " and 2") == "between 1" + "0" * 12 + " and 2"
    assert cut_runs("൦" * 20) == "൦" * 12
    assert cut_runs("end " + ". " * 20 + "next") == "end " + ". " * 6 + "next"
    assert cut_runs("7" + ",000" * 6 + " stars") == "7,000,000,000 stars"
    assert cut_runs("ഹരി " * 6 + "എന്ന്") == "ഹരി " * 3 + "എന്ന്"


def test_short_runs_are_kept():
    for text in ("1000000 people", "1,000,000,000 bytes", "hahahaha", "ജയ ജയ ജയ ജയഹേ", "........"):
        assert cut_runs(text) == text


CENSUS = "for every 100 females, there were {} males, and for every 100 females age 18 and over there were {} males."


def test_templated_sentences_are_capped_with_digits_masked():
    counts: dict[int, int] = {}
    kept = [keep_template(CENSUS.format(90 + i, 80 + i), counts) for i in range(TEMPLATE_CAP + 2)]
    assert kept == [True] * TEMPLATE_CAP + [False, False]
    assert keep_template("for every 100 males there were 90 females in the whole of the county.", counts)
    assert all(keep_template("he retired in 2012.", counts) for _ in range(TEMPLATE_CAP + 2))


def test_clean_source_caps_templates_and_cuts_runs_in_training_text_only(tmp_path, monkeypatch):
    monkeypatch.setattr(corpus, "_is_valid", lambda norm: norm.startswith("valid"))
    names = ["alpha", "beta", "gamma", "delta", "epsilon"]
    raw = [f"{n} is a town. " + CENSUS.format(90 + i, 80 + i) for i, n in enumerate(names)]
    raw += ["valid zeta is a town. " + CENSUS.format(70, 71), CENSUS.format(60, 61), "a big number 1" + "0" * 40 + "."]
    src = tmp_path / "src.txt"
    src.write_text("\n".join(raw) + "\n", encoding="utf-8")
    train, valid = [], []
    stats = corpus.clean_source("en-wiki", src, None, set(), set(), random.Random(0),
                                lambda c, t: train.append(c), lambda c, t: valid.append(c))
    assert train[:3] == raw[:3]
    assert train[3:5] == ["delta is a town.", "epsilon is a town."]
    assert valid == [raw[5]]
    assert train[5] == "a big number 1" + "0" * 12 + "."
    assert len(train) == 6  # the bare census sentence went entirely
    assert stats["template_sentences"] == 3 and stats["template_paragraphs"] == 1
    assert stats["runs_cut"] == 1 and stats["run_chars"] == 28
    assert stats["paragraphs"] == 6 and stats["valid_paragraphs"] == 1


def test_dropping_a_paragraph_keeps_the_random_stream(tmp_path, monkeypatch):
    """The chat transform draws once per training paragraph, kept or not, so the held-out
    samples drawn from the same generator don't move when the filters change."""
    monkeypatch.setattr(corpus, "_is_valid", lambda norm: False)
    kept = [f"town {'x' * (i + 1)}. " + CENSUS.format(i, i) for i in range(TEMPLATE_CAP)]
    dropped = [" ".join([CENSUS.format(k, k)] * k) for k in (1, 2, 3)]
    src = tmp_path / "src.txt"
    src.write_text("\n".join(kept + dropped) + "\n", encoding="utf-8")
    rng = random.Random(5)
    stats = corpus.clean_source("en-wiki", src, None, set(), set(), rng, lambda c, t: None, lambda c, t: None)
    assert stats["template_paragraphs"] == 3
    expected = random.Random(5)
    for _ in kept + dropped:
        expected.random()
    assert rng.random() == expected.random()


def test_loanwords_need_an_english_word_and_a_malayalam_word():
    rows = [{"source": "IndicCorp", "english word": "computer", "native word": "കമ്പ്യൂട്ടർ", "score": 0.0},
            {"source": "IndicCorp", "english word": "veedu", "native word": "വീട്", "score": 0.0},
            {"source": "Dakshina Dataset", "english word": "bus", "native word": "ബസ്", "score": 0.0}]
    english = {"computer", "bus"}
    malayalam = {"കമ്പ്യൂട്ടർ", "വീട്", "ബസ്"}
    assert loanwords_from(rows, english, malayalam) == {"കമ്പ്യൂട്ടർ": "computer"}


VOCAB = Path(__file__).resolve().parents[1] / "build/lm/pieces.tsv"


@pytest.mark.skipif(not VOCAB.exists(), reason="vocabulary not built")
def test_the_vocabulary_covers_both_scripts():
    pieces = [l.split("\t")[0] for l in VOCAB.read_text(encoding="utf-8").splitlines()]
    assert len(pieces) == 16384
    ml = sum(any("ഀ" <= c <= "ൿ" for c in p) for p in pieces)
    latin = sum(any(c.isascii() and c.isalpha() for c in p) for p in pieces)
    assert ml > 4000 and latin > 6000
