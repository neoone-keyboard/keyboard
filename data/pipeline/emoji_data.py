"""Unicode emoji-test.txt and CLDR annotations -> the keyboard's emoji files.

emoji.txt is the panel: "=Group" lines, then one line per emoji in Unicode's order,
"emoji version", plus "toneVersion tone…" for one that takes skin tones (one tone per
person only, as the panel's long-press shows them). emoji-words.txt maps a lowercased
one-word keyword or name to its emoji, best first, as "word<TAB>emoji emoji…" sorted by
UTF-8 bytes so the keyboard can binary-search it in place. emoji-search.txt is the
panel's search, in the same form: every word of each emoji's name and keywords ("red" and
"heart" from "red heart"), with all its emoji in the panel's order, those it names first,
then "|" and those it is only a keyword of. Only fully-qualified
emoji; the Component group (lone skin tones and hair styles) is left out.

The app's two word files hold English (`--annotations`). A pack language's words go to its own
pair of files (`--pack-annotations ID=FILE`, `--pack-out ID=DIR`), the pack's optional
`emojiWords` and `emojiSearch` roles: the same formats, that language's annotations and
everyday flag names only. The keyboard reads the app's files and then each active pack's."""

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
# Everyday names for flags that CLDR names only in full ("United States"), so a word of
# them suggests the flag. Our own list, not CLDR's. "us" is left out: it's a grammar word.
FLAG_NAMES = {
    "usa": "🇺🇸", "america": "🇺🇸",
    "uk": "🇬🇧", "britain": "🇬🇧",
    "uae": "🇦🇪", "emirates": "🇦🇪", "dubai": "🇦🇪",
    "korea": "🇰🇷",
    "holland": "🇳🇱",
}
# The same for a pack's language, by pack id: they go into that pack's files.
PACK_FLAG_NAMES = {
    "ml": {
        "അമേരിക്ക": "🇺🇸",
        "ബ്രിട്ടൻ": "🇬🇧", "യുകെ": "🇬🇧",
        "യുഎഇ": "🇦🇪", "ദുബായ്": "🇦🇪",
        "കൊറിയ": "🇰🇷",
    },
}
STOP_WORDS = {
    "at", "do", "down", "got", "not", "off", "out", "over", "that", "this", "up",
    # The same grammar words in the pack languages: this, that, on, up.
    "यह", "वह", "पर",  # hi
    "हे", "ते", "वर",  # mr
    "પર",  # gu
}


@dataclass
class Emoji:
    emoji: str
    version: str
    tones: list[str] = field(default_factory=list)
    tone_version: str | None = None


def _key(emoji: str) -> str:
    """The emoji without skin tones and FE0F: a toned form and its base share it."""
    return "".join(c for c in emoji if c not in TONES and c != VS16)


def _toned(node: ET.Element) -> bool:
    """A skin-toned form, as annotationsDerived lists them: its words ("medium skin tone") would
    otherwise land on the untoned emoji it shares a key with."""
    return any(c in TONES for c in node.get("cp", ""))


def _is_flag(emoji: str) -> bool:
    """A country's or region's flag: two regional indicators, or the black flag with tag letters
    (England, Scotland, Wales)."""
    regional = [c for c in emoji if 0x1F1E6 <= ord(c) <= 0x1F1FF]
    return len(regional) == 2 == len(emoji) or (emoji.startswith("\U0001F3F4") and any(0xE0061 <= ord(c) <= 0xE007A for c in emoji))


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
    return {word: [e for _, e in found] for word, found in ranked_annotations(xml, groups).items()}


