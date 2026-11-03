"""Writes the tiny seeded model the Rust forward-pass tests run against.

    uv run --group train python -m mlkb.translit.make_fixture

Output: `engine/core/tests/fixtures/translit-tiny.mlkbnn` and `translit-tiny-reference.json`
(see `export.reference`). The weights are random but seeded, and every parameter, biases and
layer norms included, is perturbed away from PyTorch's initial values, so a forward pass that
skips a bias or a norm fails the test.
"""

import argparse
from pathlib import Path

import torch

from mlkb.translit import export
from mlkb.translit.model import Config, Translit

TINY = Config(d=64, heads=4, enc_layers=2, dec_layers=2, ff=64, max_len=16)
FIXTURES = Path(__file__).resolve().parents[3] / "engine/core/tests/fixtures"
CASES = export.REFERENCE_CASES[:4]


@torch.no_grad()
def tiny_model(seed: int = 0) -> Translit:
    torch.manual_seed(seed)
    model = Translit(TINY).eval()
    for name, p in model.named_parameters():
        if p.ndim == 2:
            p.normal_(0, 1.5 / p.shape[1] ** 0.5)
        elif "norm" in name and name.endswith("weight"):
            p.uniform_(0.5, 1.5)
        else:
            p.normal_(0, 0.2)
    return model


def main(argv: list[str] | None = None) -> None:
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    p.add_argument("--out-dir", type=Path, default=FIXTURES)
    args = p.parse_args(argv)
    args.out_dir.mkdir(parents=True, exist_ok=True)
    model_path = args.out_dir / "translit-tiny.mlkbnn"
    model = tiny_model()
    export.export_model(model, model_path)
    export.write_reference(model, model_path, args.out_dir / "translit-tiny-reference.json", CASES)
    for f in sorted(args.out_dir.glob("translit-tiny*")):
        print(f"{f}: {f.stat().st_size:,} bytes")


if __name__ == "__main__":
    main()
