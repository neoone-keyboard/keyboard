from mlkb import langs
from mlkb.data import human
from mlkb.translit.vocab import MALAYALAM, decode_ml, encode_ml, fits, native


def test_every_language_config_loads():
    assert {"ml", "hi"} <= set(langs.ids())
    for lang_id in langs.ids():
        lang = langs.load(lang_id)
        assert lang.block[1] - lang.block[0] == 0x7F
        assert lang.reference


def test_malayalam_vocabulary_is_unchanged():
    assert native("ml") is MALAYALAM
    assert len(MALAYALAM) == 133


def test_devanagari_vocabulary_round_trips():
    deva = native("hi")
    assert len(deva) == len(MALAYALAM), "same model shape"
    assert deva.char(3) == "ऀ"
    for word in ["क्या", "ज़िंदगी", "नमस्ते"]:
        assert decode_ml(encode_ml(word, deva), deva) == word
    assert fits("kya", "क्या", vocab=deva)
    assert not fits("kya", "क्या")  # not Malayalam


def test_hindi_pairs_are_cleaned_as_devanagari():
    assert human.clean("Kya", "क्या", "hi") == ("kya", "क्या")
    assert human.clean("kya", "क्या।", "hi") is None, "a danda is not a letter"
    assert human.clean("kya", "എന്റെ", "hi") is None
    # NFC: a precomposed nukta letter becomes consonant + nukta.
    assert human.clean("zindagi", "ज़िंदगी", "hi") == ("zindagi", "ज़िंदगी")