def ranked_annotations(xml: str, groups: list[tuple[str, list[Emoji]]]) -> dict[str, list[tuple[bool, str]]]:
    """As `parse_annotations`, each emoji with whether the word is its name."""
    order: dict[str, tuple[int, str]] = {}
    for _, entries in groups:
        for e in entries:
            order[_key(e.emoji)] = (len(order), e.emoji)
    keywords: dict[str, list[str]] = {}
    names: dict[str, str] = {}
    for node in ET.fromstring(xml).iter("annotation"):
        found = order.get(_key(node.get("cp", "")))
        if not found or not node.text or _toned(node):
            continue
        emoji = found[1]
        if node.get("type") == "tts":
            name = node.text.strip().lower()
            # A country's flag is named "flag: India" (annotationsDerived): the country is its name.
            if _is_flag(emoji) and ":" in name:
                name = name.split(":", 1)[1].strip()
            names[emoji] = name
        else:
            keywords[emoji] = [k.strip().lower() for k in node.text.split("|")]
    ranked: dict[str, list[tuple[tuple[bool, int], str]]] = {}
    for emoji in keywords.keys() | names.keys():
        own = keywords.get(emoji, [])
        name = names.get(emoji)
        rank = order[_key(emoji)][0]
        for word in {w for w in own + ([name] if name else []) if _is_word(w) and w not in STOP_WORDS}:
            ranked.setdefault(word, []).append(((word != name, rank), emoji))
    return {word: [(not keyword, e) for (keyword, _), e in sorted(found)][:MAX_EMOJI_PER_WORD]
            for word, found in ranked.items()}


def merge_words(files: list[dict[str, list[tuple[bool, str]]]]) -> dict[str, list[str]]:
    """The words of several annotation files: for each word the emoji it names, then the
    others, each in the files' order (so 🇦🇺 from the derived names comes before 🐨)."""
    merged: dict[str, list[tuple[bool, str]]] = {}
    for words in files:
        for word, found in words.items():
            merged.setdefault(word, []).extend(found)
    return {
        word: list(dict.fromkeys([e for named, e in found if named] + [e for named, e in found if not named]))[:MAX_EMOJI_PER_WORD]
        for word, found in merged.items()
    }


def search_words(xml: str, groups: list[tuple[str, list[Emoji]]]) -> dict[str, tuple[list[str], list[str]]]:
    """Word -> (the emoji with the word in their name, those with it only in a keyword), each in
    the panel's order. A name or keyword of several words gives each of them; grammar words and
    one letter give none."""
    order: dict[str, tuple[int, str]] = {}
    for _, entries in groups:
        for e in entries:
            order[_key(e.emoji)] = (len(order), e.emoji)
    named: dict[str, set[tuple[int, str]]] = {}
    found: dict[str, set[tuple[int, str]]] = {}
    for node in ET.fromstring(xml).iter("annotation"):
        entry = order.get(_key(node.get("cp", "")))
        if not entry or not node.text or _toned(node):
            continue
        into = named if node.get("type") == "tts" else found
        for phrase in node.text.split("|"):
            for word in phrase.strip().lower().replace("-", " ").split():
                word = word.strip(",.:;!?\u201c\u201d\"()")
                if _is_word(word) and word not in STOP_WORDS:
                    into.setdefault(word, set()).add(entry)
    return {
        word: ([e for _, e in sorted(named.get(word, set()))],
               [e for _, e in sorted(found.get(word, set()) - named.get(word, set()))])
        for word in named.keys() | found.keys()
    }


def word_lines(words: dict[str, list[str]], pack: str | None = None) -> list[str]:
    header = [
        "# Emoji suggestions: lowercased word<TAB>emoji, best first, from Unicode CLDR annotations",
        "# (Unicode License v3; see data/manifests/cldr-annotations.json). Built by",
        "# data/pipeline/emoji_data.py. Sorted by UTF-8 bytes after these comment lines.",
    ] + _pack_header(pack, "emojiWords")
    # Python orders strings by code point, which is UTF-8 byte order; the tab sorts first.
    return header + sorted(f"{word}\t{' '.join(emoji)}" for word, emoji in words.items())


def _pack_header(pack: str | None, role: str) -> list[str]:
    """One more comment line for a pack's file: whose words these are."""
    return [] if pack is None else [f"# The words of pack {pack} (its {role} role); the app's own file holds English."]


