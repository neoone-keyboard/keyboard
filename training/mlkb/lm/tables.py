"""Per-piece lookup tables shipped inside `lm.mlkbnn` (design §3, §6).

- `lex.first_en`, `lex.first_ml` (`tables_for`): the log share of a lexicon's counts that
  belongs to words whose first token is each piece.
- `tok.p_en` (`p_en`): how English each Latin piece is, against Manglish.
- `lex.fingerprint` (`fingerprint`): which `en.fst` and `ml.fst` the first-token tables were
  built from, so the engine can refuse a model that doesn't match its lexicons.
"""
from collections.abc import Callable, Iterable
from pathlib import Path

import numpy as np

from mlkb.translit.export import fnv1a

# Shard buckets on each side of `p_en`; validation shards (`valid-*`) are never counted.
ENGLISH_SHARDS = ("en-*.bin",)
MANGLISH_SHARDS = ("manglish-*.bin", "mixed-*.bin")


def tables_for(words: Iterable[tuple[str, int]], first_token: Callable[[str], int], vocab: int) -> np.ndarray:
    """`[vocab]` f32: `log(Σ counts of words whose first token is t / Σ all counts)`, `-inf` for
    pieces that start no word."""
    counts = np.zeros(vocab, np.float64)
    for word, count in words:
        counts[first_token(word)] += count
    total = counts.sum()
    with np.errstate(divide="ignore"):
        return np.log(counts / max(total, 1)).astype(np.float32)


def _token_counts(shards: Path, patterns: tuple[str, ...], vocab: int) -> np.ndarray:
    counts = np.zeros(vocab, np.int64)
    for pattern in patterns:
        for path in sorted(shards.glob(pattern)):
            ids = np.memmap(path, dtype="<u2", mode="r")
            for start in range(0, len(ids), 1 << 26):
                counts += np.bincount(ids[start : start + (1 << 26)], minlength=vocab)[:vocab]
    return counts


def p_en(shards: Path, vocab: int, latin: np.ndarray) -> np.ndarray:
    """`[vocab]` f32: for each Latin piece (`latin[t]`), `f_en / (f_en + f_mg)`, where `f_en` is
    its count in the English shards over their Latin-token total and `f_mg` the same in the
    Manglish and mixed shards. 0.5 for every other piece and for Latin pieces seen in neither."""
    en = _token_counts(Path(shards), ENGLISH_SHARDS, vocab)
    mg = _token_counts(Path(shards), MANGLISH_SHARDS, vocab)
    return p_en_from_counts(en, mg, latin)


def p_en_from_counts(en: np.ndarray, mg: np.ndarray, latin: np.ndarray) -> np.ndarray:
    """`p_en` from token counts by id: `en` in English text, `mg` in Manglish and mixed text."""
    en, mg, vocab = np.asarray(en, np.float64), np.asarray(mg, np.float64), len(latin)
    f_en = np.where(latin, en, 0) / max(en[latin].sum(), 1)
    f_mg = np.where(latin, mg, 0) / max(mg[latin].sum(), 1)
    both = f_en + f_mg
    out = np.full(vocab, 0.5, np.float64)
    seen = latin & (both > 0)
    out[seen] = f_en[seen] / both[seen]
    return out.astype(np.float32)


def latin_pieces(pieces: list[str]) -> np.ndarray:
    """Pieces the engine flags `LATIN` (`engine/core/src/nn/tokenizer.rs::flags_of`): not a
    control or byte piece, no Malayalam character, at least one ASCII letter."""
    out = np.zeros(len(pieces), bool)
    for i, p in enumerate(pieces):
        if i <= 3 or (p.startswith("<0x") and p.endswith(">") and len(p) == 6):
            continue
        if any("ഀ" <= c <= "ൿ" for c in p):
            continue
        out[i] = any(c.isascii() and c.isalpha() for c in p)
    return out


def fingerprint(en_fst: Path, ml_fst: Path) -> bytes:
    """16 bytes: the little-endian 64-bit FNV-1a of `en_fst`, then of `ml_fst`."""
    return b"".join(fnv1a(Path(p).read_bytes()).to_bytes(8, "little") for p in (en_fst, ml_fst))
