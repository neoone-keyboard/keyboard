import numpy as np
import mlengine


def tiny():
    pieces = ["<pad>", "<unk>", "<s>", "</s>"] + [f"<0x{b:02X}>" for b in range(256)]
    scores = [0.0] * len(pieces)
    for p, s in [("▁", -3.0), ("▁the", -2.0), ("▁then", -4.0), ("n", -3.5)]:
        pieces.append(p); scores.append(s)
    return mlengine.Tokenizer(pieces, scores)


def test_python_sees_the_rust_tokeniser():
    t = tiny()
    assert [t.piece(i) for i in t.encode("The then")] == ["▁the", "▁then"]
    assert mlengine.lm_normalize("Hello  WORLD") == "hello world"


def test_encode_file_writes_bos_then_tokens(tmp_path):
    src, dst = tmp_path / "a.txt", tmp_path / "a.bin"
    src.write_text("the\n\nthen the\n", encoding="utf-8")
    assert tiny().encode_file(str(src), str(dst)) == 5
    ids = np.fromfile(dst, dtype="<u2").tolist()
    t = tiny()
    assert ids == [2, *t.encode("the"), 2, *t.encode("then the")]


def test_the_vocabulary_sample_reads_the_languages_files():
    from mlkb.lm import lang, tokenizer

    ml, hi = tokenizer.targets(), tokenizer.targets(lang.load("hi"))
    assert set(ml) - set(hi) == {"ml.txt", "manglish-0.txt"}
    assert set(hi) - set(ml) == {"hi.txt", "hinglish-0.txt"}
    assert sorted(ml.values()) == sorted(hi.values())
    assert tokenizer.valid_files() == tokenizer.VALID_FILES
    assert "valid-hinglish.txt" in tokenizer.valid_files(lang.load("hi"))
