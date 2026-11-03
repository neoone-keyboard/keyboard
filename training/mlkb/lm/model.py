"""The bilingual LM: a small pre-norm decoder-only transformer with rotary positions.

Layout (`state_dict` keys, exported one by one by `export.py` and re-implemented in
`engine/core/src/nn/lm.rs`):
- `emb.weight [vocab, d]`: token embedding, tied with the output (logits = norm(x) @ emb.weight.T).
- `layers.{i}.norm1.*`, `layers.{i}.qkv.{weight,bias}` (q, k, v packed as rows [0,d), [d,2d),
  [2d,3d)), `layers.{i}.out.*`, `layers.{i}.norm2.*`, `layers.{i}.ff1.*`, `layers.{i}.ff2.*`.
- `norm.*`: the final layer norm.
Rotary positions ("rotate half") on q and k of each head: for i < dh/2, the pair
(x[i], x[i + dh/2]) is rotated by pos * theta^(-2i/dh). No position embedding.
"""
import math
from dataclasses import asdict, dataclass

import torch
from torch import Tensor, nn
from torch.nn import functional as F


@dataclass(frozen=True)
class Config:
    """The model's shape; the defaults are the shipped model (design §4)."""

    vocab: int = 16384
    d: int = 256
    heads: int = 4
    layers: int = 6
    ff: int = 1024
    max_len: int = 48
    theta: float = 10000.0
    dropout: float = 0.1

    def to_dict(self) -> dict:
        """The fields as a plain dict, for checkpoints."""
        return asdict(self)


def rope(x: Tensor, pos: Tensor, theta: float) -> Tensor:
    """`x [..., T, heads, dh]` (or `[T, heads, dh]`) rotated by positions `pos [T]`."""
    dh = x.shape[-1]
    half = dh // 2
    inv = theta ** (-torch.arange(half, dtype=torch.float32, device=x.device) * 2 / dh)
    ang = pos.to(device=x.device, dtype=torch.float32)[:, None] * inv  # [T, half]
    cos, sin = ang.cos()[:, None, :], ang.sin()[:, None, :]
    a, b = x[..., :half], x[..., half:]
    return torch.cat([a * cos - b * sin, a * sin + b * cos], dim=-1)


class Block(nn.Module):
    """One pre-norm layer: causal self-attention with rotary q and k, then a ReLU feed-forward."""

    def __init__(self, c: Config):
        super().__init__()
        self.c = c
        self.norm1 = nn.LayerNorm(c.d)
        self.qkv = nn.Linear(c.d, 3 * c.d)
        self.out = nn.Linear(c.d, c.d)
        self.norm2 = nn.LayerNorm(c.d)
        self.ff1 = nn.Linear(c.d, c.ff)
        self.ff2 = nn.Linear(c.ff, c.d)
        self.drop = nn.Dropout(c.dropout)

    def forward(self, x: Tensor, pos: Tensor) -> Tensor:
        b, t, d = x.shape
        h, dh = self.c.heads, d // self.c.heads
        q, k, v = self.qkv(self.norm1(x)).view(b, t, 3, h, dh).unbind(2)
        q, k = rope(q, pos, self.c.theta), rope(k, pos, self.c.theta)
        a = F.scaled_dot_product_attention(
            q.transpose(1, 2), k.transpose(1, 2), v.transpose(1, 2),
            is_causal=True, dropout_p=self.c.dropout if self.training else 0.0,
        )
        x = x + self.drop(self.out(a.transpose(1, 2).reshape(b, t, d)))
        return x + self.drop(self.ff2(F.relu(self.ff1(self.norm2(x)))))


class LM(nn.Module):
    """The decoder-only LM; `forward` gives next-token logits."""

    def __init__(self, c: Config):
        super().__init__()
        self.config = c
        self.emb = nn.Embedding(c.vocab, c.d)
        self.layers = nn.ModuleList(Block(c) for _ in range(c.layers))
        self.norm = nn.LayerNorm(c.d)
        nn.init.normal_(self.emb.weight, std=0.02)
        for block in self.layers:
            nn.init.normal_(block.out.weight, std=0.02 / math.sqrt(2 * c.layers))
            nn.init.normal_(block.ff2.weight, std=0.02 / math.sqrt(2 * c.layers))

    def forward(self, x: Tensor) -> Tensor:
        """Next-token logits `[B, T, vocab]` for tokens `x [B, T]` at positions 0..T-1."""
        pos = torch.arange(x.shape[1], device=x.device)
        h = self.emb(x)
        for block in self.layers:
            h = block(h, pos)
        return self.norm(h) @ self.emb.weight.T
