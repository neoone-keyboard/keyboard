"""The transliterator: a small pre-norm transformer encoder–decoder over characters.

Layout (the `state_dict` keys, which Task 6 exports one by one and Task 7 re-implements):
- `src_emb.weight [src_vocab, d]`, `src_pos.weight [max_len, d]`: Latin token + learned position.
- `enc.{i}.*`: `nn.TransformerEncoderLayer` (pre-norm, ReLU): `self_attn.in_proj_{weight,bias}`
  (packed q, k, v: `[3d, d]`), `self_attn.out_proj.*`, `linear1.*`, `linear2.*`, `norm1.*`, `norm2.*`.
- `enc_norm.*`: the final encoder layer norm.
- `tgt_emb.weight [tgt_vocab, d]`, `tgt_pos.weight [max_len, d]`: Malayalam token + position.
- `dec.{i}.*`: `nn.TransformerDecoderLayer` (pre-norm, ReLU): `self_attn.*`, `multihead_attn.*`
  (cross-attention over the encoder output), `linear1.*`, `linear2.*`, `norm1..3.*`.
- `dec_norm.*`: the final decoder layer norm.
- Output logits are `dec_norm(x) @ tgt_emb.weight.T`: tied, no bias.

Embeddings are added unscaled. Dropout applies only in training.
"""

from dataclasses import asdict, dataclass

import torch
from torch import Tensor, nn

from mlkb.translit.vocab import LATIN, MALAYALAM


@dataclass(frozen=True)
class Config:
    """Model shape. `ff=512` (not 768) keeps the default at about 4.5M parameters, so the
    int8 file stays under 5 MB."""

    d: int = 256
    heads: int = 4
    enc_layers: int = 4
    dec_layers: int = 3
    ff: int = 512
    max_len: int = 32
    dropout: float = 0.1
    src_vocab: int = len(LATIN)
    tgt_vocab: int = len(MALAYALAM)

    def to_dict(self) -> dict:
        return asdict(self)


class Translit(nn.Module):
    """Latin token ids in, Malayalam next-token logits out."""

    def __init__(self, config: Config):
        super().__init__()
        c = self.config = config
        self.src_emb = nn.Embedding(c.src_vocab, c.d)
        self.src_pos = nn.Embedding(c.max_len, c.d)
        self.tgt_emb = nn.Embedding(c.tgt_vocab, c.d)
        self.tgt_pos = nn.Embedding(c.max_len, c.d)
        self.enc = nn.ModuleList(
            nn.TransformerEncoderLayer(c.d, c.heads, c.ff, c.dropout, batch_first=True, norm_first=True)
            for _ in range(c.enc_layers)
        )
        self.enc_norm = nn.LayerNorm(c.d)
        self.dec = nn.ModuleList(
            nn.TransformerDecoderLayer(c.d, c.heads, c.ff, c.dropout, batch_first=True, norm_first=True)
            for _ in range(c.dec_layers)
        )
        self.dec_norm = nn.LayerNorm(c.d)
        self.drop = nn.Dropout(c.dropout)
        for emb in (self.src_emb, self.src_pos, self.tgt_emb, self.tgt_pos):
            nn.init.normal_(emb.weight, std=0.02)

    def encode(self, src: Tensor) -> tuple[Tensor, Tensor]:
        """Encoder output `[batch, src_len, d]` and the source padding mask (True at pad)."""
        pad = src == LATIN.pad
        pos = torch.arange(src.shape[1], device=src.device)
        x = self.drop(self.src_emb(src) + self.src_pos(pos))
        for layer in self.enc:
            x = layer(x, src_key_padding_mask=pad)
        return self.enc_norm(x), pad

    def decode(self, tgt: Tensor, memory: Tensor, src_pad: Tensor) -> Tensor:
        """Logits `[batch, tgt_len, tgt_vocab]` for each position of `tgt` (teacher forcing)."""
        n = tgt.shape[1]
        pos = torch.arange(n, device=tgt.device)
        causal = torch.triu(torch.ones(n, n, dtype=torch.bool, device=tgt.device), diagonal=1)
        tgt_pad = tgt == MALAYALAM.pad
        x = self.drop(self.tgt_emb(tgt) + self.tgt_pos(pos))
        for layer in self.dec:
            x = layer(
                x,
                memory,
                tgt_mask=causal,
                tgt_key_padding_mask=tgt_pad,
                memory_key_padding_mask=src_pad,
            )
        return self.dec_norm(x) @ self.tgt_emb.weight.T

    def forward(self, src: Tensor, tgt: Tensor) -> Tensor:
        memory, src_pad = self.encode(src)
        return self.decode(tgt, memory, src_pad)


def param_count(model: nn.Module) -> int:
    return sum(p.numel() for p in model.parameters())
