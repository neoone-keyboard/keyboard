"""Language configs (mlkb/langs/*.json, data/langs/as), pack-language romanisation tables, the
Assamese normal form, and the native vocabularies."""

import random

import pytest

from mlkb import langs
from mlkb.data import human as data_human
from mlkb.langs import human
from mlkb.langs.calibrate import align, misses
from mlkb.langs.format1 import Format1, export
from mlkb.langs.romanisation import LANGS, Tables
from mlkb.translit.vocab import BENGALI_ASSAMESE, MALAYALAM, decode_ml, encode_ml, fits, native


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
    assert data_human.clean("Kya", "क्या", "hi") == ("kya", "क्या")
    assert data_human.clean("kya", "क्या।", "hi") is None, "a danda is not a letter"
    assert data_human.clean("kya", "എന്റെ", "hi") is None
    # NFC: a precomposed nukta letter becomes consonant + nukta.
    assert data_human.clean("zindagi", "ज़िंदगी", "hi") == ("zindagi", "ज़िंदगी")


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


def test_format1_optional_finals_give_a_second_key():
    f = Format1.load("as")
    assert f.keys("মানুহ") == [f.key("মানুহ")]
    hindi = Format1.load(langs.load("hi").pack / "romanisation.json")
    assert hindi.keys("हूँ") == ["hun", "hu"]
    assert hindi.keys("दिन") == ["din"]


def test_format1_export_matches_the_weighted_canonical_spellings():
    # Without the contexts the format can't hold, the two walks agree on plain words.
    f, t = Format1(export(Tables.load("as"))), Tables.load("as")
    for word in ["মই", "তুমি", "কেনে", "ভাল", "মানুহ", "ক'ত", "ৰজা", "ক্ষমা"]:
        assert f.romanize(word) == t.canonical(word), word


def test_format1_notes_and_context_clusters_come_from_the_hand_file(tmp_path):
    import json

    weights = Tables.load("as")
    as_export = export(weights)
    assert "oxom" in as_export["notes"] and "শ্ৰ" in as_export["clusters"]
    # Another language: its own hand file, no Assamese notes or sibilant clusters.
    hand = json.loads((LANGS / "as" / "romanisation.hand.json").read_text(encoding="utf-8"))
    hand["notes"] = {"inherent": "Kept at the end of a word."}
    del hand["inClusterLetters"]
    (tmp_path / "romanisation.hand.json").write_text(json.dumps(hand, ensure_ascii=False), encoding="utf-8")
    other = export(Tables(weights.data, tmp_path / "romanisation-weights.json"))
    assert other["notes"] == "inherent: Kept at the end of a word."
    assert "শ্ৰ" not in other["clusters"]
    del hand["notes"]
    (tmp_path / "romanisation.hand.json").write_text(json.dumps(hand, ensure_ascii=False), encoding="utf-8")
    assert export(Tables(weights.data, tmp_path / "romanisation-weights.json"))["notes"] == ""


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
    assert native("as") == BENGALI_ASSAMESE  # from data/langs/as/lang.json
    for word in ["অসমীয়া", "হ'ল", "ৰৱ"]:
        assert decode_ml(encode_ml(word, BENGALI_ASSAMESE), BENGALI_ASSAMESE) == word
    assert fits("hol", "হ'ল", vocab=BENGALI_ASSAMESE)
    assert not fits("hol", "হ'ল")  # not Malayalam
    # The Malayalam ids are unchanged.
    assert encode_ml("എ") == [1, 3 + 0x0D0E - 0x0D00, 2]


# ---- calibration pairs when Aksharantar's valid split has no manual rows (Marathi) ----


def _hp(native: str, source: str = "AK-Freq", latin: str = "x") -> human.HumanPair:
    return human.HumanPair(latin, native, source)


def test_calibration_uses_the_valid_splits_manual_rows():
    valid = [_hp("a1", "AK-Uni"), _hp("a2", "IndicCorp")]
    rows, where = human.calibration_split(valid, [_hp("t1")], set(), "xyz")
    assert [h.native for h in rows] == ["a1"]
    assert "xyz_valid.json" in where


def test_calibration_falls_back_to_a_held_out_slice_of_train():
    train = [_hp(f"w{i}") for i in range(4000)] + [_hp("mined", "IndicCorp")]
    test_words = {f"w{i}" for i in range(0, 4000, 2)}
    rows, where = human.calibration_split([_hp("v", "IndicCorp")], train, test_words, "xyz")
    words = {h.native for h in rows}
    assert "xyz_train.json" in where and "xyz_valid.json has none" in where
    assert not words & test_words, "never a test word"
    assert "mined" not in words, "manual rows only"
    # A slice, not all of train: the rest stays for training. About a quarter of the 2,000 left.
    assert 300 < len(rows) < 700
    # Deterministic, and chosen by word, so a word's rows go together.
    again, _ = human.calibration_split([], list(reversed(train)), test_words, "xyz")
    assert {h.native for h in again} == words


def test_calibration_falls_back_to_the_valid_splits_mined_rows():
    # Telugu: manual rows only in the test split. The valid split's IndicCorp rows calibrate.
    valid = [_hp("v1", "IndicCorp"), _hp("v2", "IndicCorp"), _hp("name", "Samanantar"), _hp("t", "IndicCorp")]
    rows, where = human.calibration_split(valid, [_hp("m", "IndicCorp")], {"t"}, "xyz")
    assert [h.native for h in rows] == ["v1", "v2"], "IndicCorp only, never a test word, nothing from train"
    assert "mined IndicCorp rows of xyz_valid.json" in where and "rough" in where


