"""Writes the transliterator in the keyboard's int8 model format (`.mlkbnn`).

    uv run --group train python -m mlkb.translit.export \\
        --checkpoint training/build/translit.pt --out training/build/translit.mlkbnn \\
        [--reference training/build/translit-reference.json]

The format is documented at the top of `engine/core/src/nn/file.rs`, which reads it; keep the
two in sync. In short: a header with the model shape, then every `state_dict` tensor under its
PyTorch name (matrices as int8 with one f32 scale per row, raw bytes as dtype 2, everything else
f32), each data block aligned to 16 bytes, then an FNV-1a checksum. The language model
(`mlkb.lm.export`) writes the same format with `kind=KIND_LM`.

`--reference` also writes, for `REFERENCE_CASES`, the encoder output and the decoder logits at
every step, from PyTorch in eval mode on the float weights and on the dequantised int8 weights.
The Rust forward pass is tested against such a file.
"""

import argparse
import json
import struct
import sys
from pathlib import Path

import numpy as np
import torch

from mlkb.translit.model import Config, Translit
from mlkb.translit.vocab import encode_latin, encode_ml

MAGIC = b"MLKBNN01"
VERSION = 1
KIND_TRANSLIT = 1  # encoder–decoder transliterator (`model.Translit`)
KIND_LM = 2  # decoder-only language model (`mlkb.lm.model.LM`)
FIELDS = ("d", "heads", "enc_layers", "dec_layers", "ff", "max_len", "src_vocab", "tgt_vocab")
F32, I8, BYTES = 0, 1, 2
ALIGN = 16
_FNV_OFFSET, _FNV_PRIME, _MASK = 0xCBF29CE484222325, 0x100000001B3, (1 << 64) - 1

# (Latin, Malayalam) pairs; the decoder is fed bos + the Malayalam tokens, minus eos.
REFERENCE_CASES = (
    ("ente", "എന്റെ"),
    ("nanni", "നന്ദി"),
    ("enthu", "എന്തു"),
    ("kollamo", "കൊള്ളാമോ"),
    ("jishnu", "ജിഷ്ണു"),
)


def fnv1a(data: bytes) -> int:
    """64-bit FNV-1a of `data`."""
    h = _FNV_OFFSET
    for b in data:
        h = ((h ^ b) * _FNV_PRIME) & _MASK
    return h


