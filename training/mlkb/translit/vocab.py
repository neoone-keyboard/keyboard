"""Token vocabularies for the transliterator.

Latin: `pad bos eos`, then `a`–`z`. A native script: `pad bos eos`, then every code point of its
128-code-point block (U+0D00–U+0D7F for Malayalam, U+0900–U+097F for Devanagari, U+0980–U+09FF
for Bengali-Assamese), then ZWJ and ZWNJ, then any extra characters the language writes inside
words (Assamese's apostrophe, হ'ল: 134 tokens, one more than Malayalam's). `native(lang)` builds
it from the language's config. Token ids are what the exported model file and the Rust forward
pass use, so never reorder them. `MALAYALAM` is the default target everywhere; the functions
below take the vocabulary as `vocab`.
"""

import json
from dataclasses import dataclass
from functools import cache
from pathlib import Path


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


def block(start: int, extra: tuple[str, ...] = ()) -> Vocab:
    """A native vocabulary: the 128 code points from `start`, ZWJ, ZWNJ, then `extra`."""
    return Vocab(tuple(chr(c) for c in range(start, start + 0x80)) + ("\u200d", "\u200c") + extra)


LATIN = Vocab(tuple(chr(c) for c in range(ord("a"), ord("z") + 1)))
MALAYALAM = block(0x0D00)
BENGALI_ASSAMESE = block(0x0980, ("'",))

ROOT = Path(__file__).resolve().parents[3]
PACK_LANGS = ROOT / "data/langs"


@cache
def _index(chars: tuple[str, ...]) -> dict[str, int]:
    return {c: i + 3 for i, c in enumerate(chars)}


def _config(lang_id: str) -> tuple[tuple[int, int], tuple[str, ...]]:
    """A language's script block and extra word characters: from its training config
    (`mlkb/langs/<id>.json`, e.g. hi) or else its pack-language config
    (`data/langs/<id>/lang.json`, e.g. as, whose `"apostrophe": true` adds `'`)."""
    from mlkb import langs

    try:
        return langs.load(lang_id).block, ()
    except FileNotFoundError:
        pass
    path = PACK_LANGS / lang_id / "lang.json"
    if not path.exists():
        raise ValueError(f"{lang_id}: no mlkb/langs/{lang_id}.json or data/langs/{lang_id}/lang.json")
    raw = json.loads(path.read_text(encoding="utf-8"))
    first, last = (int(c, 16) for c in raw["block"])
    return (first, last), ("'",) if raw.get("apostrophe") else ()


@cache
def native(lang_id: str = "ml") -> Vocab:
    """The target vocabulary of a language's transliterator (`MALAYALAM` for `ml`)."""
    if lang_id == "ml":
        return MALAYALAM
    (first, last), extra = _config(lang_id)
    if last - first != 0x7F:
        raise ValueError(f"{lang_id}: a script block must be 128 code points")
    return block(first, extra)


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
