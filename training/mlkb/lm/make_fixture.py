"""Writes the tiny LM fixture the Rust tests run against.

    uv run --group train python -m mlkb.lm.make_fixture

Outputs `engine/core/tests/fixtures/lm-tiny.mlkbnn` and `lm-tiny-reference.json`:
- a seeded `LM(Config(vocab=300, d=64, heads=2, layers=2, ff=128))`, untrained, with every
  parameter (biases and norms included) moved away from PyTorch's initial values, as
  `mlkb.translit.make_fixture` does, so a forward pass that skips a bias or a norm fails;
- the first 300 pieces of a SentencePiece vocabulary trained, with the options of
  `mlkb.lm.tokenizer`, on the committed eval sentences (`english-sentences.txt`, `next-ml.txt`);
- tables from the same text: `lex.first_en`/`lex.first_ml` from its English and Malayalam
  words, `tok.p_en` from English against the Manglish column of `next-manglish.tsv`; the
  fingerprint of the bundled lexicons;
- the reference (`mlkb.lm.export.reference`) on the dequantised weights.
Deterministic for a given `sentencepiece` and `torch` version.
"""
import json
import sys
import tempfile
from collections import Counter
from pathlib import Path

import mlengine
import numpy as np
import sentencepiece as spm
import torch

from mlkb.lm.export import LEXICONS, dequantised_model, export, reference, reference_tokens, tables_for
from mlkb.lm.model import LM, Config
from mlkb.lm.tables import fingerprint, latin_pieces, p_en_from_counts

ROOT = Path(__file__).resolve().parents[3]
EVAL = ROOT / "data/eval"
OUT = ROOT / "engine/core/tests/fixtures"
CONFIG = Config(vocab=300, d=64, heads=2, layers=2, ff=128)
SEED = 20261103
SPM_VOCAB = 1000  # trained larger, then cut to the first `CONFIG.vocab` pieces


def _lines(path: Path, column: int | None = None) -> list[str]:
    out = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        text = line.split("\t")[column] if column is not None else line
        out.append(mlengine.lm_normalize(text))
    return out


def vocabulary(english: list[str], malayalam: list[str]) -> tuple[list[str], list[float]]:
    """The first `CONFIG.vocab` pieces and scores of a SentencePiece model over the text."""
    with tempfile.TemporaryDirectory() as tmp:
        text = Path(tmp) / "text.txt"
        text.write_text("\n".join(english + malayalam) + "\n", encoding="utf-8")
        spm.SentencePieceTrainer.train(
            input=str(text), model_prefix=str(Path(tmp) / "spm"), model_type="unigram",
            vocab_size=SPM_VOCAB, hard_vocab_limit=False, pad_id=0, unk_id=1, bos_id=2, eos_id=3,
            byte_fallback=True, split_digits=True, normalization_rule_name="identity",
            character_coverage=0.9999, num_threads=1, seed_sentencepiece_size=100_000,
            minloglevel=2,
        )
        sp = spm.SentencePieceProcessor(model_file=str(Path(tmp) / "spm.model"))
    n = CONFIG.vocab
    if sp.get_piece_size() < n:
        raise SystemExit(f"sentencepiece made only {sp.get_piece_size()} pieces")
    return [sp.id_to_piece(i) for i in range(n)], [sp.get_score(i) for i in range(n)]


@torch.no_grad()
def tiny_model() -> LM:
    torch.manual_seed(SEED)
    model = LM(CONFIG).eval()
    for name, p in model.named_parameters():
        if p.ndim == 2:
            p.normal_(0, 1.5 / p.shape[1] ** 0.5)
        elif "norm" in name and name.endswith("weight"):
            p.uniform_(0.5, 1.5)
        else:
            p.normal_(0, 0.2)
    return model


def _counts(tokenizer, lines: list[str]) -> np.ndarray:
    ids = [t for line in lines for t in tokenizer.encode(line)]
    return np.bincount(np.array(ids, np.int64), minlength=CONFIG.vocab)


def main() -> int:
    english = _lines(EVAL / "english-sentences.txt")
    malayalam = _lines(EVAL / "next-ml.txt")
    manglish = _lines(EVAL / "next-manglish.tsv", column=0)
    pieces, scores = vocabulary(english, malayalam)
    tokenizer = mlengine.Tokenizer(pieces, scores)
    first = lambda w: tokenizer.encode(w)[0]  # noqa: E731
    en_words = Counter(w for line in english for w in line.split())
    ml_words = Counter(w for line in malayalam for w in line.split())
    tables = {
        "lex.first_en": tables_for(sorted(en_words.items()), first, CONFIG.vocab),
        "lex.first_ml": tables_for(sorted(ml_words.items()), first, CONFIG.vocab),
        "tok.p_en": p_en_from_counts(_counts(tokenizer, english), _counts(tokenizer, manglish),
                                     latin_pieces(pieces)),
    }
    model = tiny_model()
    OUT.mkdir(parents=True, exist_ok=True)
    path = OUT / "lm-tiny.mlkbnn"
    export(model, pieces, scores, tables, fingerprint(LEXICONS / "en.fst", LEXICONS / "ml.fst"), path)
    cases, long = reference_tokens(tokenizer)
    ref_path = OUT / "lm-tiny-reference.json"
    ref_path.write_text(json.dumps(reference(dequantised_model(path)[0], cases, long),
                                   separators=(",", ":")) + "\n")
    size = path.stat().st_size + ref_path.stat().st_size
    print(f"wrote {path} and {ref_path} ({size:,} bytes together)", file=sys.stderr)
    return 0 if size < 300_000 else 1


if __name__ == "__main__":
    sys.exit(main())
