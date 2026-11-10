from pathlib import Path

import mlengine

LEX = Path(__file__).resolve().parents[2] / "apps/ios/KeyboardExtension/Lexicon"
PACK = LEX.parents[3] / "packs/ml.neopack"


def engine():
    return mlengine.Engine(
        english=str(LEX / "en.fst"),
        malayalam=str(PACK / "ml.fst"),
        manglish=str(PACK / "manglish.fst"),
        romanisation=str(PACK / "romanisation.json"),
        suffixes=str(PACK / "suffixes.tsv"),
        chat_forms=str(PACK / "chat-forms.tsv"),
        layout=str(PACK / "layout.json"),
        next_words=str(PACK / "next.fst"),
    )


def test_suggest_returns_bar_order():
    got = engine().suggest("teh", layout="latin", autocorrect=True)
    assert got[0][:2] == ("the", "correction")


def test_manglish_reaches_python():
    texts = [t for t, kind, _ in engine().suggest("ente", "latin", True)]
    assert "എന്റെ" in texts


def test_malayalam_words_are_frequency_ordered():
    words = mlengine.malayalam_words(str(PACK / "ml.fst"), 1000)
    assert len(words) == 1000
    counts = [c for _, c in words]
    assert counts == sorted(counts, reverse=True)
    assert words[0][0] == "ഒരു"


HI = LEX.parents[3] / "packs/hi.neopack"


def hindi_engine():
    return mlengine.Engine(
        english=str(LEX / "en.fst"),
        malayalam=str(HI / "hi.fst"),
        manglish=str(HI / "hinglish.fst"),
        romanisation=str(HI / "romanisation.json"),
        layout=str(HI / "layout.json"),
        next_words=str(HI / "next.fst"),
        pack="hi",
        script="deva",
    )


def test_another_pack_loads_under_its_own_id():
    engine = hindi_engine()
    assert "hi:lexicon" in engine.loaded_sources()
    texts = [t for t, kind, _ in engine.suggest("kya", "latin", True)]
    assert "क्या" in texts


def test_the_native_layout_is_the_packs():
    bar = hindi_engine().suggest("मैं ", "native", True)
    assert bar and all(kind == "next_word" for _, kind, _ in bar)
    assert any(not t.isascii() for t, _, _ in bar)
