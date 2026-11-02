import bz2
import io

from extract_wiki import extract, paragraphs, strip_wikitext

PAGE = """<mediawiki xmlns="http://www.mediawiki.org/xml/export-0.11/">
  <page><title>Kerala</title><ns>0</ns><revision><text>{text}</text></revision></page>
  <page><title>Talk:Kerala</title><ns>1</ns><revision><text>Talk page text that is long enough</text></revision></page>
  <page><title>KL</title><ns>0</ns><redirect title="Kerala"/><revision><text>#REDIRECT [[Kerala]] with more words</text></revision></page>
</mediawiki>"""

ARTICLE = """{{Infobox state|name=Kerala}}
'''Kerala''' is a [[States of India|state]] on the [[Malabar Coast]] of India.<ref>Some source</ref>
== History ==
[[File:Kerala map.png|thumb|A map of Kerala]]
* Kerala has many rivers and lakes.
{| class="wikitable"
| a || b
|}
Short line.
[https://example.org The official site] lists the districts of the state.
[[Category:States of India]]"""


def test_strips_markup():
    text = strip_wikitext("'''Bold''' [[a|b]] [[c]] {{t|{{inner}}}} <ref>x</ref> &amp;")
    assert text.split() == ["Bold", "b", "c", "&"]


def test_paragraphs_keep_running_text_only():
    got = list(paragraphs(ARTICLE))
    assert got == [
        "Kerala is a state on the Malabar Coast of India.",
        "Kerala has many rivers and lakes.",
        "The official site lists the districts of the state.",
    ]


def test_extracts_articles_and_skips_talk_pages_and_redirects():
    dump = bz2.compress(PAGE.format(text=ARTICLE.replace("&", "&amp;").replace("<", "&lt;")).encode())
    out = io.StringIO()
    words = extract([io.BytesIO(bz2.decompress(dump))], out, None)
    lines = out.getvalue().splitlines()
    assert lines[0] == "Kerala is a state on the Malabar Coast of India."
    assert len(lines) == 3
    assert words == sum(len(line.split()) for line in lines)


def test_stops_at_max_words():
    out = io.StringIO()
    extract([io.BytesIO(PAGE.format(text=ARTICLE.replace("<", "&lt;")).encode())], out, 5)
    assert len(out.getvalue().splitlines()) == 1


def test_malayalam_text_passes_through():
    got = list(paragraphs("'''കേരളം''' ഇന്ത്യയിലെ ഒരു [[സംസ്ഥാനം|സംസ്ഥാനമാണ്]]."))
    assert got == ["കേരളം ഇന്ത്യയിലെ ഒരു സംസ്ഥാനമാണ്."]
