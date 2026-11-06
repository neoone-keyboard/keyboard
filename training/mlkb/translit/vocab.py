"""Token vocabularies for the transliterator.

Latin: `pad bos eos`, then `a`–`z`. A native script: `pad bos eos`, then every code point of its
128-code-point block (U+0D00–U+0D7F for Malayalam, U+0900–U+097F for Devanagari), then ZWJ and
ZWNJ (`native(lang)`). Token ids are what the exported model file and the Rust forward pass use,
so never reorder them. `MALAYALAM` is the default target everywhere.
"""

from dataclasses import dataclass
from functools import cache


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
        return _index(self.chars)[char]

    def char(self, token: int) -> str:
        """The character of a non-special token."""
        return self.chars[token - 3]


def block_vocab(first: int) -> Vocab:
    """The target vocabulary of the 128-code-point script block starting at `first`."""
    return Vocab(tuple(chr(c) for c in range(first, first + 0x80)) + ("‍", "‌"))


LATIN = Vocab(tuple(chr(c) for c in range(ord("a"), ord("z") + 1)))
MALAYALAM = block_vocab(0x0D00)


@cache
def _index(chars: tuple[str, ...]) -> dict[str, int]:
    return {c: i + 3 for i, c in enumerate(chars)}


@cache
def native(lang_id: str = "ml") -> Vocab:
    """The target vocabulary of a language in `mlkb/langs` (`MALAYALAM` for `ml`)."""
    from mlkb import langs

    lang = langs.load(lang_id)
    if lang.block[1] - lang.block[0] != 0x7F:
        raise ValueError(f"{lang_id}: a script block must be 128 code points")
    return MALAYALAM if lang.block[0] == 0x0D00 else block_vocab(lang.block[0])


def _encode(vocab: Vocab, text: str) -> list[int]:
    return [vocab.bos, *(vocab.index(c) for c in text), vocab.eos]


def encode_latin(text: str) -> list[int]:
    """`bos`, one token per letter of `text` (lowercased), `eos`. KeyError on anything but a–z."""
    return _encode(LATIN, text.lower())


def encode_ml(text: str, vocab: Vocab = MALAYALAM) -> list[int]:
    """`bos`, one token per code point of `text`, `eos`. KeyError outside the vocabulary."""
    return _encode(vocab, text)


def decode_ml(tokens, vocab: Vocab = MALAYALAM) -> str:
    """The text of `tokens`: a leading `bos` and any `pad` are skipped, and it stops at `eos`."""
    out = []
    for t in tokens:
        t = int(t)
        if t == vocab.eos:
            break
        if t in (vocab.pad, vocab.bos):
            continue
        out.append(vocab.char(t))
    return "".join(out)


def fits(latin: str, native_text: str, max_len: int = 32, vocab: Vocab = MALAYALAM) -> bool:
    """Whether a pair can be encoded and both sides fit `max_len` tokens with bos/eos."""
    if len(latin) + 2 > max_len or len(native_text) + 2 > max_len:
        return False
    idx_l, idx_m = _index(LATIN.chars), _index(vocab.chars)
    return all(c in idx_l for c in latin) and all(c in idx_m for c in native_text)
