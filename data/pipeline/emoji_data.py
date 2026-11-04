"""Unicode emoji-test.txt and CLDR annotations -> the keyboard's emoji files.

emoji.txt is the panel: "=Group" lines, then one line per emoji in Unicode's order,
"emoji version", plus "toneVersion tone…" for one that takes skin tones (one tone per
person only, as the panel's long-press shows them). emoji-words.txt maps a lowercased
one-word keyword or name to its emoji, best first, as "word<TAB>emoji emoji…" sorted by
UTF-8 bytes so the keyboard can binary-search it in place. Only fully-qualified emoji;
the Component group (lone skin tones and hair styles) is left out."""

import argparse
import sys
import unicodedata
import xml.etree.ElementTree as ET
from dataclasses import dataclass, field
from pathlib import Path

TONES = {chr(c) for c in range(0x1F3FB, 0x1F400)}
VS16 = "️"
SKIPPED_GROUPS = {"Component"}
# Enough for the user's own choice among them to matter; only one is shown.
MAX_EMOJI_PER_WORD = 6
# Grammar words CLDR lists as keywords (☝️ for "this", 🤮 for "up"): an emoji after every
# one of them would be noise, so they get none.
STOP_WORDS = {"at", "do", "down", "got", "not", "off", "out", "over", "that", "this", "up"}


@dataclass
class Emoji:
    emoji: str
    version: str
    tones: list[str] = field(default_factory=list)
    tone_version: str | None = None


def _key(emoji: str) -> str:
    """The emoji without skin tones and FE0F: a toned form and its base share it."""
    return "".join(c for c in emoji if c not in TONES and c != VS16)


def _version_tuple(version: str) -> tuple[int, ...]:
    return tuple(int(part) for part in version.split("."))


def parse_emoji_test(text: str) -> list[tuple[str, list[Emoji]]]:
    groups: list[tuple[str, list[Emoji]]] = []
    bases: dict[str, Emoji] = {}
    skipping = False
    for line in text.splitlines():
        if line.startswith("# group:"):
            name = line.split(":", 1)[1].strip()
            skipping = name in SKIPPED_GROUPS
            if not skipping:
                groups.append((name, []))
            continue
        if skipping or not line or line.startswith("#"):
            continue
        codes, rest = line.split(";", 1)
        status, comment = rest.split("#", 1)
        if status.strip() != "fully-qualified":
            continue
        emoji = "".join(chr(int(code, 16)) for code in codes.split())
        version = comment.split()[1].removeprefix("E")
        tones = {c for c in emoji if c in TONES}
        if not tones:
            entry = Emoji(emoji, version)
            bases[_key(emoji)] = entry
            groups[-1][1].append(entry)
        elif len(tones) == 1 and (base := bases.get(_key(emoji))):
            base.tones.append(emoji)
            if base.tone_version is None or _version_tuple(version) > _version_tuple(base.tone_version):
                base.tone_version = version
    return groups


def catalog_lines(groups: list[tuple[str, list[Emoji]]]) -> list[str]:
    lines = [
        "# The keyboard's emoji panel, from Unicode emoji-test.txt (Unicode License v3; see",
        "# data/manifests/unicode-emoji.json). Built by data/pipeline/emoji_data.py.",
        "# \"=Group\", or \"emoji version [toneVersion tone tone ...]\".",
    ]
    for name, entries in groups:
        lines.append(f"={name}")
        for e in entries:
            tones = f" {e.tone_version} {' '.join(e.tones)}" if e.tones else ""
            lines.append(f"{e.emoji} {e.version}{tones}")
    return lines


def _is_word(text: str) -> bool:
    """Letters (with their combining marks), digits and apostrophes, with a letter in it
    and two characters at least: what the keyboard takes as one word."""
    ok = all(c.isalpha() or c.isdigit() or c == "'" or unicodedata.category(c).startswith("M") for c in text)
    return ok and any(c.isalpha() for c in text) and len(text) >= 2


def parse_annotations(xml: str, groups: list[tuple[str, list[Emoji]]]) -> dict[str, list[str]]:
    """Word -> emoji, best first: an emoji whose name is the word, then the panel's order,
    which puts faces and hearts first (😀 for "happy", 🥰 for "love"). Fewer keywords first
    was tried: it put 🏩 first for "love" and 🦸 for "good"."""
    order: dict[str, tuple[int, str]] = {}
    for _, entries in groups:
        for e in entries:
            order[_key(e.emoji)] = (len(order), e.emoji)
    keywords: dict[str, list[str]] = {}
    names: dict[str, str] = {}
    for node in ET.fromstring(xml).iter("annotation"):
        found = order.get(_key(node.get("cp", "")))
        if not found or not node.text:
            continue
        emoji = found[1]
        if node.get("type") == "tts":
            names[emoji] = node.text.strip().lower()
        else:
            keywords[emoji] = [k.strip().lower() for k in node.text.split("|")]
    ranked: dict[str, list[tuple[tuple[bool, int], str]]] = {}
    for emoji in keywords.keys() | names.keys():
        own = keywords.get(emoji, [])
        name = names.get(emoji)
        rank = order[_key(emoji)][0]
        for word in {w for w in own + ([name] if name else []) if _is_word(w) and w not in STOP_WORDS}:
            ranked.setdefault(word, []).append(((word != name, rank), emoji))
    return {word: [e for _, e in sorted(found)][:MAX_EMOJI_PER_WORD] for word, found in ranked.items()}


def word_lines(words: dict[str, list[str]]) -> list[str]:
    header = [
        "# Emoji suggestions: lowercased word<TAB>emoji, best first, from Unicode CLDR annotations",
        "# (Unicode License v3; see data/manifests/cldr-annotations.json). Built by",
        "# data/pipeline/emoji_data.py. Sorted by UTF-8 bytes after these comment lines.",
    ]
    # Python orders strings by code point, which is UTF-8 byte order; the tab sorts first.
    return header + sorted(f"{word}\t{' '.join(emoji)}" for word, emoji in words.items())


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--emoji-test", type=Path, required=True)
    parser.add_argument("--annotations", type=Path, action="append", required=True)
    parser.add_argument("--out", type=Path, required=True, help="directory for emoji.txt and emoji-words.txt")
    args = parser.parse_args(argv)
    groups = parse_emoji_test(args.emoji_test.read_text(encoding="utf-8"))
    words: dict[str, list[str]] = {}
    for path in args.annotations:
        for word, emoji in parse_annotations(path.read_text(encoding="utf-8"), groups).items():
            words[word] = list(dict.fromkeys(words.get(word, []) + emoji))[:MAX_EMOJI_PER_WORD]
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "emoji.txt").write_text("\n".join(catalog_lines(groups)) + "\n", encoding="utf-8")
    (args.out / "emoji-words.txt").write_text("\n".join(word_lines(words)) + "\n", encoding="utf-8")
    count = sum(len(entries) for _, entries in groups)
    print(f"{args.out}: {count:,} emoji in {len(groups)} groups, {len(words):,} words", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
