"""The runtime spike's models: one seeded random full-size transliterator, written both as
`.mlkbnn` (for the Rust forward pass) and as an int8 Core ML model, so the app can time the two
on the device.

    make spike-assets   # from the repo root; or, from training/:
    UV_PROJECT_ENVIRONMENT=build/spike-venv uv run --python 3.13 --group spike --group train \
        python -m mlkb.translit.coreml_export --out-dir DIR

(coremltools 9.0 ships no native libraries for Python 3.14, the main environment's version, so
the conversion runs in a separate Python 3.13 environment.)

Writes `DIR/translit-spike.mlkbnn` and `DIR/translit-spike.mlmodelc` (compiled, so Xcode copies
it as is). The Core ML model has two functions:
- `encoder`: `src [1, SRC_LEN]` int32 token ids (padded with `pad`) and `src_mask [1, SRC_LEN]`
  (0 at real tokens, -1e4 at padding) in; `cross_k`, `cross_v [dec_layers, 1, heads, SRC_LEN,
  head_dim]` out: the decoder's cross-attention keys and values, computed once per word.
- `decoder`: one step for `BEAMS` beams in lockstep, stateful. The self-attention KV cache is
  Core ML state (`k_cache`, `v_cache [dec_layers, BEAMS, heads, CACHE_LEN, head_dim]`). Inputs:
  `token [BEAMS]` int32; `pos [1, CACHE_LEN]`, one-hot at this step's position; `attn_mask
  [1, CACHE_LEN]`, 0 up to and including this position and -1e4 after; `parent [BEAMS, BEAMS]`,
  a 0/1 matrix whose row `i` picks the beam whose cache beam `i` continues (the identity when no
  beam forks); plus the encoder outputs and `src_mask`. Output: `logits [BEAMS, tgt_vocab]`.

Every shape is fixed (no dynamic indexing), so the whole graph can run on the Neural Engine.
The modules below re-implement `model.Translit`'s forward pass on its own weights; the tests
check them against it in PyTorch. Weights are int8 (per-channel symmetric linear quantisation),
activations float16.
"""

import argparse
import shutil
import sys
import tempfile
from pathlib import Path

import torch
import torch.nn.functional as F
from torch import Tensor, nn

from mlkb.translit import export
from mlkb.translit.model import Config, Translit

SRC_LEN = 16  # bos + 12 letters + eos fits; production inputs (24 letters) would need 26+
CACHE_LEN = 32  # the model's max_len: bos + up to 31 output tokens
BEAMS = 4
MASKED = -1e4  # additive mask; float16-safe


def spike_model(seed: int = 0) -> Translit:
    """The seeded random full-size model both runtimes are timed on."""
    torch.manual_seed(seed)
    return Translit(Config()).eval()


# Shapes come from the config, never from `tensor.shape`: under `torch.jit.trace` the latter
# become graph ops that coremltools can't fold.


