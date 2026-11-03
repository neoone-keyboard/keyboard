import json
import struct

import numpy as np
import pytest

torch = pytest.importorskip("torch")  # in the `train` group: uv run --group train pytest

from mlkb.translit import export  # noqa: E402
from mlkb.translit.model import Config, Translit  # noqa: E402


@pytest.fixture(scope="module")
def full(tmp_path_factory):
    torch.manual_seed(0)
    model = Translit(Config()).eval()
    path = tmp_path_factory.mktemp("export") / "translit.mlkbnn"
    export.export_model(model, path)
    return model, path


def test_full_size_file_is_at_most_5_mb(full):
    _, path = full
    assert path.stat().st_size <= 5_000_000


def test_round_trip_matrices_within_half_a_step(full):
    model, path = full
    config, tensors = export.read(path)
    assert config == {"kind": export.KIND_TRANSLIT, **{k: getattr(model.config, k) for k in export.FIELDS}}
    state = model.state_dict()
    assert list(tensors) == list(state)  # state_dict order, names exactly as PyTorch's
    for name, value in state.items():
        want = value.numpy()
        got = tensors[name]
        assert got.shape == want.shape, name
        if want.ndim == 2:
            scale = np.abs(want).max(axis=1, keepdims=True) / 127
            assert (np.abs(got - want) <= scale / 2 + 1e-7).all(), name
        else:
            assert np.array_equal(got, want), name  # biases and norms stay f32


def test_header_and_alignment(full):
    _, path = full
    data = path.read_bytes()
    assert data[:8] == b"MLKBNN01"
    version, kind = struct.unpack_from("<2I", data, 8)
    assert (version, kind) == (1, export.KIND_TRANSLIT)
    for _name, dtype, _dims, offset in export.layout(data):
        assert offset % 16 == 0
        assert dtype in (export.F32, export.I8)


def test_rejects_corruption(full, tmp_path):
    _, path = full
    data = bytearray(path.read_bytes())
    data[1000] ^= 0x01
    bad = tmp_path / "flipped.mlkbnn"
    bad.write_bytes(bytes(data))
    with pytest.raises(ValueError, match="checksum"):
        export.read(bad)
    bad.write_bytes(path.read_bytes()[:-100])
    with pytest.raises(ValueError):
        export.read(bad)


def test_dequantised_model_stays_close(full):
    model, path = full
    dequantised = export.dequantised_model(path)
    ref = export.reference(model, dequantised)
    assert len(ref["cases"]) == len(export.REFERENCE_CASES)
    for case in ref["cases"]:
        steps = len(case["tgt"])
        assert steps >= 3
        for kind in ("float", "dequantised"):
            assert np.array(case[kind]["encoder"]).shape == (len(case["src"]), model.config.d)
            assert np.array(case[kind]["logits"]).shape == (steps, model.config.tgt_vocab)
        diff = np.abs(np.array(case["float"]["logits"]) - np.array(case["dequantised"]["logits"]))
        assert diff.max() < 0.05


def test_cli_exports_a_checkpoint(tmp_path):
    torch.manual_seed(1)
    model = Translit(Config(d=32, heads=2, enc_layers=1, dec_layers=1, ff=32))
    ckpt = tmp_path / "m.pt"
    torch.save({"model": model.state_dict(), "config": model.config.to_dict(), "step": 0}, ckpt)
    out, ref = tmp_path / "m.mlkbnn", tmp_path / "m.json"
    export.main(["--checkpoint", str(ckpt), "--out", str(out), "--reference", str(ref)])
    config, _ = export.read(out)
    assert config["d"] == 32
    assert json.loads(ref.read_text())["config"]["d"] == 32


def test_fnv1a_matches_the_rust_reader():
    # Same reference values as `fnv1a_matches_the_reference_values` in engine/core/src/nn/file.rs.
    assert export.fnv1a(b"") == 0xCBF29CE484222325
    assert export.fnv1a(b"a") == 0xAF63DC4C8601EC8C


def test_committed_fixture_matches_the_generator():
    # engine/core/tests/fixtures/translit-tiny.mlkbnn must come from make_fixture.tiny_model;
    # regenerate both fixture files with `python -m mlkb.translit.make_fixture` otherwise.
    from mlkb.translit.make_fixture import FIXTURES, TINY, tiny_model

    config, tensors = export.read(FIXTURES / "translit-tiny.mlkbnn")
    assert config["d"] == TINY.d and config["enc_layers"] == TINY.enc_layers
    for name, value in tiny_model().state_dict().items():
        want = value.numpy()
        tol = np.abs(want).max(axis=1, keepdims=True) / 254 + 1e-6 if want.ndim == 2 else 0
        assert (np.abs(tensors[name] - want) <= tol).all(), name
    ref = json.loads((FIXTURES / "translit-tiny-reference.json").read_text())
    assert [c["latin"] for c in ref["cases"]] == [latin for latin, _ in export.REFERENCE_CASES[:4]]
