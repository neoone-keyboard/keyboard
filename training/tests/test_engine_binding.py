from pathlib import Path

import mlengine

LEX = Path(__file__).resolve().parents[2] / "apps/ios/KeyboardExtension/Lexicon"
PACK = LEX.parents[3] / "packs/ml.neopack"


def engine():
    return mlengine.Engine(
        english=str(LEX / "en.fst"),
        malayalam=str(PACK / "ml.fst"),
        manglish=str(PACK / "manglish.fst"),
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
