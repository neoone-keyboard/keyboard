import random

from mlkb.data.table_romanise import ROOT, Table, variants

HINDI = ROOT / "packs/hi.neopack/romanisation.json"
FIXTURE = ROOT / "engine/lexbuild/tests/fixtures/hi-romanisation.tsv"


def test_fixture_rows_match_lexbuild():
    """The same rows `engine/lexbuild/tests/romanisation.rs` checks against the Rust romaniser."""
    t = Table.load(HINDI)
    rows = FIXTURE.read_text(encoding="utf-8").splitlines()
    assert len(rows) > 100
    for row in rows:
        native, canonical, skeleton, typed, typed_skeleton = row.split("\t")
        assert t.canonical(native) == canonical, native
        assert t.key(native) == skeleton, native
        assert t.skeleton(typed) == typed_skeleton, typed


def test_schwa_deletion_and_conjuncts():
    t = Table.load(HINDI)
    for word, latin in [
        ("भारत", "bharat"), ("कमरा", "kamra"), ("समझना", "samajhna"), ("बचपन", "bachpan"),
        ("मित्र", "mitra"), ("दर्द", "dard"), ("ज्ञान", "gyan"), ("में", "mein"), ("संभव", "sambhav"),
    ]:  # fmt: skip
        assert t.canonical(word) == latin, word


def test_variants_are_latin_and_share_the_skeleton_mostly():
    t = Table.load(HINDI)
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
    t = Table.load(HINDI)
    assert t.is_native("नमस्ते")
    assert not t.is_native("नमस्ते।")
    assert not t.is_native("abc")
