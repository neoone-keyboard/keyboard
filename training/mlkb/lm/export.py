"""Writes the LM, its tokeniser and its lookup tables as one `.mlkbnn` file (design §7).

    uv run --group train python -m mlkb.lm.export --checkpoint build/lm.pt \\
        --vocab build/lm/pieces.tsv --shards build/lm/shards --out build/lm.mlkbnn \\
        [--reference build/lm-reference.json]

The format is `mlkb.translit.export`'s with `kind = KIND_LM`; `engine/core/src/nn/file.rs`
documents it. The header reuses the transliterator's fields: `d`, `heads`, `enc_layers = 0`,
`dec_layers` = layers, `ff`, `max_len` = training sequence length, `src_vocab = 0`,
`tgt_vocab` = vocabulary size. Tensors, in order: the `state_dict` (matrices int8, the rest
f32), `rope.theta` (f32 [1]), `tok.pieces` (UTF-8 pieces joined by NUL, bytes), `tok.scores`,
`tok.p_en`, `lex.first_en`, `lex.first_ml` (f32 [vocab] each), `lex.fingerprint` (bytes [16]).

The first-token tables tokenise every word of the bundled `en.fst` and `ml.fst` with the
production tokeniser (`mlengine.Tokenizer`).

`--reference` writes next-token log-probabilities from PyTorch on the dequantised weights, for
the Rust forward pass to be tested against (`reference`).
"""
import argparse
import json
import sys
from pathlib import Path

import mlengine
import numpy as np
import torch
from torch import Tensor
from torch.nn import functional as F

from mlkb.lm.model import LM, Config, rope
from mlkb.lm.shards import read_pieces
from mlkb.lm.tables import fingerprint as lexicon_fingerprint
from mlkb.lm.tables import latin_pieces, p_en, tables_for
from mlkb.translit.export import FIELDS, KIND_LM, read, write

__all__ = ["dequantised_model", "export", "forward_masked", "reference", "tables_for"]

ROOT = Path(__file__).resolve().parents[3]
LEXICONS = ROOT / "apps/ios/KeyboardExtension/Lexicon"
BOS = 2
TABLES = ("tok.p_en", "lex.first_en", "lex.first_ml")
# Texts for `--reference` on the real model; the fixture uses the same ones.
REFERENCE_TEXTS = (
    "are you free this evening",
    "ഞാൻ നാളെ വീട്ടിൽ വരും",
    "njan nale veettil varum, ok",
)
LONG_TEXT = ("the old house at the end of the road was empty for years, and then one summer a family "
             "moved in. ഞങ്ങൾ അവിടെ പോയി, avar nalla aalukal aanu ennu thonni. we stayed for dinner.")
LONG_TOKENS = 40
EVICT = (20, 8, 4)  # feed 20 tokens, drop the oldest 8 rows, feed 4 more


def header(c: Config) -> dict:
    return {"d": c.d, "heads": c.heads, "enc_layers": 0, "dec_layers": c.layers, "ff": c.ff,
            "max_len": c.max_len, "src_vocab": 0, "tgt_vocab": c.vocab}


def export(model: LM, pieces: list[str], scores: list[float], tables: dict[str, np.ndarray],
           fingerprint: bytes, path: Path) -> None:
    """Writes `model`, its tokeniser (`pieces`, `scores` in id order), `tables` (the `TABLES`
    keys, `[vocab]` each) and the lexicon `fingerprint` (16 bytes) to `path`."""
    c = model.config
    if len(pieces) != c.vocab or len(scores) != c.vocab:
        raise ValueError(f"{len(pieces)} pieces and {len(scores)} scores for a {c.vocab}-token model")
    if len(fingerprint) != 16:
        raise ValueError("the lexicon fingerprint is 16 bytes")
    if any("\0" in p for p in pieces):
        raise ValueError("a piece contains NUL")
    tensors: dict[str, np.ndarray | bytes] = {
        name: t.detach().cpu().float().numpy() for name, t in model.state_dict().items()
    }
    tensors["rope.theta"] = np.array([c.theta], np.float32)
    tensors["tok.pieces"] = "\0".join(pieces).encode("utf-8")
    tensors["tok.scores"] = np.asarray(scores, np.float32)
    for name in TABLES:
        table = np.asarray(tables[name], np.float32)
        if table.shape != (c.vocab,):
            raise ValueError(f"{name}: shape {table.shape}, want ({c.vocab},)")
        tensors[name] = table
    tensors["lex.fingerprint"] = bytes(fingerprint)
    write(path, header(c), tensors, kind=KIND_LM)


def dequantised_model(path: Path) -> tuple[LM, dict]:
    """The file's model in eval mode, with the weights exactly as the engine sees them, and the
    file's other tensors (tokeniser, tables) by name."""
    config, tensors = read(path)
    if config["kind"] != KIND_LM:
        raise ValueError(f"{path}: kind {config['kind']}, not a language model")
    theta = float(tensors["rope.theta"][0])
    c = Config(vocab=config["tgt_vocab"], d=config["d"], heads=config["heads"], layers=config["dec_layers"],
               ff=config["ff"], max_len=config["max_len"], theta=theta)
    model = LM(c)
    keys = set(model.state_dict())
    model.load_state_dict({k: torch.from_numpy(v) for k, v in tensors.items() if k in keys})
    return model.eval(), {k: v for k, v in tensors.items() if k not in keys}


