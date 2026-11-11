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


def test_held_out_sentences_split_into_disjoint_tune_and_gate_halves():
    sentences_ = [f"ഇത് {i} ആം വാക്യം ആണ് ശരി" for i in range(200)] + ["ഇത് ഒരു വാക്യം ആണ്"] * 3
    halves = [corpus.is_tune_half(s) for s in sentences_]
    assert 60 < sum(halves) < 140
    assert len({corpus.is_tune_half(s) for s in sentences_[-3:]}) == 1  # a sentence always lands on one side


def test_held_out_sentences_seen_in_training_text_are_dropped():
    train = corpus.sentence_keys(["ഞാൻ ഇന്ന് വീട്ടിൽ പോകും. അവൻ വന്നു"])
    kept = corpus.not_in(["ഞാൻ ഇന്ന് വീട്ടിൽ പോകും", "അവൻ വന്നു.", "അവൾ പോയി."], train)
    assert kept == ["അവൾ പോയി."]


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


# --- Hindi (mlkb.lm.lang "hi") ------------------------------------------------------------

from mlkb.lm import lang as lm_lang  # noqa: E402

HI = lm_lang.load("hi")
HI_LOAN = {"कंप्यूटर": "computer"}


def test_malayalam_keeps_its_paths_and_buckets():
    ml = lm_lang.load()
    assert ml.id == "ml" and ml.build == "build/lm" and ml.checkpoint == "build/lm.pt"
    assert ml.romanised_buckets() == ["manglish-0", "manglish-1", "manglish-2", "manglish-3"]
    assert ml.mixed_buckets() == ["mixed-0", "mixed-1"]
    assert ml.word.pattern == corpus._MALAYALAM.pattern
    assert ml.eval_dir == corpus.EVAL


def test_hindi_builds_apart_from_malayalam():
    assert HI.build == "build/lm-hi" and HI.checkpoint == "build/lm-hi.pt"
    assert HI.romanised_buckets() == ["hinglish-0", "hinglish-1"]
    assert HI.tune_file.parent == HI.eval_dir == corpus.EVAL / "hi"


def test_a_pack_language_trains_on_the_sources_its_manifest_credits():
    import json

    from mlkb.lm import lang

    for lang_id in ("as", "ta"):
        config = lang.load(lang_id)
        assert [row[0] for row in corpus.english_sources(config)] == ["en-wiki"]
        assert config.dakshina is None and config.dakshina_romanized is None
        manifest = json.loads((config.pack / "manifest.json").read_text(encoding="utf-8"))
        credits = {c["name"] for c in manifest["credits"]}
        assert {"English Wikipedia", f"{config.name} Wikipedia", f"{config.name} Wikisource",
                f"{config.name} Wikiquote", "Aksharantar (AI4Bharat)"} <= credits
        assert not {"Simple English Wikipedia", "English Wikinews", "Project Gutenberg"} & credits
    assert corpus.english_sources(HI) == corpus.ENGLISH


def test_a_language_without_dakshina_reads_only_its_eval_sets_for_leaks(tmp_path):
    (tmp_path / "next-romanised.tsv").write_text("moi ghorot ahim\tমই ঘৰত আহিম\n", encoding="utf-8")
    keys = corpus.leak_keys(tmp_path, None, own=set(), dakshina_lang=None)
    assert is_leak("মই ঘৰত আহিম।", keys) and is_leak("moi ghorot ahim", keys)


def test_assamese_and_tamil_are_romanised_by_their_packs_tables(tmp_path):
    from mlkb.lm import lang

    for lang_id, text in (("as", "মই কাইলৈ ঘৰত আহিম। ক'ত আছা?\n"), ("ta", "நான் நாளை வீட்டுக்கு வருவேன். அவன் வந்தான்.\n")):
        src = tmp_path / f"{lang_id}-clean.txt"
        src.write_text(text, encoding="utf-8")
        corpus._init_worker({}, lang_id)
        corpus.synthesise(("romanised", str(src), str(tmp_path / f"{lang_id}.txt"), 1, 0, 1))
        roman = (tmp_path / f"{lang_id}.txt").read_text(encoding="utf-8")
        assert roman.isascii() and len(roman.split()) == len(text.split()), roman
    assamese = lang.load("as")
    assert assamese.word.findall("ক'ত আছা ১২ জন।") == ["ক'ত", "আছা", "জন"]
    assert corpus.tune_pattern(assamese).fullmatch("মই কাইলৈ ঘৰত আহিম।")
    assert not corpus.tune_pattern(assamese).fullmatch("মই ১২ ঘৰত")
    assert "Assamese Wikiquote" in corpus.headers(assamese)[1]
    assert corpus.headers(HI) == corpus.HEADERS["hi"]
    corpus._init_worker({}, "ml")


