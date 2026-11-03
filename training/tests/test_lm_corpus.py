import random
from pathlib import Path

import pytest

from mlkb.lm.corpus import (dedupe_key, is_leak, loanwords_from, mix_sentence,
                            romanise_sentence, sentences)

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
