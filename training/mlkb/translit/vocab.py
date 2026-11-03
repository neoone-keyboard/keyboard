"""Token vocabularies for the transliterator.

Latin: `pad bos eos`, then `a`–`z`. Malayalam: `pad bos eos`, then every code point in
U+0D00–U+0D7F, then ZWJ and ZWNJ. Token ids are what the exported model file and the Rust
forward pass use, so never reorder them.
"""

from dataclasses import dataclass


@dataclass(frozen=True)
class Vocab:
    """A fixed token list: three specials, then one token per character."""

    chars: tuple[str, ...]
    pad: int = 0
    bos: int = 1
    eos: int = 2

    def __len__(self) -> int:
        return 3 + len(self.chars)

    def index(self, char: str) -> int:
        """The token of `char`; KeyError if it isn't in the vocabulary."""
        return _INDEX[self.chars][char]

    def char(self, token: int) -> str:
        """The character of a non-special token."""
        return self.chars[token - 3]


LATIN = Vocab(tuple(chr(c) for c in range(ord("a"), ord("z") + 1)))
MALAYALAM = Vocab(tuple(chr(c) for c in range(0x0D00, 0x0D80)) + ("‍", "‌"))
_INDEX = {v.chars: {c: i + 3 for i, c in enumerate(v.chars)} for v in (LATIN, MALAYALAM)}


def _encode(vocab: Vocab, text: str) -> list[int]:
    return [vocab.bos, *(vocab.index(c) for c in text), vocab.eos]


def encode_latin(text: str) -> list[int]:
    """`bos`, one token per letter of `text` (lowercased), `eos`. KeyError on anything but a–z."""
    return _encode(LATIN, text.lower())


def encode_ml(text: str) -> list[int]:
    """`bos`, one token per code point of `text`, `eos`. KeyError outside the vocabulary."""
    return _encode(MALAYALAM, text)


def decode_ml(tokens) -> str:
    """The text of `tokens`: a leading `bos` and any `pad` are skipped, and it stops at `eos`."""
    out = []
    for t in tokens:
        t = int(t)
        if t == MALAYALAM.eos:
            break
        if t in (MALAYALAM.pad, MALAYALAM.bos):
            continue
        out.append(MALAYALAM.char(t))
    return "".join(out)


def fits(latin: str, malayalam: str, max_len: int = 32) -> bool:
    """Whether a pair can be encoded and both sides fit `max_len` tokens with bos/eos."""
    if len(latin) + 2 > max_len or len(malayalam) + 2 > max_len:
        return False
    idx_l, idx_m = _INDEX[LATIN.chars], _INDEX[MALAYALAM.chars]
    return all(c in idx_l for c in latin) and all(c in idx_m for c in malayalam)