def test_hindi_words_leave_the_danda_and_digits_out():
    assert HI.word.findall("मैं घर गया। २०२६ में") == ["मैं", "घर", "गया", "में"]


def test_hinglish_sentences_are_latin_keep_punctuation_and_use_loanwords():
    spell = corpus.speller(HI)
    out = romanise_sentence("मैं कंप्यूटर देख रहा हूँ।", random.Random(0), HI_LOAN, 1.0, word=HI.word, spell=spell)
    words = out.split()
    assert words[1] == "computer" and len(words) == 5
    assert out.endswith("।") and out[:-1].isascii()


def test_hindi_mixed_sentences_switch_some_words_only():
    spell = corpus.speller(HI)
    out = mix_sentence("मैं आज घर जाऊंगा", random.Random(1), HI_LOAN, 0.5, 0.0, word=HI.word, spell=spell).split()
    assert 0 < sum(w.isascii() for w in out) < 4 and len(out) == 4


def test_hindi_loanwords_skip_inflected_forms():
    rows = [{"source": "IndicCorp", "english word": "computer", "native word": "कंप्यूटर", "score": 0.0},
            {"source": "IndicCorp", "english word": "computers", "native word": "कंप्यूटरों", "score": 0.0}]
    got = loanwords_from(rows, {"computer", "computers"}, {"कंप्यूटर", "कंप्यूटरों"}, endings=HI.loanword_endings)
    assert got == {"कंप्यूटर": "computer"}


def test_hindi_leak_keys_read_its_eval_sets_and_its_dakshina_sentences(tmp_path):
    top, own_dir, dakshina = tmp_path / "eval", tmp_path / "eval/hi", tmp_path / "dakshina"
    own_dir.mkdir(parents=True)
    dakshina.mkdir()
    (top / "english-sentences.txt").write_text("we went home today\n", encoding="utf-8")
    (own_dir / "next-hinglish.tsv").write_text("main ghar gaya\tमैं घर गया\n", encoding="utf-8")
    (own_dir / "next-hi.tune.txt").write_text("यह वाक्य अपना है\n", encoding="utf-8")
    (dakshina / "hi.romanized.rejoined.dev.native.txt").write_text("वह कल आएगा\n", encoding="utf-8")
    (dakshina / "ml.romanized.rejoined.dev.native.txt").write_text("അവൻ നാളെ വരും\n", encoding="utf-8")
    keys = corpus.leak_keys(top, dakshina, own={own_dir / "next-hi.tune.txt"}, more=(own_dir,), dakshina_lang="hi")
    for leak in ("we went home today", "मैं घर गया", "main ghar gaya", "वह कल आएगा।"):
        assert is_leak(leak, keys), leak
    assert not is_leak("यह वाक्य अपना है", keys)
    assert not is_leak("അവൻ നാളെ വരും", keys)


def test_malayalam_leak_keys_are_as_before(tmp_path):
    top, dakshina = tmp_path / "eval", tmp_path / "dakshina"
    top.mkdir()
    dakshina.mkdir()
    (top / "next-ml.tune.txt").write_text("ഇത് എന്റെ വീട് ആണ്\n", encoding="utf-8")
    (top / "chat-golden.tsv").write_text("ente veedu\tente\tഎന്റെ\n", encoding="utf-8")
    (dakshina / "ml.romanized.rejoined.dev.native.txt").write_text("അവൻ നാളെ വരും\n", encoding="utf-8")
    keys = corpus.leak_keys(top, dakshina)
    assert is_leak("അവൻ നാളെ വരും", keys) and is_leak("ente veedu ente", keys)
    assert not is_leak("ഇത് എന്റെ വീട് ആണ്", keys)