def test_no_calibration_pairs_is_an_error():
    with pytest.raises(SystemExit, match="no calibration pairs"):
        human.calibration_split([_hp("v", "Samanantar")], [_hp("m", "IndicCorp")], set(), "xyz")


def _fake_aksharantar(monkeypatch, rows: dict[str, list]) -> None:
    monkeypatch.setattr(human, "aksharantar", lambda lang, split="train", stats=None: rows.get(split, []))


def test_calibrate_stops_on_zero_pairs(monkeypatch):
    from mlkb.langs import calibrate

    _fake_aksharantar(monkeypatch, {"train": [_hp("অসম", "IndicCorp")]})
    with pytest.raises(SystemExit, match="no calibration pairs"):
        calibrate.main(["--lang", "as"])


def test_calibrate_report_names_what_to_tune_on(monkeypatch, capsys):
    from mlkb.langs import calibrate

    train = [_hp("অসম", latin="oxom"), _hp("মই", latin="moi")] * 20
    _fake_aksharantar(monkeypatch, {"train": train, "test": [_hp("তুমি", latin="tumi")]})
    monkeypatch.setattr(human, "in_calibration_slice", lambda word, share=human.CALIBRATION_SHARE: True)
    calibrate.main(["--lang", "as", "--iterations", "1"])
    out = capsys.readouterr().out
    assert "asm_train.json" in out, "says which split it calibrated on"
    assert "tune on these" in out and "never tune on these" in out
    assert "only 40 calibration pairs: one pair is 2.5 points" in out


def test_pairs_stops_on_zero_calibration_pairs(monkeypatch, tmp_path):
    from mlkb.langs import pairs

    _fake_aksharantar(monkeypatch, {"train": [_hp("অসম", "IndicCorp")]})
    with pytest.raises(SystemExit, match="no calibration pairs"):
        pairs.main(["--lang", "as", "--out", str(tmp_path)])


def test_a_missing_aksharantar_file_names_the_step(monkeypatch, tmp_path):
    monkeypatch.setattr(human, "RAW", tmp_path)
    with pytest.raises(SystemExit, match="build-as-text.sh"):
        human.aksharantar("as", "valid")


def test_misses_lists_what_the_tables_cannot_spell_or_find(tables):
    native = "অসম"
    good = tables.canonical(native)
    found = misses(tables, [(native, good), (native, "qqq"), (native, good + "ir")])
    assert [(m.latin, m.aligns) for m in found] == [("qqq", False), (good + "ir", False)]
    assert all(m.native == native and m.canonical == good and m.key == tables.key(native) for m in found)
    assert found[0].typed_key == tables.skeleton("qqq")


def test_calibrate_misses_prints_the_failing_pairs(monkeypatch, capsys):
    from mlkb.langs import calibrate

    rows = [human.HumanPair("oxom", "অসম", "AK-Freq"), human.HumanPair("zzz", "অসম", "AK-Freq")]
    monkeypatch.setattr(calibrate.human, "aksharantar", lambda lang, split, stats=None: rows)
    calibrate.main(["--lang", "as", "--misses"])
    out = capsys.readouterr().out
    assert "misses: 1 of 2 calibration pairs" in out
    assert "অসম\tzzz\t" in out and "does not align" in out


def test_the_skeletons_optional_steps_come_from_the_hand_file():
    import copy

    weights = Tables.load("as")
    # A hand file without them exports as before: no letters to drop, no dropBetweenConsonants.
    plain = export(weights)["skeleton"]
    assert plain["dropTrailing"] == {"letters": "", "afterVowel": True, "minLength": 3}
    assert "dropBetweenConsonants" not in plain
    # With them (Gujarati: a final n or m after a vowel, an a between consonants), the export
    # has the format's fields, and the weighted tables and the pack's file give the same keys.
    data = copy.deepcopy(weights.data)
    data["skeleton"]["dropTrailing"] = {"letters": "nm", "afterVowel": True, "minLength": 3}
    data["skeleton"]["dropBetweenConsonants"] = "a"
    data["skeleton"]["dropFinal"] = ""
    data["skeleton"]["alsoBetweenConsonants"] = "u"
    t = Tables(data)
    f = Format1(export(t))
    assert "alsoBetweenConsonants" not in plain
    assert f.data["skeleton"]["alsoBetweenConsonants"] == "u"
    assert f.data["skeleton"]["dropTrailing"]["letters"] == "nm"
    assert f.data["skeleton"]["dropBetweenConsonants"] == "a"
    assert list(f.data["skeleton"])[-1] == "dropFinalVowel"
    for typed, key in [
        ("karnun", "krnu"), ("karnum", "krnu"), ("karnu", "krnu"), ("karanu", "krnu"),
        ("an", "an"),  # shorter than minLength: kept
        ("bharat", "brt"), ("amar", "amr"), ("kaam", "ka"), ("tr", "tr"), ("", ""),
    ]:
        assert t.skeleton(typed) == key and f.skeleton(typed) == key, typed
