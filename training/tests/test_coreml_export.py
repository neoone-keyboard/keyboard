"""The Core ML spike's re-implemented forward pass, against `Translit` in PyTorch. The
conversion itself needs coremltools (the `spike` group) and is checked by `make spike-assets`."""

import pytest

torch = pytest.importorskip("torch")  # in the `train` group: uv run --group train pytest

from mlkb.translit import coreml_export as cx  # noqa: E402
from mlkb.translit.make_fixture import tiny_model  # noqa: E402

WORDS = ([1, 10, 16, 22, 3, 2], [1, 5, 9, 2])
TOL = 1e-4


def _encode(model, word):
    src = torch.zeros(1, cx.SRC_LEN, dtype=torch.int32)
    src[0, : len(word)] = torch.tensor(word)
    mask = cx.masks(len(word), cx.SRC_LEN)
    return src, mask


@torch.no_grad()
def test_encoder_cross_attention_matches_translit():
    model = tiny_model()
    c = model.config
    for word in WORDS:
        src, mask = _encode(model, word)
        k, v = cx.Encoder(model)(src, mask)
        memory, _ = model.encode(torch.tensor([word]))
        d, hd = c.d, c.d // c.heads
        for i, layer in enumerate(model.dec):
            w, b = layer.multihead_attn.in_proj_weight, layer.multihead_attn.in_proj_bias
            want_k = (memory @ w[d : 2 * d].T + b[d : 2 * d]).view(1, -1, c.heads, hd).transpose(1, 2)
            assert (k[i, :, :, : len(word)] - want_k).abs().max() < TOL


@torch.no_grad()
def test_stateful_steps_match_full_prefix_decoding_and_forks():
    model = tiny_model()
    word = WORDS[0]
    src, src_mask = _encode(model, word)
    cross_k, cross_v = cx.Encoder(model)(src, src_mask)
    memory, pad = model.encode(torch.tensor([word]))
    step = cx.DecoderStep(model, beams=2, cache_len=model.config.max_len)
    size = model.config.max_len
    # Beam 0 follows `a`, beam 1 follows `b`; at step 3 both beams continue beam 1's history.
    a, b = [1, 5, 9, 12, 7, 30], [1, 6, 8, 40, 11, 50]
    for i in range(len(a)):
        parent = torch.eye(2) if i != 3 else torch.tensor([[0.0, 1.0], [0.0, 1.0]])
        if i == 3:
            a = b[:3] + a[3:]
        logits = step(
            torch.tensor([a[i], b[i]], dtype=torch.int32),
            cx.one_hot(i, size),
            cx.masks(i + 1, size),
            parent,
            cross_k,
            cross_v,
            src_mask,
        )
        for beam, tokens in enumerate((a, b)):
            want = model.decode(torch.tensor([tokens[: i + 1]]), memory, pad)[0, -1]
            assert (logits[beam] - want).abs().max() < TOL, (i, beam)