def test_a_pass_built_in_parts_holds_every_paragraph_once(tmp_path):
    src = tmp_path / "hi-clean.txt"
    paras = [f"मैं {i} बार घर गया" for i in range(40)] + ["only english here"]
    src.write_text("\n".join(paras) + "\n", encoding="utf-8")
    corpus._init_worker(HI_LOAN, "hi")
    parts = [corpus.synthesise(("romanised", str(src), str(tmp_path / f"p{k}.txt"), 7, k, 3)) for k in range(3)]
    lines = [line for k in range(3) for line in (tmp_path / f"p{k}.txt").read_text(encoding="utf-8").splitlines()]
    assert len(lines) == 40 and all(line.isascii() for line in lines)
    assert sum(stats["skipped_not_native"] for _, stats in parts) == 1


def test_hindi_tune_sentences_may_end_with_a_danda():
    p = corpus.tune_pattern(HI)
    assert p.fullmatch("मैं आज घर जा रहा हूँ।")
    assert not p.fullmatch("मैं 2026 में घर गया।")
    assert corpus.tune_pattern(lm_lang.load()) is corpus._TUNE_ML


def test_malayalam_synthetic_jobs_keep_their_files_and_seeds(tmp_path):
    jobs = corpus.synthetic_jobs(lm_lang.load(), tmp_path / "work", tmp_path / "text", 3)
    got = [(kind, Path(dst).name, seed, parts) for kind, _, dst, seed, _, parts in jobs]
    assert got == [("romanised", "manglish-0.txt", 30, 1), ("romanised", "manglish-1.txt", 31, 1),
                   ("romanised", "manglish-2.txt", 32, 1), ("romanised", "manglish-3.txt", 33, 1),
                   ("mixed", "mixed-0.txt", 35, 1), ("mixed", "mixed-1.txt", 36, 1),
                   ("romanised", "valid-manglish.txt", 38, 1), ("mixed", "valid-mixed.txt", 39, 1)]


def test_hindi_parts_are_joined_into_one_file_per_pass(tmp_path):
    src = tmp_path / "work" / "hi-clean.txt"
    src.parent.mkdir()
    src.write_text("\n".join(f"मैं {i} बार घर गया" for i in range(30)) + "\n", encoding="utf-8")
    (tmp_path / "work" / "valid-hi-clean.txt").write_text("वह कल आएगा\n", encoding="utf-8")
    (tmp_path / "text").mkdir()
    jobs = corpus.synthetic_jobs(HI, tmp_path / "work", tmp_path / "text", 3)
    corpus._init_worker(HI_LOAN, "hi")
    summary = corpus.join_parts(jobs, [corpus.synthesise(j) for j in jobs])
    assert sorted(p.name for p in (tmp_path / "text").iterdir()) == [
        "hinglish-0.txt", "hinglish-1.txt", "mixed-0.txt", "valid-hinglish.txt", "valid-mixed.txt"]
    assert summary["hinglish-0"]["paragraphs"] == 30 and summary["valid-hinglish"]["paragraphs"] == 1
    assert len((tmp_path / "text" / "hinglish-1.txt").read_text(encoding="utf-8").splitlines()) == 30


def test_dakshina_sentences_of_either_language():
    from mlkb.lm import sources

    assert sources.dakshina_files() == sources.DAKSHINA_FILES
    assert sources.dakshina_files("hi") == [f.replace("/ml/", "/hi/").replace("ml.roman", "hi.roman")
                                             for f in sources.DAKSHINA_FILES]


def test_hinglish_ends_sentences_with_a_full_stop_not_a_danda(tmp_path):
    src = tmp_path / "hi-clean.txt"
    src.write_text("मैं घर गया। वह आया॥\n", encoding="utf-8")
    corpus._init_worker({}, "hi")
    corpus.synthesise(("romanised", str(src), str(tmp_path / "r.txt"), 1, 0, 1))
    corpus.synthesise(("mixed", str(src), str(tmp_path / "m.txt"), 1, 0, 1))
    roman = (tmp_path / "r.txt").read_text(encoding="utf-8")
    assert roman.isascii(), roman
    assert roman.count(".") in (0, 2)  # 0 when the chat transform dropped punctuation
