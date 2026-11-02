"""Wikipedia / Wikisource XML dump (.xml.bz2) -> plain text, one paragraph per line.

A small streaming parser with markup stripping. It is not a full wikitext
renderer: it only needs running text good enough to count words. Templates,
tables and links are removed with bracket-balancing scans, not regexes, so
nesting is handled; anything left unclosed is dropped to the end of its
paragraph rather than leaked into the corpus.
"""

import argparse
import bz2
import html
import re
import sys
import xml.etree.ElementTree as ET
from collections.abc import Iterable, Iterator
from typing import BinaryIO, TextIO

COMMENT = re.compile(r"<!--.*?(?:-->|\Z)", re.S)
# Tags whose content is not running text. (<poem> is NOT here: Wikisource verse lives in it.)
SKIPPED_TAGS = (
    "ref|math|gallery|code|syntaxhighlight|source|timeline|score|chem|hiero|graph|mapframe|templatedata|imagemap"
)
SKIPPED_BLOCKS = re.compile(
    rf"<({SKIPPED_TAGS})\b[^>]*?(/>|>.*?</\1\s*>)",
    re.S | re.I,
)
# An opening tag that never closes: drop it and the rest of its line.
UNCLOSED_BLOCK = re.compile(rf"<(?:{SKIPPED_TAGS})\b[^>]*>[^\n]*", re.I)
TAG = re.compile(r"</?[A-Za-z][^>]*>")
EXTERNAL_LINK = re.compile(r"\[(?:https?:)?//[^\s\]]+\s*([^\]]*)\]")
BARE_URL = re.compile(r"(?:https?|ftp)://[^\s<>\[\]|]+|\bwww\.[^\s<>\[\]|]+", re.I)
MAGIC_WORD = re.compile(r"__[A-Z_]+__")
REDIRECT_LINE = re.compile(r"^\s*#redirect\b.*$", re.I | re.M)
BOLD_ITALIC = re.compile(r"'{2,}")
HEADING = re.compile(r"^=+.*=+\s*$")
LIST_MARKER = re.compile(r"^[*#:;]+\s*")
SPACES = re.compile(r"[ \t ]+")

# Link prefixes that are not article text: namespaces (English and Malayalam) and interwikis.
LINK_NAMESPACES = {
    "file", "image", "media", "category", "special",
    "പ്രമാണം", "ചിത്രം", "വർഗ്ഗം", "പ്രത്യേകം",
}  # fmt: skip
INTERWIKI = re.compile(r"^[a-z]{2,3}(?:-[a-z]+)*$")

MIN_WORDS = 4


def remove_braces(text: str) -> str:
    """Delete {{templates}}, {{{params}}} and {|tables|}, nested to any depth.

    An opener that never closes is dropped up to the next blank line (or the end)."""
    out: list[str] = []
    i = 0
    n = len(text)
    while i < n:
        start = _opener(text, i)
        if start is None:
            nxt = min((p for p in (text.find("{{", i), text.find("{|", i)) if p != -1), default=-1)
            if nxt == -1:
                out.append(text[i:])
                break
            out.append(text[i:nxt])
            i = nxt
            continue
        end = _balanced_end(text, i)
        if end is None:
            cut = text.find("\n\n", i)
            if cut == -1:
                break
            i = cut
        else:
            i = end
    return "".join(out)


def _opener(text: str, i: int) -> str | None:
    if text.startswith("{{{", i):
        return "{{{"
    if text.startswith("{{", i):
        return "{{"
    if text.startswith("{|", i):
        return "{|"
    return None


def _balanced_end(text: str, i: int) -> int | None:
    stack: list[str] = []
    n = len(text)
    while i < n:
        opener = _opener(text, i)
        if opener:
            stack.append(opener)
            i += len(opener)
            continue
        top = stack[-1]
        if top == "{{{" and text.startswith("}}}", i):
            stack.pop()
            i += 3
        elif top == "{{" and text.startswith("}}", i):
            stack.pop()
            i += 2
        elif top == "{|" and text.startswith("|}", i):
            stack.pop()
            i += 2
        else:
            i += 1
        if not stack:
            return i
    return None


def _link_end(text: str, i: int) -> int | None:
    """Index just past the `]]` that closes the `[[` at i, counting nested links."""
    depth = 0
    n = len(text)
    while i < n:
        if text.startswith("[[", i):
            depth += 1
            i += 2
        elif text.startswith("]]", i):
            depth -= 1
            i += 2
            if depth == 0:
                return i
        else:
            i += 1
    return None


