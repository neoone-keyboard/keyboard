"""Pack-language romanisation tables (data/langs/as), the Assamese normal form, and the vocab."""

import random

import pytest

from mlkb.langs import human
from mlkb.langs.calibrate import align
from mlkb.langs.format1 import Format1, export
from mlkb.langs.romanisation import LANGS, Tables
from mlkb.translit.vocab import BENGALI_ASSAMESE, MALAYALAM, decode_ml, encode_ml, fits, native


@pytest.fixture(scope="module", params=["romanisation-weights.json", "romanisation.hand.json"])
def tables(request) -> Tables:
    return Tables.load(LANGS / "as" / request.param)


def test_canonical_romanisations(tables):
    # The fitted and the hand-set tables agree on these.
    assert tables.canonical("মই") == "moi"
    assert tables.canonical("তুমি") == "tumi"
    assert tables.canonical("ভাল") == "bhal"
    assert tables.canonical("ক'ত") == "kot"
    assert tables.canonical("মানুহ") == "manuh"
    assert tables.canonical("অসম").endswith("xom")


def test_assamese_letters_have_their_own_spellings(tables):
    # ৰ is r and ৱ is w (or b), not the Bengali র / ব they look like.
    assert tables.canonical("ৰজা") == "roja"
    assert tables.slots("ৱ")[0].table == "consonants"
    assert tables.slots("ৱ")[0].key == "ৱ"
    assert {s for s, _ in tables.slots("ৱা")[0].options} >= {"w", "b"}


def test_conjuncts_khanda_ta_and_signs(tables):
    assert tables.canonical("ক্ষমা").startswith("kh")
    assert tables.canonical("উৎসৱ").startswith("ut")  # khanda ta
    assert tables.canonical("সংগঠন").startswith("xong")  # anusvara
    assert "y" in tables.canonical("ধন্যবাদ")  # -ya sign
    # A doubled letter is spelled doubled or single.
    spellings = {s for s, _ in tables.slots("আল্লা")[1].options}
    assert spellings == {"ll", "l"}
    # য় (written য + nukta after NFC) is one letter.
    assert tables.canonical(human.load("as").normalize("বিদ্যালয়")).endswith("loy")


def test_schwa_deletion_marks_the_unsaid_vowel():
    t = Tables.load("as")
    tables = [s.table for s in t.slots("ৰাজনীতি")]
    assert "inherent.schwaDeleted" in tables
    # উন্নয়ন: right to left, ন's vowel goes, so য়'s stays.
    kinds = [s.table for s in t.slots("উন্নয়নমূলক")]
    assert kinds.count("inherent.schwaDeleted") == 1


def test_typed_spellings_share_a_key():
    f = Format1.load("as")
    for word, typed in [
        ("অসম", ["oxom", "asom", "axom", "oshom"]),
        ("কেনে", ["kene"]),
        ("আছা", ["asa", "acha", "aasa"]),
        ("চাহ", ["sah", "chah", "cha"]),
        ("ভাল", ["bhal", "bal", "bhaal", "bhalo"]),
        ("মানুহ", ["manuh", "manuh"]),
    ]:
        for latin in typed:
            # An exact key, or a prefix of it (the lookup's completions): চাহ "cha".
            assert f.key(word).startswith(f.skeleton(latin)), (word, latin)


def test_format1_file_follows_the_spec():
    f = Format1.load("as")
    d = f.data
    assert d["format"] == 1 and d["script"] == "beng" and d["virama"] == "\u09cd"
    assert d["atWordEnd"]["inherent"] == ""  # মানুহ manuh
    assert {"ৎ", "ং", "ঁ"} <= set(d["finals"])
    assert d["consonants"]["ৰ"] == "r" and d["consonants"]["ৱ"] in ("w", "b")
    assert isinstance(d["notes"], str)
    assert f.romanize("মানুহ") == "manuh"
    assert f.romanize("কৰিব") == "korib"
    assert f.romanize("হ'ল") == "hol"  # the apostrophe is skipped; the o is the inherent vowel
    assert f.romanize("স্কুল") == "skul"  # a sibilant in a cluster is s (a listed cluster)
    assert f.romanize("ধন্যবাদ") == "dhonyobad"  # the -ya sign is y
    assert f.romanize("উৎসৱ").startswith("ut")


def test_format1_export_matches_the_weighted_canonical_spellings():
    # Without the contexts the format can't hold, the two walks agree on plain words.
    f, t = Format1(export(Tables.load("as"))), Tables.load("as")
    for word in ["মই", "তুমি", "কেনে", "ভাল", "মানুহ", "ক'ত", "ৰজা", "ক্ষমা"]:
        assert f.romanize(word) == t.canonical(word), word


def test_skeleton_is_lowercase_letters():
    t = Tables.load("as")
    assert t.skeleton("Oxom!") == t.skeleton("oxom")
    assert t.skeleton("123") == ""
    assert all("a" <= c <= "z" for c in t.skeleton("khyomota"))


def test_variants_are_latin_and_include_canonical_sometimes(tables):
    rng = random.Random(0)
    seen = set()
    for _ in range(200):
        for v in tables.variants("অসমীয়া", rng):
            assert v.isascii() and v.isalpha()
            seen.add(v)
    assert tables.canonical("অসমীয়া") in seen
    assert len(seen) > 3


def test_align_finds_the_typed_spelling(tables):
    slots = tables.slots("অসম")
    assert align(slots, "axom") is not None
    assert align(slots, "qqq") is None


def test_assamese_normal_form():
    lang = human.load("as")
    assert lang.normalize("তোমার") == "তোমাৰ"  # Bengali ra folds to ৰ
    assert lang.normalize("ৰৱ") == "ৰৱ"
    assert lang.normalize("উত্‍সৱ") == "উৎসৱ"
    assert lang.normalize("হ’ল") == "হ'ল"
    assert lang.normalize("য়") == "য়"
    assert lang.is_word("হ'ল") and not lang.is_word("২০") and not lang.is_word("ন্")
    assert lang.normalize("মােৰ") == "মোৰ" and not lang.is_word("আু")


def test_native_vocabularies():
    assert native("ml") is MALAYALAM and len(MALAYALAM) == 133
    assert len(BENGALI_ASSAMESE) == 134  # the block, ZWJ, ZWNJ and the apostrophe
    for word in ["অসমীয়া", "হ'ল", "ৰৱ"]:
        assert decode_ml(encode_ml(word, BENGALI_ASSAMESE), BENGALI_ASSAMESE) == word
    assert fits("hol", "হ'ল", vocab=BENGALI_ASSAMESE)
    assert not fits("hol", "হ'ল")  # not Malayalam
    # The Malayalam ids are unchanged.
    assert encode_ml("എ") == [1, 3 + 0x0D0E - 0x0D00, 2]
