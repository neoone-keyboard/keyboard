import random
from pathlib import Path

import mlengine
import pytest
from mlkb.data.pairs import build, excluded_words
from mlkb.data.romanise import canonical, variants
from mlkb.eval.translit import load

REPO = Path(__file__).resolve().parents[2]
LEXICON = REPO / "packs/ml.neopack/ml.fst"
GOLDEN = REPO / "data/eval/chat-golden.tsv"


def test_canonical_spellings():
    assert canonical("എന്റെ") == "ente"
    # Canonical writes every long vowel doubled, ോ included.
    assert canonical("സുഖമാണോ") == "sukhamaanoo"
    assert canonical("കഴിച്ചോ") == "kazhichoo"
    assert canonical("നന്ദി") == "nandi"
    assert canonical("മലയാളം") == "malayaalam"


def test_canonical_conjuncts_chillus_and_signs():
    assert canonical("എന്ത്") == "enthu"
    assert canonical("ക്ഷമിക്കണം") == "kshamikkanam"
    assert canonical("പറഞ്ഞു") == "paranju"
    assert canonical("എങ്ങനെ") == "engane"
    assert canonical("അത്ത") == "attha"
    assert canonical("കുട്ടി") == "kutti"
    assert canonical("സത്യം") == "sathyam"
    assert canonical("സ്വന്തം") == "svantham"
    assert canonical("കൃഷ്ണൻ") == "krushnan"
    assert canonical("അവൾ") == "aval"
    assert canonical("ദുഃഖം") == "duhkham"
    assert canonical("വീട്") == "veetu"  # a final virama is u
    # Old-style chillu: consonant, virama, ZWJ.
    assert canonical("അവന്‍") == "avan"
    assert canonical("പറ്റില്ല") == "pattilla"


def test_variants_cover_how_people_type():
    rng = random.Random(0)
    got = {v for _ in range(400) for v in variants("എന്റെ", rng)}
    assert {"ente", "ende", "entey", "nte"} <= got
    got = {v for _ in range(400) for v in variants("സുഖമാണോ", rng)}
    assert "sukhamano" in got and "sugamano" in got
    got = {v for _ in range(400) for v in variants("എനിക്ക്", rng)}
    assert {"enikku", "eniku", "enik"} <= got
    got = {v for _ in range(400) for v in variants("ശരി", rng)}
    assert {"sheri", "shari", "seri"} <= got


def test_variants_are_plain_latin():
    rng = random.Random(1)
    for word in ["ജിഷ്ണു", "തൃശ്ശൂർ", "ക്ഷമിക്കണം"]:
        for v in variants(word, rng):
            assert v and all("a" <= c <= "z" for c in v), v


def test_variants_contract():
    rng = random.Random(2)
    for word in ["എന്റെ", "ഞാൻ", "തിരുവനന്തപുരം", "ഈ", "ം", "ക്"]:
        for _ in range(50):
            got = variants(word, rng)
            assert 1 <= len(got) <= 4
            assert len(set(got)) == len(got)
            assert all(v and v.isascii() and v.isalpha() and v.islower() for v in got)


def test_variants_are_deterministic_given_the_rng():
    words = ["എന്റെ", "സുഖമാണോ", "തൃശ്ശൂർ", "ആലപ്പുഴ"]
    a = [variants(w, random.Random(7)) for w in words]
    b = [variants(w, random.Random(7)) for w in words]
    assert a == b


@pytest.mark.parametrize("word", ["എന്റെ", "തിരുവനന്തപുരം", "ക്ഷമിക്കണം", "സുഖമായിരുന്നു"])
def test_variants_can_produce_the_canonical_form(word):
    rng = random.Random(3)
    assert canonical(word) in {v for _ in range(200) for v in variants(word, rng)}


def test_never_crashes_on_any_malayalam_code_point():
    points = [chr(c) for c in range(0x0D00, 0x0D80)] + ["‌", "‍"]
    rng = random.Random(4)
    for p in points:
        for word in [p, "ക" + p, p + "ക", "ക്" + p, "അ" + p + "്"]:
            canonical(word)
            got = variants(word, rng)
            assert len(got) <= 4
            assert all(v.isascii() and v.isalpha() and v.islower() for v in got)
    for _ in range(2000):
        word = "".join(rng.choice(points) for _ in range(rng.randint(1, 8)))
        canonical(word)
        variants(word, rng)


def test_golden_chat_spellings_are_produced():
    """The Latin of most chat-golden rows whose target is a single lexicon word comes out of
    200 draws of `variants`. The rate is printed; tune the rules, never per word."""
    lexicon = {w for w, _ in mlengine.malayalam_words(str(LEXICON), 300_000)}
    rows = {
        (r.latin.lower(), r.malayalam)
        for r in load(GOLDEN)
        if " " not in r.malayalam and r.malayalam in lexicon
    }
    misses = []
    for latin, word in sorted(rows):
        rng = random.Random(word)
        got = {v for _ in range(200) for v in variants(word, rng)}
        if latin not in got:
            misses.append((latin, word))
    rate = 1 - len(misses) / len(rows)
    print(f"\ngolden coverage {rate:.1%} of {len(rows)}; misses: {misses}")
    assert rate >= 0.8, misses


def test_excluded_words_include_every_golden_target():
    excluded = excluded_words([GOLDEN])
    assert {"എന്റെ", "ഞാൻ", "സുഖമാണോ"} <= excluded


def test_build_splits_by_word_and_skips_excluded():
    words = [(w, c) for w, c in mlengine.malayalam_words(str(LEXICON), 3000)]
    excluded = {"ഒരു", "എന്ന"}
    train, valid = build(words, rows=20_000, valid_frac=0.05, excluded=excluded, seed=0)
    train_words = {m for _, m in train}
    valid_words = {m for _, m in valid}
    assert not train_words & valid_words
    assert not (train_words | valid_words) & excluded
    assert 18_000 <= len(train) + len(valid) <= 22_000
    assert valid
    assert all(latin.isascii() and latin.isalpha() for latin, _ in train + valid)
    # Frequent words are sampled more often than rare ones.
    counts = {}
    for _, m in train:
        counts[m] = counts.get(m, 0) + 1
    assert counts.get(words[3][0], 0) > counts.get(words[-1][0], 0)
