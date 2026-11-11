"""Which language a bilingual LM pairs with English: everything the LM pipeline reads per language.

Malayalam (`ml`) is the default of every `mlkb.lm` command, with the paths and names it has
always had (`build/lm`, `ml.txt`, `manglish-*`), so its commands and outputs are unchanged. Hindi
(`hi`) builds in `build/lm-hi` and `build/lm-hi.pt`, with `hi` and `hinglish-*` buckets.

Assamese (`as`) and Tamil (`ta`) are packs built from `data/langs/<id>/` (`pack_lang`): their
romanised text is spelled by the pack's fitted tables (`mlkb.langs.romanisation`), and their LM
trains only on what the pack's manifest credits. So their English is English Wikipedia alone
(`english`: no Simple English Wikipedia, Wikinews or Gutenberg bucket), nothing of theirs reads
Dakshina (`dakshina` is None), and their native text is Wikipedia, then Wikisource, then
Wikiquote (`data/build-<id>-text.sh`).

    from mlkb.lm.lang import load
    lang = load("hi")
"""
import re
from dataclasses import dataclass
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
DEFAULT = "ml"


@dataclass(frozen=True)
class LmLang:
    id: str
    name: str
    # Bucket names: the native-script text and the synthetic romanised text (`<romanised>-<k>`).
    romanised: str
    raw_text: str  # in data/raw/text
    block: tuple[int, int]  # the script's Unicode block (the engine's `ScriptRules::block`)
    word: re.Pattern  # a native word, as the synthetic buckets romanise it
    pack: Path
    lexicon: str  # file in the pack
    romanised_lexicon: str
    translit: str
    dakshina: str | None  # Dakshina's language directory; None for a language that reads none
    aksharantar_train: Path
    eval_dir: Path  # where this language's next-word eval sets are written
    build: str  # relative to training/: the text, vocabulary and shards
    checkpoint: str  # relative to training/
    romanised_passes: int
    mixed_passes: int
    synth_parts: int  # each synthetic pass is built in this many parallel parts
    wikisource: str  # manifest id of the Wikisource dump, which follows Wikipedia in `raw_text`
    wikisource_namespaces: tuple[str, ...]
    # Inflected endings: a mined Aksharantar pair for a word with one of these drops the ending
    # (കമ്മിഷനും -> "commission"), so such a word is not a loanword.
    loanword_endings: tuple[str, ...]
    reference_texts: tuple[str, ...]
    long_text: str
    # The English buckets the LM trains on (`corpus.ENGLISH`): all four unless the pack credits
    # only some of their sources.
    english: tuple[str, ...] = ("en-wiki", "en-simple", "en-news", "en-gutenberg")
    # A pack of `data/langs/<id>/`: spellings from `mlkb.langs.romanisation.Tables`, and the
    # engine opened with the pack's `lang.json`.
    pack_lang: bool = False
    # The characters of `word` (a regex class body), when `word` is more than one class.
    word_chars: str = ""
    # What follows Wikisource in `raw_text`, if anything ("Wikiquote"): the memorisation check
    # draws its Wikisource prompts from there to the end.
    after_wikisource: str = ""

    @property
    def native(self) -> str:
        """The native-script bucket: `ml`, `hi`."""
        return self.id

    @property
    def dakshina_romanized(self) -> Path | None:
        if self.dakshina is None:
            return None
        return REPO / f"data/raw/dakshina/dakshina_dataset_v1.0/{self.dakshina}/romanized"

    @property
    def wikisource_dump(self) -> Path | None:
        return next(iter(sorted((REPO / "data/raw" / self.wikisource).glob("*.xml.bz2"))), None)

    @property
    def tune_file(self) -> Path:
        return self.eval_dir / f"next-{self.id}.tune.txt"

    @property
    def gate_file(self) -> Path:
        return self.eval_dir / f"next-{self.id}.heldout.txt"

    def in_block(self, c: str) -> bool:
        return self.block[0] <= ord(c) <= self.block[1]

    def romanised_buckets(self) -> list[str]:
        return [f"{self.romanised}-{i}" for i in range(self.romanised_passes)]

    def mixed_buckets(self) -> list[str]:
        return [f"mixed-{i}" for i in range(self.mixed_passes)]


ML = LmLang(
    id="ml",
    name="Malayalam",
    romanised="manglish",
    raw_text="ml.txt",
    block=(0x0D00, 0x0D7F),
    word=re.compile("[ഀ-ൿ‌‍]+"),
    pack=REPO / "packs/ml.neopack",
    lexicon="ml.fst",
    romanised_lexicon="manglish.fst",
    translit="translit.mlkbnn",
    dakshina="ml",
    aksharantar_train=REPO / "data/raw/aksharantar/extracted/mal_train.json",
    eval_dir=REPO / "data/eval",
    build="build/lm",
    checkpoint="build/lm.pt",
    romanised_passes=4,
    mixed_passes=2,
    synth_parts=1,
    wikisource="mlwikisource",
    wikisource_namespaces=("104", "Page", "താൾ"),
    loanword_endings=("ും", "ിൽ", "ന്റെ", "ുടെ", "ിനെ", "ിന്", "ിനു", "ക്ക്", "ാണ്", "ായി", "ിലെ", "ോട്", "ോടെ"),
    reference_texts=("are you free this evening", "ഞാൻ നാളെ വീട്ടിൽ വരും", "njan nale veettil varum, ok"),
    long_text=("the old house at the end of the road was empty for years, and then one summer a family "
               "moved in. ഞങ്ങൾ അവിടെ പോയി, avar nalla aalukal aanu ennu thonni. we stayed for dinner."),
)