def _heads(x: Tensor, c: Config, b: int, n: int) -> Tensor:
    """`[b, n, d]` → `[b, heads, n, d / heads]`."""
    return x.view(b, n, c.heads, c.d // c.heads).transpose(1, 2)


def _merge(x: Tensor, c: Config, b: int, n: int) -> Tensor:
    """`[b, heads, n, hd]` → `[b, n, d]`."""
    return x.transpose(1, 2).reshape(b, n, c.d)


def _attend(q: Tensor, k: Tensor, v: Tensor, mask: Tensor, c: Config) -> Tensor:
    scores = q @ k.transpose(-1, -2) * ((c.d // c.heads) ** -0.5) + mask
    return torch.softmax(scores, dim=-1) @ v


def _ff(layer: nn.Module, x: Tensor) -> Tensor:
    return layer.linear2(F.relu(layer.linear1(x)))


class Encoder(nn.Module):
    """`Translit.encode` at a fixed source length, plus the decoder's cross-attention K and V."""

    def __init__(self, model: Translit, src_len: int = SRC_LEN):
        super().__init__()
        self.m = model
        self.n = src_len

    def forward(self, src: Tensor, src_mask: Tensor) -> tuple[Tensor, Tensor]:
        m, c, n = self.m, self.m.config, self.n
        x = m.src_emb(src) + m.src_pos.weight[:n]
        mask = src_mask.view(1, 1, 1, n)
        for layer in m.enc:
            a = layer.self_attn
            h = layer.norm1(x)
            q, k, v = F.linear(h, a.in_proj_weight, a.in_proj_bias).chunk(3, dim=-1)
            o = _attend(_heads(q, c, 1, n), _heads(k, c, 1, n), _heads(v, c, 1, n), mask, c)
            x = x + a.out_proj(_merge(o, c, 1, n))
            x = x + _ff(layer, layer.norm2(x))
        memory = m.enc_norm(x)
        ks, vs = [], []
        d = c.d
        for layer in m.dec:
            a = layer.multihead_attn
            w, b = a.in_proj_weight, a.in_proj_bias
            ks.append(_heads(F.linear(memory, w[d : 2 * d], b[d : 2 * d]), c, 1, n))
            vs.append(_heads(F.linear(memory, w[2 * d :], b[2 * d :]), c, 1, n))
        return torch.stack(ks), torch.stack(vs)


class DecoderStep(nn.Module):
    """One decoder step for `beams` beams, with the self-attention KV cache as buffers (Core ML
    state). Beams are reordered by `parent` before this step's keys and values are written."""

    def __init__(self, model: Translit, beams: int = BEAMS, cache_len: int = CACHE_LEN, src_len: int = SRC_LEN):
        super().__init__()
        self.m = model
        self.beams, self.cache_len, self.src_len = beams, cache_len, src_len
        c = model.config
        self.cache_shape = (beams, c.heads, cache_len, c.d // c.heads)
        # One buffer per layer, each overwritten whole: Core ML (macOS 26.6) crashes or hangs
        # loading a model that writes a slice of a state.
        for i in range(c.dec_layers):
            self.register_buffer(f"k_cache_{i}", torch.zeros(self.cache_shape))
            self.register_buffer(f"v_cache_{i}", torch.zeros(self.cache_shape))

    def caches(self, i: int) -> tuple[Tensor, Tensor]:
        """Layer `i`'s key and value caches."""
        return getattr(self, f"k_cache_{i}"), getattr(self, f"v_cache_{i}")

    def forward(
        self,
        token: Tensor,
        pos: Tensor,
        attn_mask: Tensor,
        parent: Tensor,
        cross_k: Tensor,
        cross_v: Tensor,
        src_mask: Tensor,
    ) -> Tensor:
        m, c = self.m, self.m.config
        beams, cache_len, src_len = self.beams, self.cache_len, self.src_len
        x = (m.tgt_emb(token) + pos @ m.tgt_pos.weight[:cache_len]).unsqueeze(1)  # [beams, 1, d]
        here = pos.view(1, 1, cache_len, 1)
        self_mask = attn_mask.view(1, 1, 1, cache_len)
        cross_mask = src_mask.view(1, 1, 1, src_len)
        d = c.d
        for i, layer in enumerate(m.dec):
            a = layer.self_attn
            q, k, v = F.linear(layer.norm1(x), a.in_proj_weight, a.in_proj_bias).chunk(3, dim=-1)
            caches = []
            for cache, new in zip(self.caches(i), (k, v)):
                kept = (parent @ cache.reshape(beams, -1)).view(self.cache_shape)
                updated = kept * (1 - here) + _heads(new, c, beams, 1) * here
                cache[:] = updated
                caches.append(updated)
            o = _attend(_heads(q, c, beams, 1), caches[0], caches[1], self_mask, c)
            x = x + a.out_proj(_merge(o, c, beams, 1))

            a = layer.multihead_attn
            q = F.linear(layer.norm2(x), a.in_proj_weight[:d], a.in_proj_bias[:d])
            o = _attend(_heads(q, c, beams, 1), cross_k[i], cross_v[i], cross_mask, c)
            x = x + a.out_proj(_merge(o, c, beams, 1))
            x = x + _ff(layer, layer.norm3(x))
        return (m.dec_norm(x) @ m.tgt_emb.weight.T)[:, 0]


def masks(length: int, size: int) -> Tensor:
    """`[1, size]`: 0 for the first `length` positions, `MASKED` after."""
    out = torch.full((1, size), MASKED)
    out[0, :length] = 0
    return out


def one_hot(index: int, size: int) -> Tensor:
    out = torch.zeros(1, size)
    out[0, index] = 1
    return out


def convert(model: Translit, out: Path) -> None:
    """Writes the int8, two-function, stateful Core ML package `out` (`.mlpackage`)."""
    import coremltools as ct
    import coremltools.optimize.coreml as cto
    import numpy as np

    c = model.config
    hd = c.d // c.heads
    fp16 = np.float16
    src = torch.ones(1, SRC_LEN, dtype=torch.int32)
    src_mask = masks(6, SRC_LEN)
    cross = torch.zeros(c.dec_layers, 1, c.heads, SRC_LEN, hd)
    quantise = cto.OptimizationConfig(
        global_config=cto.OpLinearQuantizerConfig(mode="linear_symmetric", dtype="int8", granularity="per_channel")
    )
    common = dict(
        minimum_deployment_target=ct.target.iOS18,
        compute_precision=ct.precision.FLOAT16,
        convert_to="mlprogram",
        skip_model_load=True,
    )
    with torch.no_grad(), tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        encoder = torch.jit.trace(Encoder(model).eval(), (src, src_mask))
        enc = ct.convert(
            encoder,
            inputs=[
                ct.TensorType("src", shape=src.shape, dtype=np.int32),
                ct.TensorType("src_mask", shape=src_mask.shape, dtype=fp16),
            ],
            outputs=[ct.TensorType("cross_k", dtype=fp16), ct.TensorType("cross_v", dtype=fp16)],
            **common,
        )
        cto.linear_quantize_weights(enc, quantise).save(tmp / "encoder.mlpackage")

        step = DecoderStep(model).eval()
        example = (
            torch.ones(BEAMS, dtype=torch.int32),
            one_hot(0, CACHE_LEN),
            masks(1, CACHE_LEN),
            torch.eye(BEAMS),
            cross,
            cross,
            src_mask,
        )
        decoder = torch.jit.trace(step, example)
        cache = ct.TensorType(shape=step.cache_shape, dtype=fp16)
        dec = ct.convert(
            decoder,
            inputs=[
                ct.TensorType("token", shape=(BEAMS,), dtype=np.int32),
                ct.TensorType("pos", shape=(1, CACHE_LEN), dtype=fp16),
                ct.TensorType("attn_mask", shape=(1, CACHE_LEN), dtype=fp16),
                ct.TensorType("parent", shape=(BEAMS, BEAMS), dtype=fp16),
                ct.TensorType("cross_k", shape=cross.shape, dtype=fp16),
                ct.TensorType("cross_v", shape=cross.shape, dtype=fp16),
                ct.TensorType("src_mask", shape=src_mask.shape, dtype=fp16),
            ],
            outputs=[ct.TensorType("logits", dtype=fp16)],
            states=[
                ct.StateType(wrapped_type=cache, name=f"{kv}_cache_{i}")
                for i in range(c.dec_layers)
                for kv in "kv"
            ],
            **common,
        )
        cto.linear_quantize_weights(dec, quantise).save(tmp / "decoder.mlpackage")

        functions = ct.utils.MultiFunctionDescriptor()
        functions.add_function(str(tmp / "encoder.mlpackage"), "main", "encoder")
        functions.add_function(str(tmp / "decoder.mlpackage"), "main", "decoder")
        functions.default_function_name = "decoder"
        if out.exists():
            shutil.rmtree(out)
        ct.utils.save_multifunction(functions, str(out))


def check(model: Translit, package: Path, word: list[int] = (1, 10, 16, 22, 3, 2)) -> float:
    """Max abs difference between Core ML (macOS, CPU) and PyTorch logits over 10 lockstep
    steps of greedy decoding; a conversion sanity check."""
    import coremltools as ct
    import numpy as np

    c = model.config
    src = torch.zeros(1, SRC_LEN, dtype=torch.int32)
    src[0, : len(word)] = torch.tensor(word)
    src_mask = masks(len(word), SRC_LEN)
    cpu = ct.ComputeUnit.CPU_ONLY
    enc = ct.models.MLModel(str(package), function_name="encoder", compute_units=cpu)
    dec = ct.models.MLModel(str(package), function_name="decoder", compute_units=cpu)
    out = enc.predict({"src": src.numpy(), "src_mask": src_mask.numpy()})
    state = dec.make_state()
    memory, pad = model.encode(torch.tensor([word]))
    tokens = [1]
    worst = 0.0
    with torch.no_grad():
        for i in range(10):
            got = dec.predict(
                {
                    "token": np.full(BEAMS, tokens[-1], dtype=np.int32),
                    "pos": one_hot(i, CACHE_LEN).numpy(),
                    "attn_mask": masks(i + 1, CACHE_LEN).numpy(),
                    "parent": np.eye(BEAMS, dtype=np.float32),
                    "cross_k": out["cross_k"],
                    "cross_v": out["cross_v"],
                    "src_mask": src_mask.numpy(),
                },
                state,
            )["logits"]
            want = model.decode(torch.tensor([tokens]), memory, pad)[0, -1].numpy()
            worst = max(worst, float(np.abs(got - want[None]).max()))
            tokens.append(int(want[3:].argmax()) + 3)
    return worst


def main(argv: list[str] | None = None) -> None:
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    p.add_argument("--out-dir", type=Path, required=True)
    p.add_argument("--seed", type=int, default=0)
    args = p.parse_args(argv)
    args.out_dir.mkdir(parents=True, exist_ok=True)
    model = spike_model(args.seed)

    mlkbnn = args.out_dir / "translit-spike.mlkbnn"
    export.export_model(model, mlkbnn)
    print(f"wrote {mlkbnn} ({mlkbnn.stat().st_size:,} bytes)", file=sys.stderr)

    import coremltools as ct

    package = args.out_dir / "translit-spike.mlpackage"
    convert(model, package)
    print(f"wrote {package}; Core ML vs PyTorch max |logit diff| {check(model, package):.3f}", file=sys.stderr)
    compiled = Path(ct.utils.compile_model(str(package)))
    target = args.out_dir / "translit-spike.mlmodelc"
    if target.exists():
        shutil.rmtree(target)
    shutil.move(compiled, target)
    print(f"wrote {target}", file=sys.stderr)


if __name__ == "__main__":
    main()
