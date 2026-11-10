import numpy as np
import torch
from mlkb.lm.export import export, forward_masked, tables_for
from mlkb.lm.model import LM, Config
from mlkb.lm.tables import fingerprint, p_en
from mlkb.translit.export import read


def test_export_round_trips_and_stays_small(tmp_path):
    torch.manual_seed(0)
    m = LM(Config())
    pieces = ["<pad>", "<unk>", "<s>", "</s>"] + [f"<0x{b:02X}>" for b in range(256)]
    pieces += [f"▁w{i}" for i in range(16384 - len(pieces))]
    scores = [0.0] * len(pieces)
    tables = {"tok.p_en": np.full(16384, 0.5, np.float32),
              "lex.first_en": np.zeros(16384, np.float32), "lex.first_ml": np.zeros(16384, np.float32)}
    path = tmp_path / "lm.mlkbnn"
    export(m, pieces, scores, tables, fingerprint=bytes(16), path=path)
    assert path.stat().st_size <= 10_000_000
    config, tensors = read(path)
    assert config["kind"] == 2 and config["tgt_vocab"] == 16384 and config["enc_layers"] == 0
    w = m.state_dict()["layers.0.qkv.weight"].numpy()
    scale = np.abs(w).max(axis=1, keepdims=True) / 127
    assert np.all(np.abs(tensors["layers.0.qkv.weight"] - w) <= scale / 2 + 1e-7)
    assert tensors["tok.pieces"].split(b"\0")[4] == b"<0x00>"


def test_first_token_tables_share_out_the_lexicon():
    words = [("the", 60), ("then", 30), ("cat", 10)]
    first = {"the": 7, "then": 7, "cat": 8}  # word -> its first token id
    t = tables_for(words, lambda w: first[w], vocab=10)
    assert np.isclose(np.exp(t[7]), 0.9) and np.isclose(np.exp(t[8]), 0.1)
    assert np.isneginf(t[0])


def test_p_en_compares_shares_of_latin_tokens(tmp_path):
    # ids 4, 5 Latin; 6 Malayalam script. English: 4 x3, 5 x1. Manglish: 4 x1, 5 x1, 6 x2.
    np.array([2, 4, 4, 4, 5], "<u2").tofile(tmp_path / "en-wiki.bin")
    np.array([2, 4, 5, 6, 6], "<u2").tofile(tmp_path / "manglish-0.bin")
    np.array([2, 4, 4, 4, 4], "<u2").tofile(tmp_path / "valid-en-wiki.bin")  # never counted
    latin = np.array([False] * 4 + [True, True, False, False])
    p = p_en(tmp_path, 8, latin)
    assert np.isclose(p[4], 0.75 / (0.75 + 0.5)) and np.isclose(p[5], 0.25 / (0.25 + 0.5))
    assert p[6] == 0.5 and p[7] == 0.5


def test_fingerprint_is_both_files_fnv(tmp_path):
    (tmp_path / "en.fst").write_bytes(b"")
    (tmp_path / "ml.fst").write_bytes(b"a")
    fp = fingerprint(tmp_path / "en.fst", tmp_path / "ml.fst")
    assert fp == (0xCBF29CE484222325).to_bytes(8, "little") + (0xAF63DC4C8601EC8C).to_bytes(8, "little")


def test_masked_forward_with_a_causal_mask_is_the_model():
    torch.manual_seed(0)
    m = LM(Config(vocab=300, d=64, heads=2, layers=2, ff=128)).eval()
    x = torch.randint(4, 300, (1, 12))
    causal = torch.ones(12, 12, dtype=torch.bool).tril()
    with torch.no_grad():
        assert torch.allclose(forward_masked(m, x[0], causal), m(x)[0], atol=1e-5)


def test_p_en_counts_the_languages_romanised_shards(tmp_path):
    from mlkb.lm import lang

    np.array([2, 4, 4, 4, 5], "<u2").tofile(tmp_path / "en-wiki.bin")
    np.array([2, 4, 5, 6, 6], "<u2").tofile(tmp_path / "hinglish-0.bin")
    np.array([2, 5, 5, 5, 5], "<u2").tofile(tmp_path / "manglish-0.bin")  # another language's: never counted
    latin = np.array([False] * 4 + [True, True, False, False])
    p = p_en(tmp_path, 8, latin, lang.load("hi"))
    assert np.isclose(p[4], 0.75 / (0.75 + 0.5)) and np.isclose(p[5], 0.25 / (0.25 + 0.5))


def test_latin_pieces_leave_out_the_native_script():
    from mlkb.lm import lang
    from mlkb.lm.tables import latin_pieces

    pieces = ["<pad>", "<unk>", "<s>", "</s>", "▁the", "▁हम", "▁हमa", "▁എa", "<0x41>", "."]
    assert latin_pieces(pieces).tolist() == [False] * 4 + [True, False, True, False, False, False]
    assert latin_pieces(pieces, lang.load("hi")).tolist() == [False] * 4 + [True, False, False, True, False, False]


def test_reference_texts_are_the_languages():
    from mlkb.lm import lang
    from mlkb.lm.export import LONG_TEXT, REFERENCE_TEXTS

    assert lang.load().reference_texts == REFERENCE_TEXTS and lang.load().long_text == LONG_TEXT
    assert any("मैं" in t for t in lang.load("hi").reference_texts)