def _drops(target: str) -> bool:
    target = target.strip()
    if target.startswith(":") or ":" not in target:  # [[:Category:x]] is a visible link
        return False
    prefix = target.split(":", 1)[0].strip()
    return prefix.casefold() in LINK_NAMESPACES or bool(INTERWIKI.match(prefix))


def replace_links(text: str) -> str:
    """[[a|b|c]] -> c, [[a]] -> a; File/Category/interwiki links are removed whole."""
    out: list[str] = []
    i = 0
    while True:
        start = text.find("[[", i)
        if start == -1:
            out.append(text[i:])
            break
        out.append(text[i:start])
        end = _link_end(text, start)
        if end is None:  # unclosed: drop the brackets, keep the words
            i = start + 2
            continue
        inner = text[start + 2 : end - 2]
        target = inner.split("|", 1)[0]
        if not _drops(target):
            label = inner.split("|")[-1]
            out.append(replace_links(label.lstrip(":")))
        i = end
    return "".join(out)


def strip_wikitext(text: str) -> str:
    text = html.unescape(text)  # so that &lt;ref&gt; is seen as a tag
    text = COMMENT.sub("", text)
    text = REDIRECT_LINE.sub("", text)
    text = SKIPPED_BLOCKS.sub("", text)
    text = UNCLOSED_BLOCK.sub("", text)
    text = remove_braces(text)
    text = replace_links(text)
    text = EXTERNAL_LINK.sub(r"\1", text)
    text = BARE_URL.sub("", text)
    text = MAGIC_WORD.sub("", text)
    text = TAG.sub(" ", text)
    text = BOLD_ITALIC.sub("", text)
    return text.replace("[[", "").replace("]]", "")


def paragraphs(wikitext: str) -> Iterator[str]:
    for line in strip_wikitext(wikitext).splitlines():
        line = line.strip()
        if not line or HEADING.match(line) or line[0] in "|!{}":
            continue
        line = SPACES.sub(" ", LIST_MARKER.sub("", line)).strip()
        if len(line.split()) >= MIN_WORDS:
            yield line


def article_texts(stream: BinaryIO, extra_namespaces: Iterable[str] = ()) -> Iterator[str]:
    """Wikitext of every wanted page that isn't a redirect.

    Main namespace always; plus any namespace in `extra_namespaces`, given as a
    key ("104") or a name from the dump's <siteinfo> ("Page")."""
    wanted = {s.strip().casefold() for s in extra_namespaces if s.strip()}
    names: dict[str, str] = {}  # key -> name, from <siteinfo><namespaces>
    context = ET.iterparse(stream, events=("start", "end"))
    _, root = next(context)
    for event, elem in context:
        if event != "end":
            continue
        if elem.tag.endswith("}namespace"):
            names[elem.get("key", "")] = (elem.text or "").strip().casefold()
            continue
        if not elem.tag.endswith("}page"):
            continue
        ns = elem.findtext("{*}ns")
        redirect = elem.find("{*}redirect")
        text = elem.findtext("{*}revision/{*}text")
        keep = ns == "0" or (ns is not None and (ns in wanted or names.get(ns, "\0") in wanted))
        if keep and redirect is None and text:
            yield text
        root.clear()  # keep memory flat on multi-GB dumps


def extract(
    streams: Iterable[BinaryIO], out: TextIO, max_words: int | None, extra_namespaces: Iterable[str] = ()
) -> int:
    words = 0
    for stream in streams:
        for text in article_texts(stream, extra_namespaces):
            for para in paragraphs(text):
                out.write(para + "\n")
                words += len(para.split())
                if max_words is not None and words >= max_words:
                    return words
    return words


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dumps", nargs="+", help=".xml.bz2 dump files, read in order")
    parser.add_argument("--out", required=True, help="output text file")
    parser.add_argument("--max-words", type=int, default=None, help="stop after this many words")
    parser.add_argument(
        "--extra-namespaces",
        default="",
        help="comma-separated namespace keys or names to read besides the main one "
        "(Wikisource proofread text is in 'Page', key 104)",
    )
    args = parser.parse_args(argv)
    extra = args.extra_namespaces.split(",")
    with open(args.out, "w", encoding="utf-8") as out:
        streams = (bz2.open(path, "rb") for path in args.dumps)
        words = extract(streams, out, args.max_words, extra)
    print(f"{args.out}: {words:,} words", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