def search_lines(words: dict[str, tuple[list[str], list[str]]], pack: str | None = None) -> list[str]:
    header = [
        "# Emoji search: lowercased word<TAB>the emoji it names | the emoji it is a keyword of,",
        "# each in the panel's order, from Unicode CLDR annotations (Unicode License v3; see",
        "# data/manifests/cldr-annotations.json). Built by data/pipeline/emoji_data.py. Sorted by",
        "# UTF-8 bytes after these comment lines.",
    ] + _pack_header(pack, "emojiSearch")
    return header + sorted(
        f"{word}\t{' '.join(named + (['|'] if others else []) + others)}"
        for word, (named, others) in words.items()
    )


def build_words(
    annotations: list[str], flags: dict[str, str], groups: list[tuple[str, list[Emoji]]]
) -> tuple[dict[str, list[str]], dict[str, tuple[list[str], list[str]]]]:
    """The suggestion words and the search words of one language: its annotation files (their
    XML text, in order) and its everyday flag names."""
    ranked: list[dict[str, list[tuple[bool, str]]]] = []
    search: dict[str, tuple[list[str], list[str]]] = {}
    for xml in annotations:
        ranked.append(ranked_annotations(xml, groups))
        for word, (named, others) in search_words(xml, groups).items():
            had_named, had_others = search.get(word, ([], []))
            all_named = list(dict.fromkeys(had_named + named))
            search[word] = (all_named, [e for e in dict.fromkeys(had_others + others) if e not in all_named])
    words = merge_words([{word: [(True, flag)] for word, flag in flags.items()}] + ranked)
    for word, flag in flags.items():
        named, others = search.get(word, ([], []))
        search[word] = ([flag] + [e for e in named if e != flag], [e for e in others if e != flag])
    return words, search


def _by_id(pairs: list[str], what: str) -> dict[str, list[Path]]:
    out: dict[str, list[Path]] = {}
    for pair in pairs:
        pack, sep, path = pair.partition("=")
        if not sep or not pack or not path:
            raise SystemExit(f"{what} wants ID=PATH, got {pair!r}")
        out.setdefault(pack, []).append(Path(path))
    return out


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--emoji-test", type=Path, required=True)
    parser.add_argument("--annotations", type=Path, action="append", required=True,
                        help="an English annotation file, for the app's files")
    parser.add_argument("--out", type=Path, required=True, help="directory for emoji.txt and the app's word files")
    parser.add_argument("--pack-annotations", action="append", default=[], metavar="ID=FILE",
                        help="an annotation file of a pack's language")
    parser.add_argument("--pack-out", action="append", default=[], metavar="ID=DIR",
                        help="directory for that pack's emoji-words.txt and emoji-search.txt")
    args = parser.parse_args(argv)
    groups = parse_emoji_test(args.emoji_test.read_text(encoding="utf-8"))
    read_all = lambda paths: [path.read_text(encoding="utf-8") for path in paths]  # noqa: E731
    words, search = build_words(read_all(args.annotations), FLAG_NAMES, groups)
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "emoji.txt").write_text("\n".join(catalog_lines(groups)) + "\n", encoding="utf-8")
    (args.out / "emoji-words.txt").write_text("\n".join(word_lines(words)) + "\n", encoding="utf-8")
    (args.out / "emoji-search.txt").write_text("\n".join(search_lines(search)) + "\n", encoding="utf-8")
    count = sum(len(entries) for _, entries in groups)
    print(f"{args.out}: {count:,} emoji in {len(groups)} groups, {len(words):,} words, {len(search):,} search words",
          file=sys.stderr)
    annotations = _by_id(args.pack_annotations, "--pack-annotations")
    outs = _by_id(args.pack_out, "--pack-out")
    if set(annotations) != set(outs):
        raise SystemExit("--pack-annotations and --pack-out name different packs")
    for pack, paths in annotations.items():
        words, search = build_words(read_all(paths), PACK_FLAG_NAMES.get(pack, {}), groups)
        for out in outs[pack]:
            out.mkdir(parents=True, exist_ok=True)
            (out / "emoji-words.txt").write_text("\n".join(word_lines(words, pack)) + "\n", encoding="utf-8")
            (out / "emoji-search.txt").write_text("\n".join(search_lines(search, pack)) + "\n", encoding="utf-8")
            print(f"{out}: pack {pack}, {len(words):,} words, {len(search):,} search words", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
