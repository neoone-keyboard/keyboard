"""Project Gutenberg books for the English LM text (licence audit: public domain).

We keep a book only if every author has a death year before `CUTOFF` (public domain under
life+70 and India's life+60), strip the Project Gutenberg header and footer (the PG licence
covers only the trademark), and refuse any book whose header says it is copyrighted.
"""
import re

CUTOFF = 1956
_LIFE = re.compile(r"(\d{3,4})\??-(\d{3,4})\??")
_START = re.compile(r"^\*\*\* ?START OF (THE|THIS) PROJECT GUTENBERG EBOOK.*$", re.M | re.I)
_END = re.compile(r"^\*\*\* ?END OF (THE|THIS) PROJECT GUTENBERG EBOOK.*$", re.M | re.I)
_COPYRIGHTED = re.compile(r"copyrighted", re.I)
# Straight or curly double quotes: most UTF-8 books use “…”.
_QUOTED = re.compile(r"[\"“]([^\"“”]{2,400})[\"”]")


def authors_dead_before(authors: str, cutoff: int) -> bool:
    """`authors` is the catalogue's `Authors` field ("Name, 1775-1817; Name, …")."""
    people = [a for a in authors.split(";") if a.strip()]
    if not people:
        return False
    for person in people:
        life = _LIFE.search(person)
        if not life or int(life.group(2)) >= cutoff:
            return False
    return True


def strip_boilerplate(text: str) -> str | None:
    """The book's own text, or None if its header marks it as copyrighted."""
    start, end = _START.search(text), _END.search(text)
    head = text[: start.start()] if start else text[:3000]
    if _COPYRIGHTED.search(head):
        return None
    body = text[start.end() if start else 0 : end.start() if end else len(text)]
    return body.strip()


def dialogue_lines(paragraph: str) -> list[str]:
    """The quoted speech in a paragraph: the closest thing to chat in old books."""
    return [m.group(1).strip() for m in _QUOTED.finditer(paragraph)]
