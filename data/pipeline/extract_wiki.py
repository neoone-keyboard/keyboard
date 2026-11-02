"""Wikipedia / Wikisource XML dump (.xml.bz2) -> plain text, one paragraph per line.

A small streaming parser with regex markup stripping. It is not a full
wikitext renderer: it only needs running text good enough to count words.
"""

import argparse
import bz2
import html
import re
import sys
import xml.etree.ElementTree as ET
from collections.abc import Iterable, Iterator
from typing import BinaryIO, TextIO

COMMENT = re.compile(r"<!--.*?-->", re.S)
# Tags whose content is not running text.
SKIPPED_BLOCKS = re.compile(
    r"<(ref|math|gallery|code|syntaxhighlight|source|timeline|score|chem|hiero|graph|mapframe|templatedata|imagemap|poem)\b[^>]*?(/>|>.*?</\1\s*>)",
    re.S | re.I,
)
TAG = re.compile(r"<[^>]+>")
TEMPLATE = re.compile(r"\{\{[^{}]*\}\}")
TABLE = re.compile(r"\{\|[^{}]*?\|\}", re.S)
# [[File:...]], [[Category:...]], [[en:...]], [[പ്രമാണം:...]]: any namespaced link.
NAMESPACED_LINK = re.compile(r"\[\[[^\[\]|]*:[^\[\]]*\]\]")
PIPED_LINK = re.compile(r"\[\[[^\[\]|]*\|([^\[\]]*)\]\]")
LINK = re.compile(r"\[\[([^\[\]]*)\]\]")
EXTERNAL_LINK = re.compile(r"\[(?:https?:)?//[^\s\]]+\s*([^\]]*)\]")
BOLD_ITALIC = re.compile(r"'{2,}")
HEADING = re.compile(r"^=+.*=+\s*$")
LIST_MARKER = re.compile(r"^[*#:;]+\s*")
SPACES = re.compile(r"[ \t ]+")

MIN_WORDS = 4


def strip_wikitext(text: str) -> str:
    text = COMMENT.sub("", text)
    text = SKIPPED_BLOCKS.sub("", text)
    for pattern in (TEMPLATE, TABLE):
        for _ in range(10):  # nested templates: remove the innermost first
            text, n = pattern.subn("", text)
            if n == 0:
                break
    text = NAMESPACED_LINK.sub("", text)
    text = PIPED_LINK.sub(r"\1", text)
    text = LINK.sub(r"\1", text)
    text = EXTERNAL_LINK.sub(r"\1", text)
    text = TAG.sub("", text)
    text = BOLD_ITALIC.sub("", text)
    return html.unescape(text)


def paragraphs(wikitext: str) -> Iterator[str]:
    for line in strip_wikitext(wikitext).splitlines():
        line = line.strip()
        if not line or HEADING.match(line) or line[0] in "|!{}":
            continue
        line = SPACES.sub(" ", LIST_MARKER.sub("", line)).strip()
        if len(line.split()) >= MIN_WORDS:
            yield line


def article_texts(stream: BinaryIO) -> Iterator[str]:
    """Wikitext of every main-namespace page that isn't a redirect."""
    context = ET.iterparse(stream, events=("start", "end"))
    _, root = next(context)
    for event, elem in context:
        if event != "end" or not elem.tag.endswith("}page"):
            continue
        ns = elem.findtext("{*}ns")
        redirect = elem.find("{*}redirect")
        text = elem.findtext("{*}revision/{*}text")
        if ns == "0" and redirect is None and text:
            yield text
        root.clear()  # keep memory flat on multi-GB dumps


def extract(streams: Iterable[BinaryIO], out: TextIO, max_words: int | None) -> int:
    words = 0
    for stream in streams:
        for text in article_texts(stream):
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
    args = parser.parse_args(argv)
    with open(args.out, "w", encoding="utf-8") as out:
        streams = (bz2.open(path, "rb") for path in args.dumps)
        words = extract(streams, out, args.max_words)
    print(f"{args.out}: {words:,} words", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