def quantise(w: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Symmetric per-row int8: `(scales [rows] f32, values [rows, cols] i8)` with
    `w ≈ values * scales[:, None]`. A zero row gets scale 0."""
    w = np.asarray(w, dtype=np.float32)
    scales = (np.abs(w).max(axis=1) / 127).astype(np.float32)
    safe = np.where(scales > 0, scales, 1)[:, None]
    values = np.clip(np.rint(w / safe), -127, 127).astype(np.int8)
    return scales, values


def write(path: Path, config: dict, tensors: dict[str, np.ndarray | bytes], kind: int = KIND_TRANSLIT) -> None:
    """Writes `tensors` in order: `bytes` values as raw bytes (1-D), 2-D arrays as int8, the
    rest as f32. `config` needs `FIELDS`."""
    out = bytearray(MAGIC)
    out += struct.pack("<2I", VERSION, kind)
    out += struct.pack(f"<{len(FIELDS)}I", *(int(config[k]) for k in FIELDS))
    out += struct.pack("<I", len(tensors))
    for name, value in tensors.items():
        encoded = name.encode()
        out += struct.pack("<H", len(encoded)) + encoded
        if isinstance(value, bytes | bytearray):
            out += struct.pack("<BBI", BYTES, 1, len(value))
            out += bytes(-len(out) % ALIGN)
            out += value
            continue
        value = np.asarray(value, dtype=np.float32)
        dtype = I8 if value.ndim == 2 else F32
        out += struct.pack("<BB", dtype, value.ndim) + struct.pack(f"<{value.ndim}I", *value.shape)
        out += bytes(-len(out) % ALIGN)
        if dtype == I8:
            scales, values = quantise(value)
            out += scales.astype("<f4").tobytes() + values.tobytes()
        else:
            out += value.astype("<f4").tobytes()
    out += struct.pack("<Q", fnv1a(bytes(out)))
    Path(path).write_bytes(bytes(out))


def export_model(model: Translit, path: Path) -> None:
    """Writes `model`'s weights (its `state_dict`, in order) and shape to `path`."""
    tensors = {name: t.detach().cpu().float().numpy() for name, t in model.state_dict().items()}
    write(path, model.config.to_dict(), tensors)


def layout(data: bytes) -> list[tuple[str, int, tuple[int, ...], int]]:
    """`(name, dtype, dims, data offset)` of each tensor; ValueError if the file is malformed."""
    if data[:8] != MAGIC:
        raise ValueError("not an .mlkbnn file (bad magic)")
    pos = 8 + 4 * (2 + len(FIELDS))
    (count,) = struct.unpack_from("<I", data, pos)
    pos += 4
    out = []
    try:
        for _ in range(count):
            (n,) = struct.unpack_from("<H", data, pos)
            name = data[pos + 2 : pos + 2 + n].decode()
            pos += 2 + n
            dtype, ndim = struct.unpack_from("<BB", data, pos)
            dims = struct.unpack_from(f"<{ndim}I", data, pos + 2)
            pos += 2 + 4 * ndim
            pos += -pos % ALIGN
            out.append((name, dtype, dims, pos))
            size = int(np.prod(dims))
            pos += {F32: 4 * size, BYTES: size}.get(dtype, 4 * dims[0] + size)
    except struct.error as e:
        raise ValueError(f"truncated .mlkbnn file: {e}") from e
    if pos + 8 != len(data):
        raise ValueError(f"truncated or padded .mlkbnn file: tensors end at {pos}, file is {len(data)} bytes")
    return out


def read(path: Path) -> tuple[dict, dict[str, np.ndarray | bytes]]:
    """`(config, tensors)` from an `.mlkbnn` file, int8 matrices dequantised to f32, raw bytes
    as `bytes`.
    ValueError on a bad magic, version, checksum or layout."""
    data = Path(path).read_bytes()
    entries = layout(data)
    (stored,) = struct.unpack_from("<Q", data, len(data) - 8)
    if stored != fnv1a(data[:-8]):
        raise ValueError("bad .mlkbnn checksum")
    version, kind = struct.unpack_from("<2I", data, 8)
    if version != VERSION:
        raise ValueError(f"unsupported .mlkbnn version {version}")
    config = {"kind": kind, **dict(zip(FIELDS, struct.unpack_from(f"<{len(FIELDS)}I", data, 16), strict=True))}
    tensors = {}
    for name, dtype, dims, offset in entries:
        if dtype == F32:
            tensors[name] = np.frombuffer(data, "<f4", int(np.prod(dims)), offset).reshape(dims).copy()
        elif dtype == I8:
            rows, cols = dims
            scales = np.frombuffer(data, "<f4", rows, offset)
            values = np.frombuffer(data, np.int8, rows * cols, offset + 4 * rows).reshape(rows, cols)
            tensors[name] = (values.astype(np.float32) * scales[:, None]).astype(np.float32)
        elif dtype == BYTES:
            tensors[name] = bytes(data[offset : offset + dims[0]])
        else:
            raise ValueError(f"unknown dtype {dtype} for {name}")
    return config, tensors


def dequantised_model(path: Path) -> Translit:
    """A `Translit` in eval mode holding the file's weights exactly as the engine sees them."""
    config, tensors = read(path)
    model = Translit(Config(**{k: config[k] for k in FIELDS}))
    model.load_state_dict({k: torch.from_numpy(v) for k, v in tensors.items()})
    return model.eval()


def _rounded(t: torch.Tensor) -> list:
    return np.round(t.detach().cpu().double().numpy(), 5).tolist()


@torch.no_grad()
def _outputs(model: Translit, src: list[int], tgt: list[int]) -> dict:
    model.eval()
    memory, pad = model.encode(torch.tensor([src]))
    logits = model.decode(torch.tensor([tgt]), memory, pad)  # causal: row i = step i
    return {"encoder": _rounded(memory[0]), "logits": _rounded(logits[0])}


def reference(model: Translit, dequantised: Translit, cases=REFERENCE_CASES) -> dict:
    """Reference outputs for each case: token ids fed in, then the encoder output `[src, d]`
    and the decoder logits after each fed token `[tgt, tgt_vocab]`, from `model` ("float")
    and from `dequantised`. Values are rounded to 5 decimals."""
    out = []
    for latin, malayalam in cases:
        src, tgt = encode_latin(latin), encode_ml(malayalam)[:-1]
        out.append(
            {
                "latin": latin,
                "malayalam": malayalam,
                "src": src,
                "tgt": tgt,
                "float": _outputs(model, src, tgt),
                "dequantised": _outputs(dequantised, src, tgt),
            }
        )
    return {"config": {k: getattr(model.config, k) for k in FIELDS}, "cases": out}


def write_reference(model: Translit, model_path: Path, out: Path, cases=REFERENCE_CASES) -> None:
    """Writes `reference` for `model` and the dequantised weights in `model_path` to `out`."""
    ref = reference(model, dequantised_model(model_path), cases)
    Path(out).write_text(json.dumps(ref, separators=(",", ":")) + "\n")


def load_checkpoint(path: Path) -> Translit:
    """A checkpoint written by `mlkb.translit.train` (`{"model", "config", ...}`), in eval mode."""
    state = torch.load(path, map_location="cpu", weights_only=False)
    model = Translit(Config(**state["config"]))
    model.load_state_dict(state["model"])
    return model.eval()


def main(argv: list[str] | None = None) -> None:
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    p.add_argument("--checkpoint", type=Path, required=True)
    p.add_argument("--out", type=Path, required=True)
    p.add_argument("--reference", type=Path, help="also write PyTorch reference outputs (JSON)")
    args = p.parse_args(argv)
    model = load_checkpoint(args.checkpoint)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    export_model(model, args.out)
    print(f"wrote {args.out} ({args.out.stat().st_size:,} bytes)", file=sys.stderr)
    if args.reference:
        write_reference(model, args.out, args.reference)
        print(f"wrote {args.reference}", file=sys.stderr)


if __name__ == "__main__":
    main()
