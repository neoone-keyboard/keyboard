"""Which language a bilingual LM pairs with English: everything the LM pipeline reads per language.

Malayalam (`ml`) is the default of every `mlkb.lm` command, with the paths and names it has
always had (`build/lm`, `ml.txt`, `manglish-*`), so its commands and outputs are unchanged. Hindi
(`hi`) builds in `build/lm-hi` and `build/lm-hi.pt`, with `hi` and `hinglish-*` buckets.

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
    dakshina: str  # Dakshina's language directory
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

    @property
    def native(self) -> str:
        """The native-script bucket: `ml`, `hi`."""
        return self.id

    @property
    def dakshina_romanized(self) -> Path:
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

LANGS = {"ml": ML, "hi": HI}


def load(lang_id: str = DEFAULT) -> LmLang:
    """The LM config of `lang_id`; KeyError for a language with no LM pipeline."""
    return LANGS[lang_id]
