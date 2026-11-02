import pytest

torch = pytest.importorskip("torch")  # in the `train` group: uv run --group train pytest

from mlkb.translit.decode import beam, greedy  # noqa: E402
from mlkb.translit.model import Config, Translit  # noqa: E402
from mlkb.translit.vocab import LATIN, MALAYALAM, decode_ml, encode_latin, encode_ml  # noqa: E402


def test_vocab_round_trip():
    assert decode_ml(encode_ml("എന്റെ")) == "എന്റെ"
    assert encode_latin("ente")[0] == LATIN.bos
    assert encode_latin("ente")[-1] == LATIN.eos
    assert decode_ml(encode_ml("അവൻ") + [MALAYALAM.pad]) == "അവൻ"
    assert decode_ml(encode_ml("ക്‌ട")) == "ക്‌ട"  # ZWNJ is in the vocabulary


def test_shapes_and_param_count():
    m = Translit(Config())
    src = torch.tensor([encode_latin("ente")])
    tgt = torch.tensor([encode_ml("എന്റെ")[:-1]])
    assert m(src, tgt).shape == (1, tgt.shape[1], len(MALAYALAM))
    assert 3_000_000 <= sum(p.numel() for p in m.parameters()) <= 5_000_000


def test_padding_does_not_change_the_logits():
    torch.manual_seed(0)
    m = Translit(Config()).eval()
    src = torch.tensor([encode_latin("ente")])
    tgt = torch.tensor([encode_ml("എന്റെ")[:-1]])
    padded_src = torch.cat([src, torch.full((1, 3), LATIN.pad)], dim=1)
    padded_tgt = torch.cat([tgt, torch.full((1, 2), MALAYALAM.pad)], dim=1)
    with torch.no_grad():
        a = m(src, tgt)
        b = m(padded_src, padded_tgt)[:, : tgt.shape[1]]
    assert torch.allclose(a, b, atol=1e-5)


def test_decoder_is_causal():
    torch.manual_seed(0)
    m = Translit(Config()).eval()
    src = torch.tensor([encode_latin("ente")])
    tgt = torch.tensor([encode_ml("എന്റെ")[:-1]])
    changed = tgt.clone()
    changed[0, -1] = MALAYALAM.index("ക")
    with torch.no_grad():
        a, b = m(src, tgt), m(src, changed)
    assert torch.allclose(a[:, :-1], b[:, :-1], atol=1e-5)


def test_overfits_one_pair():
    torch.manual_seed(0)
    m = Translit(Config())
    opt = torch.optim.Adam(m.parameters(), 1e-3)
    src = torch.tensor([encode_latin("ente")])
    full = torch.tensor([encode_ml("എന്റെ")])
    for _ in range(200):
        loss = torch.nn.functional.cross_entropy(m(src, full[:, :-1]).transpose(1, 2), full[:, 1:])
        opt.zero_grad()
        loss.backward()
        opt.step()
    assert beam(m, "ente", width=4)[0][0] == "എന്റെ"
    assert greedy(m, ["ente"]) == ["എന്റെ"]


def test_beam_returns_distinct_sorted_candidates():
    torch.manual_seed(0)
    m = Translit(Config(d=64, heads=2, enc_layers=1, dec_layers=1, ff=128))
    got = beam(m, "ente", width=4, max_len=8)
    assert 1 <= len(got) <= 4
    assert len({text for text, _ in got}) == len(got)
    assert all(lp <= 0 for _, lp in got)


def test_training_saves_the_best_checkpoint_and_resumes(tmp_path, capsys):
    from mlkb.translit import train

    pairs = tmp_path / "pairs"
    pairs.mkdir()
    rows = [("ente", "എന്റെ"), ("njan", "ഞാൻ"), ("avan", "അവൻ"), ("nanni", "നന്ദി")] * 8
    (pairs / "train.tsv").write_text("".join(f"{a}\t{b}\n" for a, b in rows), encoding="utf-8")
    (pairs / "valid.tsv").write_text("ente\tഎന്റെ\nwayanad\tവയനാട്\n", encoding="utf-8")
    out = tmp_path / "t.pt"
    common = ["--pairs", str(pairs), "--out", str(out), "--batch", "8", "--warmup", "2", "--eval-every", "2"]
    assert train.main([*common, "--steps", "4", "--device", "cpu"]) == 0
    last = torch.load(tmp_path / "t-last.pt", weights_only=False)
    assert last["step"] == 4 and len(last["history"]) == 2
    best = torch.load(out, weights_only=False)
    assert set(best) == {"model", "config", "step", "score"}
    assert train.main([*common, "--steps", "6", "--device", "cpu"]) == 0
    assert "resumed from" in capsys.readouterr().out
    assert torch.load(tmp_path / "t-last.pt", weights_only=False)["step"] == 6


def test_schedule_warms_up_then_decays():
    from mlkb.translit.train import schedule

    assert schedule(0, 10, 100) == 0.1
    assert schedule(9, 10, 100) == 1.0
    assert abs(schedule(100, 10, 100) - 0.1) < 1e-9
    assert schedule(50, 10, 100) < schedule(20, 10, 100)