def forward_masked(model: LM, tokens: Tensor, allowed: Tensor) -> Tensor:
    """Logits `[T, vocab]` for `tokens [T]` at positions 0..T-1, where row i attends to row j
    only if `allowed[i, j]` (in every layer). With a lower-triangular `allowed` this is
    `model(tokens[None])[0]` in eval mode."""
    c = model.config
    t = tokens.shape[0]
    h, dh = c.heads, c.d // c.heads
    pos = torch.arange(t)
    x = model.emb(tokens)[None]
    for block in model.layers:
        q, k, v = block.qkv(block.norm1(x)).view(1, t, 3, h, dh).unbind(2)
        q, k = rope(q, pos, c.theta), rope(k, pos, c.theta)
        a = F.scaled_dot_product_attention(q.transpose(1, 2), k.transpose(1, 2), v.transpose(1, 2),
                                           attn_mask=allowed)
        x = x + block.out(a.transpose(1, 2).reshape(1, t, c.d))
        x = x + block.ff2(F.relu(block.ff1(block.norm2(x))))
    return (model.norm(x) @ model.emb.weight.T)[0]


def _log_probs(logits: Tensor) -> list[float]:
    return np.round(torch.log_softmax(logits.double(), -1).numpy(), 5).tolist()


def reference_tokens(tokenizer) -> tuple[list[list[int]], list[int]]:
    """The reference cases (`BOS` + tokens of each text, the long one cut to `LONG_TOKENS`)."""
    cases = [[BOS, *tokenizer.encode(text)] for text in REFERENCE_TEXTS]
    long = [BOS, *tokenizer.encode(LONG_TEXT)][:LONG_TOKENS]
    if len(long) < LONG_TOKENS:
        raise ValueError(f"the long reference text is only {len(long)} tokens")
    return [*cases, long], long


@torch.no_grad()
def reference(model: LM, cases: list[list[int]], long: list[int]) -> dict:
    """`{"config", "cases": [{"tokens", "log_probs"}], "eviction": {"tokens", "log_probs",
    "feed", "evict"}}`. `log_probs` is the log-softmax after the last token. For the eviction
    case, the first `EVICT[0]` tokens are fed, the oldest `EVICT[1]` rows dropped and `EVICT[2]`
    more fed: rows fed later attend only to the kept rows, which keep their positions."""
    model.eval()
    out = []
    for tokens in cases:
        x = torch.tensor(tokens)
        logits = forward_masked(model, x, torch.ones(len(tokens), len(tokens), dtype=torch.bool).tril())
        out.append({"tokens": tokens, "log_probs": _log_probs(logits[-1])})
    fed, dropped, more = EVICT
    tokens = long[: fed + more]
    n = len(tokens)
    allowed = torch.ones(n, n, dtype=torch.bool).tril()
    allowed[fed:, :dropped] = False
    logits = forward_masked(model, torch.tensor(tokens), allowed)
    c = model.config
    return {
        "config": {"vocab": c.vocab, "d": c.d, "heads": c.heads, "layers": c.layers, "ff": c.ff, "theta": c.theta},
        "cases": out,
        "eviction": {"tokens": tokens, "feed": fed, "evict": dropped, "log_probs": _log_probs(logits[-1])},
    }


def load_checkpoint(path: Path) -> LM:
    """A checkpoint written by `mlkb.lm.train` (`{"model", "config", ...}`), in eval mode."""
    state = torch.load(path, map_location="cpu", weights_only=False)
    model = LM(Config(**state["config"]))
    model.load_state_dict(state["model"])
    return model.eval()


def lexicon_tables(tokenizer, en_fst: Path, ml_fst: Path, vocab: int) -> dict[str, np.ndarray]:
    """`lex.first_en` and `lex.first_ml` from every word of the two lexicons."""
    out = {}
    for name, path in (("lex.first_en", en_fst), ("lex.first_ml", ml_fst)):
        words = [(w, c) for w, c in mlengine.malayalam_words(str(path), 10**9) if tokenizer.encode(w)]
        out[name] = tables_for(words, lambda w: tokenizer.encode(w)[0], vocab)
        print(f"{name}: {len(words):,} words from {path.name}, "
              f"{int(np.isfinite(out[name]).sum()):,} first tokens", file=sys.stderr)
    return out


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    p.add_argument("--checkpoint", type=Path, default=Path("build/lm.pt"))
    p.add_argument("--vocab", type=Path, default=Path("build/lm/pieces.tsv"))
    p.add_argument("--shards", type=Path, default=Path("build/lm/shards"))
    p.add_argument("--lexicons", type=Path, default=LEXICONS, help="folder with en.fst and ml.fst")
    p.add_argument("--out", type=Path, default=Path("build/lm.mlkbnn"))
    p.add_argument("--reference", type=Path, help="also write PyTorch reference outputs (JSON)")
    args = p.parse_args(argv)
    model = load_checkpoint(args.checkpoint)
    pieces, scores = read_pieces(args.vocab)
    tokenizer = mlengine.Tokenizer(pieces, scores)
    en_fst, ml_fst = args.lexicons / "en.fst", args.lexicons / "ml.fst"
    tables = lexicon_tables(tokenizer, en_fst, ml_fst, len(pieces))
    tables["tok.p_en"] = p_en(args.shards, len(pieces), latin_pieces(pieces))
    args.out.parent.mkdir(parents=True, exist_ok=True)
    export(model, pieces, scores, tables, lexicon_fingerprint(en_fst, ml_fst), args.out)
    print(f"wrote {args.out} ({args.out.stat().st_size:,} bytes)", file=sys.stderr)
    if args.reference:
        cases, long = reference_tokens(tokenizer)
        args.reference.write_text(json.dumps(reference(dequantised_model(args.out)[0], cases, long),
                                             separators=(",", ":")) + "\n")
        print(f"wrote {args.reference}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
