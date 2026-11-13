"""Decoding without the lexicon: beam search for one word, greedy for a batch.

The lexicon-constrained search the keyboard uses is in Rust (`engine/core`); these are for
training-time validation and for measuring the model alone.
"""

import torch

from mlkb.translit.model import Translit
from mlkb.translit.vocab import MALAYALAM, Vocab, decode_ml, encode_latin


def _normalised(logprob: float, length: int) -> float:
    return logprob / max(length, 1)


@torch.no_grad()
def beam(
    model: Translit, latin: str, width: int, max_len: int = 32, vocab: Vocab = MALAYALAM
) -> list[tuple[str, float]]:
    """Up to `width` distinct native spellings (in `vocab`; Malayalam by default) of `latin` with their total log probability,
    best first by log probability per generated token (eos included). Runs on the model's
    device; one word at a time is meant for the CPU."""
    was_training = model.training
    model.eval()
    device = next(model.parameters()).device
    src = torch.tensor([encode_latin(latin)], device=device)
    memory, src_pad = model.encode(src)
    alive: list[tuple[list[int], float]] = [([MALAYALAM.bos], 0.0)]
    done: list[tuple[list[int], float]] = []
    steps = min(max_len, model.config.max_len) - 1
    for _ in range(steps):
        if not alive or len(done) >= width:
            break
        tgt = torch.tensor([tokens for tokens, _ in alive], device=device)
        k = len(alive)
        logits = model.decode(tgt, memory.expand(k, -1, -1), src_pad.expand(k, -1))[:, -1]
        logp = torch.log_softmax(logits.float(), dim=-1)
        top = torch.topk(logp, width, dim=-1)
        cands = []
        for b, (tokens, lp) in enumerate(alive):
            for v, t in zip(top.values[b].tolist(), top.indices[b].tolist(), strict=True):
                if t in (MALAYALAM.pad, MALAYALAM.bos):
                    continue
                cands.append((tokens + [t], lp + v))
        cands.sort(key=lambda c: _normalised(c[1], len(c[0]) - 1), reverse=True)
        alive = []
        for tokens, lp in cands:
            if tokens[-1] == MALAYALAM.eos:
                done.append((tokens, lp))
            else:
                alive.append((tokens, lp))
            if len(alive) + len(done) >= width:
                break
        alive = alive[: width - min(len(done), width)]
    done += alive  # beams cut off at max_len count as finished
    done.sort(key=lambda c: _normalised(c[1], len(c[0]) - 1), reverse=True)
    out: list[tuple[str, float]] = []
    seen = set()
    for tokens, lp in done:
        text = decode_ml(tokens, vocab)
        if text not in seen:
            seen.add(text)
            out.append((text, lp))
    model.train(was_training)
    return out[:width]


@torch.no_grad()
def greedy_tokens(model: Translit, src: torch.Tensor, max_len: int | None = None) -> torch.Tensor:
    """Greedy decoding of a padded `src` batch on its device: `[batch, <= max_len]` token ids."""
    max_len = max_len or model.config.max_len
    memory, src_pad = model.encode(src)
    out = torch.full((src.shape[0], 1), MALAYALAM.bos, dtype=torch.long, device=src.device)
    finished = torch.zeros(src.shape[0], dtype=torch.bool, device=src.device)
    for _ in range(max_len - 1):
        nxt = model.decode(out, memory, src_pad)[:, -1].argmax(-1)
        nxt = torch.where(finished, torch.full_like(nxt, MALAYALAM.pad), nxt)
        out = torch.cat([out, nxt[:, None]], dim=1)
        finished |= nxt == MALAYALAM.eos
        if bool(finished.all()):
            break
    return out


def greedy(model: Translit, words: list[str], vocab: Vocab = MALAYALAM) -> list[str]:
    """The greedy Malayalam spelling of each Latin word, on the model's device."""
    was_training = model.training
    model.eval()
    device = next(model.parameters()).device
    encoded = [encode_latin(w) for w in words]
    width = max(len(e) for e in encoded)
    src = torch.tensor([e + [0] * (width - len(e)) for e in encoded], device=device)
    tokens = greedy_tokens(model, src).cpu().tolist()
    model.train(was_training)
    return [decode_ml(t, vocab) for t in tokens]
