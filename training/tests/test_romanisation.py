import random

from mlkb.data.romanisation import ROOT, Romanisation
from mlkb.data.table_romanise import Table, variants

HINDI = ROOT / "packs/hi.neopack/romanisation.json"
SPELLING = ROOT / "training/mlkb/langs/hi-spelling.json"
FIXTURE = ROOT / "engine/lexbuild/tests/fixtures/hi-romanisation.tsv"


def test_fixture_rows_match_lexbuild():
    """The same rows `engine/lexbuild/tests/romanisation.rs` checks against the Rust walk."""
    r = Romanisation.load(HINDI)
    rows = FIXTURE.read_text(encoding="utf-8").splitlines()
    assert len(rows) > 100
    for row in rows:
        native, canonical, skeleton, typed, typed_skeleton = row.split("\t")
        assert r.canonical(native) == canonical, native
        assert r.key(native) == skeleton, native
        assert r.skeleton(typed) == typed_skeleton, typed


def test_walk_follows_the_format_spec():
    r = Romanisation.load(HINDI)
    for word, latin in [
        ("भारत", "bharat"), ("कमरा", "kamara"), ("समझना", "samajhana"), ("मित्र", "mitr"),
        ("दर्द", "dard"), ("ज्ञान", "gyan"), ("क्षमा", "kshama"), ("में", "men"),
        ("संभव", "sanbhav"), ("फ़िल्म", "film"), ("ज़रा", "zara"), ("न", "n"),
    ]:  # fmt: skip
        assert r.canonical(word) == latin, word
    assert r.skeleton("sambhav") == r.skeleton("sanbhav")
    assert r.skeleton("nahih") == r.skeleton("nahi")
    assert r.skeleton("mein") != r.skeleton("main")


def test_an_optional_final_gives_a_second_key():
    """As `engine/core/src/romanisation.rs` (`word_skeletons`): the pack's `optionalFinals`."""
    r = Romanisation.load(HINDI)
    assert r.optional_finals == ["ं", "ँ"]
    for word, short, keys in [
        ("नहीं", "nahi", ["nhin", "nhi"]),
        ("हूँ", "hu", ["hun", "hu"]),
        ("लोगों", "logo", ["logon", "logo"]),
        ("दिन", None, ["din"]),
        ("हिंदी", None, ["hindi"]),
        ("नही", None, ["nhi"]),
        ("जगह", None, ["jga", "jgh"]),
        ("कह", None, ["ka"]),
        ("तेरह", None, ["tera"]),
    ]:
        assert r.canonical_short(word) == short, word
        assert r.keys(word) == keys, word
        assert r.keys(word)[0] == r.key(word)
    assert r.keys("्") == []
    plain = Romanisation({**r.raw, "optionalFinals": []})
    assert plain.keys("नहीं") == ["nhin"]


def test_spelling_table_keeps_schwa_deletion():
    t = Table.load(SPELLING)
    for word, latin in [("कमरा", "kamra"), ("समझना", "samajhna"), ("मित्र", "mitra"), ("में", "mein")]:
        assert t.canonical(word) == latin, word


def test_variants_are_latin_and_share_the_skeleton_mostly():
    t = Table.load(SPELLING)
    rng = random.Random(0)
    same = total = 0
    for word in ["समझना", "भारत", "ज़िंदगी", "नहीं", "पढ़ाई", "क्षमा", "विश्वास"]:
        for _ in range(50):
            for v in variants(t, word, rng):
                assert v.isascii() and v.isalpha() and v == v.lower(), v
                total += 1
                same += t.skeleton(v) == t.key(word)
    assert same / total > 0.7


def test_script_check():
    t = Table.load(SPELLING)
    assert t.is_native("नमस्ते")
    assert not t.is_native("नमस्ते।")
    assert not t.is_native("abc")