# Devanagari letters and signs, without the dandas, digits and abbreviation sign (U+0964-U+0970).
_DEVA = "ऀ-ॣॱ-ॿ‌‍"

HI = LmLang(
    id="hi",
    name="Hindi",
    romanised="hinglish",
    raw_text="hi.txt",
    block=(0x0900, 0x097F),
    word=re.compile(f"[{_DEVA}]+"),
    pack=REPO / "packs/hi.neopack",
    lexicon="hi.fst",
    romanised_lexicon="hinglish.fst",
    translit="translit.mlkbnn",
    dakshina="hi",
    aksharantar_train=REPO / "data/raw/aksharantar-hin/extracted/hin_train.json",
    eval_dir=REPO / "data/eval/hi",
    build="build/lm-hi",
    checkpoint="build/lm-hi.pt",
    # Hindi Wikipedia is about 3.7 times Malayalam's (100M words against 27M), so one pass of
    # Hinglish is already about 150M tokens: the design's 187M Hinglish and 121M mixed tokens
    # seen (§5) take 1.2 and 0.8 passes, and more passes would only fill the disk.
    romanised_passes=2,
    mixed_passes=1,
    synth_parts=8,
    wikisource="hiwikisource",
    wikisource_namespaces=("250", "पृष्ठ"),
    # Plural and oblique endings: कंप्यूटरों, मशीनें, फ़ैक्टरियाँ.
    loanword_endings=("ों", "ें", "ियां", "ियाँ"),
    reference_texts=("are you free this evening", "मैं कल घर आऊंगा", "main kal ghar aaunga, ok"),
    long_text=("the old house at the end of the road was empty for years, and then one summer a family "
               "moved in. हम वहाँ गए, ve bahut ache log lage. we stayed for dinner."),
)

# Bengali-Assamese letters and signs (with ৰ and ৱ, U+09F0-09F1), without the digits (U+09E6-
# 09EF) and the currency and fraction signs after them.
_BENG = "ঀ-ৣৰৱৼ৾‌‍"

AS = LmLang(
    id="as",
    name="Assamese",
    romanised="romanised",
    raw_text="as.txt",
    block=(0x0980, 0x09FF),
    # A word may hold an apostrophe (ক'ত, হ'ল), which the tables spell as a vowel.
    word=re.compile(f"[{_BENG}]+(?:'[{_BENG}]+)*"),
    word_chars=_BENG,
    pack=REPO / "packs/as.neopack",
    lexicon="as.fst",
    romanised_lexicon="romanised.fst",
    translit="translit.mlkbnn",
    dakshina=None,
    aksharantar_train=REPO / "data/raw/aksharantar-asm/extracted/asm_train.json",
    eval_dir=REPO / "data/eval/as",
    build="build/lm-as",
    checkpoint="build/lm-as.pt",
    # Assamese text is smaller than Malayalam's, so it takes Malayalam's passes.
    romanised_passes=4,
    mixed_passes=2,
    synth_parts=8,
    wikisource="aswikisource",
    wikisource_namespaces=("104", "Page", "পৃষ্ঠা"),
    after_wikisource="Wikiquote",
    # Plural and definite endings: কম্পিউটাৰবোৰ, স্কুলসমূহ, বাছখন.
    loanword_endings=("বোৰ", "সমূহ", "বিলাক", "সকল", "খন", "টো"),
    reference_texts=("are you free this evening", "মই কাইলৈ ঘৰত আহিম", "moi kailoi ghorot ahim, ok"),
    long_text=("the old house at the end of the road was empty for years, and then one summer a family "
               "moved in. আমি তাত গৈছিলো, tekhetxokol bhal manuh jen lagil. we stayed for dinner."),
    english=("en-wiki",),
    pack_lang=True,
)

# Tamil letters and signs (U+0B82-0BD7), without the digits and number signs (U+0BE6-0BFA).
_TAML = "ஂ-ௗ‌‍"

TA = LmLang(
    id="ta",
    name="Tamil",
    romanised="tanglish",
    raw_text="ta.txt",
    block=(0x0B80, 0x0BFF),
    word=re.compile(f"[{_TAML}]+"),
    pack=REPO / "packs/ta.neopack",
    lexicon="ta.fst",
    romanised_lexicon="romanised.fst",
    translit="translit.mlkbnn",
    dakshina=None,
    aksharantar_train=REPO / "data/raw/aksharantar-tam/extracted/tam_train.json",
    eval_dir=REPO / "data/eval/ta",
    build="build/lm-ta",
    checkpoint="build/lm-ta.pt",
    romanised_passes=2,
    mixed_passes=1,
    synth_parts=8,
    wikisource="tawikisource",
    wikisource_namespaces=("250", "Page", "பக்கம்"),
    after_wikisource="Wikiquote",
    # Case, plural and clitic endings: கம்ப்யூட்டரில், பஸ்ஸுக்கு, ஸ்கூல்கள், டீயும்.
    loanword_endings=("ில்", "ுக்கு", "க்கு", "ின்", "ை", "ும்", "கள்", "ுடன்", "ாக", "ால்"),
    reference_texts=("are you free this evening", "நான் நாளை வீட்டுக்கு வருவேன்", "naan naalai veetukku varuven, ok"),
    long_text=("the old house at the end of the road was empty for years, and then one summer a family "
               "moved in. நாங்கள் அங்கே போனோம், avanga romba nalla manushanga nu thonuchu. we stayed for dinner."),
    english=("en-wiki",),
    pack_lang=True,
)

LANGS = {"ml": ML, "hi": HI, "as": AS, "ta": TA}


def load(lang_id: str = DEFAULT) -> LmLang:
    """The LM config of `lang_id`; KeyError for a language with no LM pipeline."""
    return LANGS[lang_id]
