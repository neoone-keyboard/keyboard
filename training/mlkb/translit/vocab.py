"""Token vocabularies for the transliterator.

Latin: `pad bos eos`, then `a`–`z`. Malayalam: `pad bos eos`, then every code point in
U+0D00–U+0D7F, then ZWJ and ZWNJ. Token ids are what the exported model file and the Rust
forward pass use, so never reorder them.

A pack language in another script has the same layout over its own 128-code-point block
(`NATIVE`, `native(lang)`): Assamese's is U+0980–U+09FF (Bengali-Assamese), plus the
apostrophe Assamese writes inside words (হ'ল), so its target vocabulary has 134 tokens, one more
than Malayalam's. The functions below take the vocabulary as `vocab`, Malayalam by default.
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


def block(start: int, extra: tuple[str, ...] = ()) -> Vocab:
    """A native vocabulary: the 128 code points from `start`, ZWJ, ZWNJ, then `extra`."""
    return Vocab(tuple(chr(c) for c in range(start, start + 0x80)) + ("\u200d", "\u200c") + extra)


LATIN = Vocab(tuple(chr(c) for c in range(ord("a"), ord("z") + 1)))
MALAYALAM = block(0x0D00)
BENGALI_ASSAMESE = block(0x0980, ("'",))
NATIVE = {"ml": MALAYALAM, "as": BENGALI_ASSAMESE}
_INDEX = {v.chars: {c: i + 3 for i, c in enumerate(v.chars)} for v in (LATIN, *NATIVE.values())}


def native(lang: str) -> Vocab:
    """The target vocabulary of a language's transliterator."""
    return NATIVE[lang]


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


def fits(latin: str, malayalam: str, max_len: int = 32, vocab: Vocab = MALAYALAM) -> bool:
    """Whether a pair can be encoded and both sides fit `max_len` tokens with bos/eos."""
    if len(latin) + 2 > max_len or len(malayalam) + 2 > max_len:
        return False
    idx_l, idx_m = _INDEX[LATIN.chars], _INDEX[vocab.chars]
    return all(c in idx_l for c in latin) and all(c in idx_m for c in malayalam)
