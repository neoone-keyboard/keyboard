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


def clean(text):
    return " ".join(strip_wikitext(text).split())


def test_poem_text_is_kept_for_wikisource():
    got = list(paragraphs("<poem>\nഒന്നാം വരി ഇവിടെ ഉണ്ട്\nരണ്ടാം വരി ഇവിടെ ഉണ്ട്\n</poem>"))
    assert got == ["ഒന്നാം വരി ഇവിടെ ഉണ്ട്", "രണ്ടാം വരി ഇവിടെ ഉണ്ട്"]


def test_file_links_with_nested_links_in_the_caption_are_removed_whole():
    text = "Before [[File:Map.png|thumb|The [[Malabar Coast|coast]] of [[India]]]] after [[Image:A.jpg|x]] end"
    assert clean(text) == "Before after end"
    assert clean("[[പ്രമാണം:a.jpg|ലഘു|[[കേരളം]] ഭൂപടം]] ശേഷം [[വർഗ്ഗം:ഇന്ത്യ]] [[en:Kerala]]") == "ശേഷം"


def test_article_links_with_colons_are_kept():
    assert clean("He saw [[Star Wars: Episode I|the film]] and [[Mission: Impossible]] twice") == (
        "He saw the film and Mission: Impossible twice"
    )
    assert clean("[[a|b|c]]") == "c"


def test_nested_templates_params_and_tables_inside_templates_are_removed():
    text = (
        "Start {{Infobox|name={{{name|x}}}|body={{outer|{{inner|1}}|\n"
        "{| class=wikitable\n| a || {{b}}\n|}\n}}|end=1}} middle {{cite|a}} finish"
    )
    assert clean(text) == "Start middle finish"


def test_unclosed_template_or_ref_is_dropped_to_end_of_paragraph():
    assert clean("Kept here.\n\nSecond {{Infobox|a=b\n| c = d\n\nThird stays.") == "Kept here. Second Third stays."
    assert clean("Visible text <ref name=a>never closed\nNext line stays") == "Visible text Next line stays"


def test_html_tags_become_spaces_and_entities_are_unescaped_first():
    assert clean("Foo<br/>bar and<br>baz") == "Foo bar and baz"
    assert clean("Text&nbsp;here &lt;ref&gt;hidden&lt;/ref&gt; done &lt;b&gt;bold&lt;/b&gt;") == "Text here done bold"
    assert clean("1 < 2 and 3 > 2") == "1 < 2 and 3 > 2"


def test_urls_magic_words_and_redirects_are_removed():
    assert clean("__NOTOC__ See https://example.org/a?b=1 and www.example.com now") == "See and now"
    assert clean("#REDIRECT [[Kerala]]\nReal text follows") == "Real text follows"


def test_wikisource_page_namespace_is_read_when_asked():
    dump = """<mediawiki xmlns="http://www.mediawiki.org/xml/export-0.11/">
  <siteinfo><namespaces>
    <namespace key="0" case="first-letter" />
    <namespace key="104" case="first-letter">താൾ</namespace>
  </namespaces></siteinfo>
  <page><title>പുസ്തകം</title><ns>0</ns><revision><text>&lt;pages index="x.pdf" from=1 to=2 /&gt;</text></revision></page>
  <page><title>താൾ:x.pdf/1</title><ns>104</ns><revision><text>ഇത് ഒരു താളിലെ വാക്യം ആണ്</text></revision></page>
  <page><title>Talk</title><ns>1</ns><revision><text>ഇത് ഒരു സംവാദ താൾ ആണ്</text></revision></page>
</mediawiki>""".encode()
    for extra in (["104"], ["താൾ"], ["Page", "താൾ"]):
        out = io.StringIO()
        extract([io.BytesIO(dump)], out, None, extra)
        assert out.getvalue().splitlines() == ["ഇത് ഒരു താളിലെ വാക്യം ആണ്"], extra
    out = io.StringIO()
    extract([io.BytesIO(dump)], out, None)
    assert out.getvalue() == ""


def test_malayalam_words_broken_across_printed_lines_are_joined():
    # Wikisource Page text keeps the book's line-end hyphens.
    text = "ഇഷ്ടാനുരൂപം തപോനിഷ്ഠയാ വസിക്ക സ-\nന്തുഷ്ട്യാ താപസകുലം വാഴുന്നു"
    assert list(paragraphs(text)) == ["ഇഷ്ടാനുരൂപം തപോനിഷ്ഠയാ വസിക്ക സന്തുഷ്ട്യാ താപസകുലം വാഴുന്നു"]


def test_assamese_words_broken_across_printed_lines_are_joined():
    text = "মই আজি ঘৰলৈ যা-\nওঁ বুলি ক'লে"
    assert list(paragraphs(text)) == ["মই আজি ঘৰলৈ যাওঁ বুলি ক'লে"]


def test_assamese_category_and_file_links_are_removed():
    text = "অসম ভাৰতৰ এখন ৰাজ্য [[শ্ৰেণী:ভাৰতৰ ৰাজ্য]] [[চিত্ৰ:x.jpg|থাম্ব]] [[গুৱাহাটী]]"
    assert strip_wikitext(text).split() == ["অসম", "ভাৰতৰ", "এখন", "ৰাজ্য", "গুৱাহাটী"]


def test_english_line_end_hyphens_are_left_alone():
    text = "the town is well-\nknown for its old fort"
    assert list(paragraphs(text)) == ["the town is well-", "known for its old fort"]


def test_sanskrit_verse_in_malayalam_script_is_skipped():
    sanskrit = [
        "അശ്വത്ഥാമ്നശ് ച ജനനീ കൃപശ് ചൈവ മഹാബലഃ",
        "15 ന വിഗ്രഹഃ കുതസ് തന്ദ്രീ ന ദ്വേഷോ നാപി വൈ കൃതം",
        "ഹേതോർനിദർശനകൃതഃ സ ദൃഷ്ടാന്ത ഇതി സ്മൃതഃ || 142||",
    ]
    malayalam = [
        "അദ്ദേഹം ഒരു വലിയ ക്ഷേത്രം പണിതു എന്ന് പറയപ്പെടുന്നു",
        "ദുഃഖം നിറഞ്ഞ ഒരു കാലത്ത് അവൻ വീട്ടിൽ നിന്ന് പോയി",
    ]
    assert list(paragraphs("\n".join(sanskrit + malayalam))) == malayalam


def test_lines_without_malayalam_are_never_taken_for_sanskrit():
    assert list(paragraphs("x || y and some more words")) == ["x || y and some more words"]
