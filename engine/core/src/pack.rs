//! Language packs (M6 design): a folder `<id>.neopack/` holding `manifest.json` and the
//! language's data files. The manifest maps roles (`lexicon`, `romanisedLexicon`,
//! `romanisation`, `nextWords`, `translit`, `lm`, `suffixes`, `chatForms`, `layout`, `lang`) to file
//! names in the folder; `Pack::open` reads and checks it, and `Paths::with_packs` turns it into
//! the paths the engine opens. A phrase-conversion pack (`"conversion": "phrase"`, engine 9:
//! Japanese) has the roles `kana`, `conversionLexicon` and `connection` instead
//! (`crate::convert`).

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::convert::ConversionPaths;
use crate::lang::LangId;
use crate::{PackPaths, Paths};

/// The pack format this engine reads.
pub const FORMAT: u32 = 1;
/// This engine's pack level: a pack whose `minEngine` is higher needs a newer app.
/// 1: phase 1 (Malayalam, its tables compiled in). 2: phase 2, which reads format 1 with the
/// `romanisation`, `suffixes`, `chatForms` and `layout` roles, any number of packs, and the
/// scripts `mlym`, `deva` and `beng` (Hindi and Assamese need it). 3: an `lm` in a
/// pack whose script isn't Malayalam's (Hindi's): the LM's native pieces are the pack's script
/// (`Lm::set_native`). Engine 2 would load such an LM but see none of its native pieces, and
/// rank the pack's words worse than with no LM. 4: the `lang` role, a `lang.json` that defines
/// the pack's script as data (`script::data`), so a pack may be in any script. Only a pack
/// whose script the engine has no copy of (`script::supported`) needs 4: a pack in mlym, deva
/// or beng may carry the file and keep a lower `minEngine`, as older engines ignore the role
/// and have those scripts' rules in code. (4 is also the first level all of whose engines
/// decode a `translit` model in its pack's script; the fix came after 3 was defined, so a
/// `translit` in a pack whose script isn't Malayalam's needs 4.) 5: `lang.json`'s `notLetter`,
/// `looseSigns`, `finalPairs` and `suffixJoins`, with which every script's rules are data
/// (Malayalam's too: the engine's own copies of the mlym, deva and beng files replace the coded
/// plug-ins). Only a pack in another script that uses one of those fields needs 5; Malayalam's
/// pack doesn't, as engines before 5 have its rules in code and ignore the fields. 6:
/// `lang.json`'s `joiners` and `zwjForms` (the normal form's joiner rules, which were
/// Malayalam's in code for every script), and a `suffixJoins.nasal` ending of several code
/// points (Tamil's ம்). A pack whose lexicon was built with them needs 6, as an older engine
/// would drop its joiners and miss its words; Malayalam's doesn't, as older engines have its
/// rules in code. 6 also reads what older engines ignore and replace with Malayalam's:
/// `romanisation.json`'s `looseSpelling`, `swipe` and `fromLatin` (the rule-based speller), so
/// a pack in another script with any of them needs 6.
/// 7: what makes typed and swiped Hinglish reach its words (Hindi 2026.11.13 and .14), all in
/// `romanisation.json` and read for a pack in any script, Malayalam's too, as no older engine
/// has them in code: `optionalFinals` (a word ending in one of those letters has a second key
/// without it, `lexicon::build_with_aliases`, found once and counted once; a swipe takes
/// `swipe::Weights::second` from it), `skeleton.dropTrailing.alsoKept` (a dropped final letter
/// also kept, a second key), `skeleton.alsoBetweenConsonants` (typed letters also read with
/// those letters between consonants dropped) and `skeleton.alsoBesideConsonant` (a letter
/// written for the dropped vowel beside the consonants named; a swipe fitted so gives up
/// `swipe::Weights::stand_in`). An older engine loads such a pack and ignores the fields, so
/// typed nahi, mai or kehte miss their words. 7 also gives each language its own rules where
/// two packs share a script (before it, the pack loaded last gave both its `lang.json`), so a
/// pack in mlym, deva or beng whose `lang.json` isn't the engine's own copy of that script's
/// (Marathi's, Bengali's: engines before 4 read it by the copy, 4 to 6 by whichever pack of
/// the script loaded last) needs 7. Engine behaviour that came with 7 but needs no level: a
/// swipe fits each `chatForms` row too (older engines still offer the rows when typed).
/// 8: `lang.json`'s `charTable` (`packing`), the characters of a script whose letters span
/// more than 128 code points (Urdu's, across the Arabic block), in order: the romanised
/// lexicon packs a word one byte per character by its place in the table, and a
/// transliterator spells in the table's characters (`nn::vocab::Target`). An older engine
/// packs from the script's first code point instead, so it would unpack every romanised key
/// as other characters and refuse the transliterator: a pack whose `lang.json` sets
/// `charTable` needs 8, in any script. A pack without one is packed and spelled as before.
///
/// 9: phrase conversion (`crate::convert`, Japanese): a manifest with `"conversion": "phrase"`
/// and the roles `kana` (`kana.json`, romaji to kana), `conversionLexicon` (reading → surface,
/// part-of-speech ids and cost) and `connection` (the ids' connection costs). Such a pack has no
/// word list: it is opened as a converter, its script is no word script, and an older engine
/// would refuse it for having no lexicon. `notices` (licence texts the app shows, such as
/// IPAdic's, which must travel with the dictionary) is an app role and needs no level.
///
/// No level: `lang.json`'s `direction` ("rtl" or "ltr"), which only the apps read (a mirrored
/// suggestion bar; without it they take the Unicode direction of the script's letters). The engine
/// ignores it, and an older app lays the bar out left to right as before, as for `APP_ROLES`.
///
/// 10: the manifest's `ranking` (`Ranking`), a pack's own ranking choices, which engines before
/// it made by the script code: every choice for a pack in Malayalam's script, none for any
/// other. An older engine ignores the object, so a `mlym` pack that sets every choice
/// (Malayalam's) or a pack in another script that sets none needs no level for it, and any
/// other combination needs 10. Engine behaviour that came with 10 but needs no level: each
/// active pack's prior at the start of the text is its own (`startBelowEnglish`), where before
/// every pack had Malayalam's while a Malayalam pack was active.
///
/// Adding a pack feature an older engine would ignore or misread: raise this, say here what
/// the new level reads, and add its rule to `Pack::needs`, the one place that maps a pack's
/// contents to the level they need (`tests/pack_check.rs` holds every pack in `packs/` to it).
pub const PACK_ENGINE: u32 = 10;

/// `manifest.json`. Keys are camelCase in the file.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format: u32,
    pub id: String,
    pub version: String,
    pub name: String,
    pub native_name: String,
    pub script: String,
    #[serde(default)]
    pub romanised: Option<Romanised>,
    pub min_engine: u32,
    /// The `en.fst` the pack's LM and next words were built against (FNV-1a of the file, as
    /// `Lexicon::fingerprint`, 16 hex digits, little-endian bytes in order). Not checked when a
    /// pack opens: an LM is only loaded when its own tables fit the lexicons (`lm_fits`), and
    /// with another `en.fst` next words still work, only the English pairs are a little off.
    /// Phase 3's installer compares it with the base's to offer an update.
    #[serde(default)]
    pub english_fingerprint: Option<String>,
    /// Role → file name in the pack folder.
    pub files: BTreeMap<String, String>,
    #[serde(default)]
    pub sha256: BTreeMap<String, String>,
    #[serde(default)]
    pub credits: Vec<Credit>,
    /// `"phrase"` for a phrase-conversion pack (engine 9, `crate::convert`); None for a pack of
    /// words.
    #[serde(default)]
    pub conversion: Option<String>,
    /// The pack's own ranking choices (engine 10); all off when left out.
    #[serde(default)]
    pub ranking: Ranking,
}

/// `manifest.json`'s `ranking`: how this pack's words are ranked where packs may differ, one
/// choice per field, each for this pack alone (another active pack is ranked by its own).
/// Every field is false when left out, as every pack but Malayalam is ranked; Malayalam's
/// manifest sets them all, the choices its recorded results were tuned with (`make
/// golden-check`). Fields this engine doesn't know are ignored when a pack opens, and refused
/// by `Pack::needs`, so none ships without a level. docs/design/pack-format.md and
/// docs/make-a-language.md say what each does for a pack's author.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Ranking {
    /// At the start of the text the pack's prior is `rank::CONTEXT_AT_START`, a step behind
    /// English; else `rank::CONTEXT_AT_START_PACK`, level with it (`Ranking::start_prior`).
    pub start_below_english: bool,
    /// A swiped word of the pack competes with a common English word on its score alone; else
    /// it may outrank one only by fitting the path better (`swipe::Weights::english_margin`).
    pub swipe_ignores_english_margin: bool,
    /// The transliterator's lexicon readings and the skeleton's loose matches are ranked by
    /// frequency however far the model puts them behind its best reading; else those
    /// `rank::MANGLISH_MODEL_TRAILS` behind it trail it (`rank::MANGLISH_MODEL_TRAILS_STEP`).
    pub readings_by_frequency: bool,
    /// The lexicon beam keeps all its rows when the free beam runs beside rare words; else it
    /// gives one up (`Engine::add_model`).
    pub keep_lexicon_beam: bool,
    /// A rare word that only the previous word lifted over the floor of words that may replace
    /// is still rare: the free spelling runs and is offered beside it
    /// (`ManglishCandidates::lift`).
    pub lifted_words_stay_rare: bool,
    /// The free spelling offered beside rare words is held under the pack's best word and a
    /// likely English word once context has moved them (`Engine::keep_free_beside`).
    pub hold_own_spelling: bool,
}

impl Ranking {
    /// The fields as `manifest.json` names them, with their values.
    pub fn fields(&self) -> [(&'static str, bool); 6] {
        [
            ("startBelowEnglish", self.start_below_english),
            ("swipeIgnoresEnglishMargin", self.swipe_ignores_english_margin),
            ("readingsByFrequency", self.readings_by_frequency),
            ("keepLexiconBeam", self.keep_lexicon_beam),
            ("liftedWordsStayRare", self.lifted_words_stay_rare),
            ("holdOwnSpelling", self.hold_own_spelling),
        ]
    }

    /// The pack's prior at the start of the text (`Engine::priors`).
    pub fn start_prior(&self) -> f32 {
        if self.start_below_english {
            crate::rank::CONTEXT_AT_START
        } else {
            crate::rank::CONTEXT_AT_START_PACK
        }
    }

    /// The `ranking` of the `manifest.json` in the folder that holds `file`, for a pack's paths
    /// given one by one rather than by its manifest (`PackPaths`): all off without a readable
    /// manifest there.
    pub fn beside(file: &Path) -> Self {
        #[derive(Deserialize)]
        struct Only {
            #[serde(default)]
            ranking: Ranking,
        }
        file.parent()
            .and_then(|dir| std::fs::read_to_string(dir.join("manifest.json")).ok())
            .and_then(|text| serde_json::from_str::<Only>(&text).ok())
            .map(|m| m.ranking)
            .unwrap_or_default()
    }
}

/// The one kind of conversion this engine does: the typed reading of a whole phrase converted
/// at once (Japanese).
pub const PHRASE: &str = "phrase";

/// The roles a phrase-conversion pack must have.
const CONVERSION_ROLES: &[&str] = &["kana", "conversionLexicon", "connection"];

#[derive(Debug, Clone, Deserialize)]
pub struct Romanised {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Credit {
    pub name: String,
    pub licence: String,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug)]
pub enum PackError {
    /// No `manifest.json`, or it couldn't be read.
    Unreadable(String),
    /// Not valid JSON for a manifest.
    Malformed(String),
    /// A format this engine doesn't read.
    Format(u32),
    /// A script this engine has no rules for, or a bad `lang` file: why, in words.
    Script(String),
    /// The pack needs a newer engine.
    Engine(u32),
    /// The required `lexicon` file is missing.
    NoLexicon,
    /// A conversion this engine doesn't do, or a conversion pack without a file it needs.
    Conversion(String),
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable(e) => write!(f, "manifest unreadable: {e}"),
            Self::Malformed(e) => write!(f, "manifest malformed: {e}"),
            Self::Format(v) => write!(f, "pack format {v}, this engine reads {FORMAT}"),
            Self::Script(why) => write!(f, "script rules: {why}"),
            Self::Engine(v) => write!(f, "pack needs engine {v}, this is {PACK_ENGINE}"),
            Self::NoLexicon => write!(f, "pack has no lexicon file"),
            Self::Conversion(why) => write!(f, "conversion: {why}"),
        }
    }
}

impl std::error::Error for PackError {}

/// An opened, checked pack.
#[derive(Debug, Clone)]
pub struct Pack {
    pub dir: PathBuf,
    pub manifest: Manifest,
}

impl Pack {
    pub fn open(dir: &Path) -> Result<Self, PackError> {
        let text = std::fs::read_to_string(dir.join("manifest.json"))
            .map_err(|e| PackError::Unreadable(e.to_string()))?;
        let manifest: Manifest =
            serde_json::from_str(&text).map_err(|e| PackError::Malformed(e.to_string()))?;
        if manifest.format != FORMAT {
            return Err(PackError::Format(manifest.format));
        }
        if manifest.min_engine > PACK_ENGINE {
            return Err(PackError::Engine(manifest.min_engine));
        }
        let pack = Self {
            dir: dir.to_path_buf(),
            manifest,
        };
        if let Some(kind) = pack.manifest.conversion.as_deref() {
            // Not words in a script: no script rules and no lexicon, but its own files.
            if kind != PHRASE {
                return Err(PackError::Conversion(format!("{kind:?}, this engine converts {PHRASE:?}")));
            }
            if let Some(role) = CONVERSION_ROLES.iter().find(|r| pack.file(r).is_none()) {
                return Err(PackError::Conversion(format!("no {role} file")));
            }
            return Ok(pack);
        }
        if let (Some(name), None) = (pack.manifest.files.get("lang"), pack.file("lang")) {
            return Err(PackError::Script(format!(
                "the manifest names a lang file, {name:?}, that isn't in the pack"
            )));
        }
        crate::script::for_pack(&pack.manifest.script, pack.file("lang").as_deref())
        .map_err(PackError::Script)?;
        if pack.file("lexicon").is_none() {
            return Err(PackError::NoLexicon);
        }
        Ok(pack)
    }

    /// The file for `role`, if the manifest names one and it is there.
    pub fn file(&self, role: &str) -> Option<PathBuf> {
        let name = self.manifest.files.get(role)?;
        // A file name, never a path out of the pack.
        if name.is_empty() || name.contains('/') || name.contains('\\') || name.starts_with('.') {
            return None;
        }
        let path = self.dir.join(name);
        path.is_file().then_some(path)
    }
}

impl Pack {
    /// The paths of this pack's files, as the engine opens them.
    pub fn paths(&self) -> PackPaths {
        PackPaths {
            id: LangId(self.manifest.id.clone()),
            script: self.manifest.script.clone(),
            dir: Some(self.dir.clone()),
            lexicon: self.file("lexicon"),
            romanised: self.file("romanisedLexicon"),
            romanisation: self.file("romanisation"),
            next_words: self.file("nextWords"),
            translit: self.file("translit"),
            lm: self.file("lm"),
            suffixes: self.file("suffixes"),
            chat_forms: self.file("chatForms"),
            layout: self.file("layout"),
            lang: self.file("lang"),
            conversion: self.conversion_paths(),
            ranking: self.manifest.ranking,
        }
    }

    /// A phrase-conversion pack's files; None for a pack of words.
    fn conversion_paths(&self) -> Option<ConversionPaths> {
        self.manifest.conversion.as_ref()?;
        Some(ConversionPaths {
            kana: self.file("kana")?,
            dictionary: self.file("conversionLexicon")?,
            connection: self.file("connection")?,
        })
    }
}

impl Paths {
    /// The paths for English at `english` plus `packs`' languages, in order; `user_dir` is kept.
    pub fn with_packs(english: Option<PathBuf>, packs: &[Pack], user_dir: Option<PathBuf>) -> Self {
        Self {
            english,
            english_next: None,
            packs: packs.iter().map(Pack::paths).collect(),
            user_dir,
        }
    }
}

/// One reason a pack needs an engine level: the level, and what in the pack needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Need {
    pub level: u32,
    pub what: String,
}

impl fmt::Display for Need {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.level, self.what)
    }
}

/// How a key of a pack's `lang.json` or `romanisation.json` counts towards the level the pack
/// needs (`PACK_ENGINE`'s history). Every key the files may have is in `LANG_KEYS` or
/// `ROMANISATION_KEYS`: one that isn't is refused, so a new field can't ship without its level.
#[derive(Debug, Clone, Copy)]
enum Key {
    /// Read by every engine that reads the file, or by none (notes, the builder's keys).
    Base,
    /// An object whose own keys are in the table too, as `key.inner`.
    Fields,
    /// Read from this level. Counts when present, even empty, for a pack in a script other
    /// than Malayalam's: an older engine puts Malayalam's rules in its place. Malayalam's own
    /// file says what older engines have in code, so for mlym it doesn't count.
    NotMalayalam(u32),
    /// Read from this level for a pack in any script; counts when set (not false, 0, empty or
    /// null), as an older engine would ignore it.
    Set(u32),
    /// Read by the apps, never by the engine (the script's direction, for the suggestion
    /// bar): needs no level, as `APP_ROLES`. An older app ignores it and lays the bar out as
    /// before.
    App,
}

/// `lang.json`'s keys (`script::data`). The script's definition itself is the `lang` role's,
/// whose level `Pack::needs` decides from the script.
const LANG_KEYS: &[(&str, Key)] = &[
    ("id", Key::Base),
    ("name", Key::Base),
    ("notes", Key::Base),
    ("reference", Key::Base),
    ("aksharantar", Key::Base),
    ("firstLetter", Key::Base),
    ("maxChars", Key::Base),
    ("maxWords", Key::Base),
    ("minCount", Key::Base),
    ("script", Key::Base),
    ("block", Key::Base),
    ("letters", Key::Base),
    ("notWord", Key::Base),
    ("virama", Key::Base),
    ("signs", Key::Base),
    ("bases", Key::Base),
    ("apostrophe", Key::Base),
    ("replace", Key::Base),
    ("phraseBreaks", Key::Base),
    ("notLetter", Key::NotMalayalam(5)),
    ("looseSigns", Key::NotMalayalam(5)),
    ("finalPairs", Key::NotMalayalam(5)),
    ("suffixJoins", Key::NotMalayalam(5)),
    ("joiners", Key::NotMalayalam(6)),
    ("zwjForms", Key::NotMalayalam(6)),
    ("charTable", Key::Set(8)),
    ("direction", Key::App),
];

/// `kana.json`'s keys (`convert::romaji`): all read since the file's level, 9.
const KANA_KEYS: &[(&str, Key)] = &[("format", Key::Base), ("notes", Key::Base), ("rules", Key::Base)];

/// `romanisation.json`'s keys (`romanisation::Raw`), an object's inner keys after a dot.
const ROMANISATION_KEYS: &[(&str, Key)] = &[
    ("format", Key::Base),
    ("script", Key::Base),
    ("notes", Key::Base),
    ("virama", Key::Base),
    ("inherent", Key::Base),
    ("atWordEnd", Key::Base),
    ("independentVowels", Key::Base),
    ("vowelSigns", Key::Base),
    ("consonants", Key::Base),
    ("finals", Key::Base),
    ("clusters", Key::Base),
    ("skeleton", Key::Fields),
    ("skeleton.pairs", Key::Base),
    ("skeleton.singles", Key::Base),
    ("skeleton.collapseDoubles", Key::Base),
    ("skeleton.dropBetweenConsonants", Key::Base),
    ("skeleton.dropFinalVowel", Key::Base),
    ("skeleton.dropTrailing", Key::Fields),
    ("skeleton.dropTrailing.letters", Key::Base),
    ("skeleton.dropTrailing.afterVowel", Key::Base),
    ("skeleton.dropTrailing.minLength", Key::Base),
    ("skeleton.dropTrailing.alsoKept", Key::Set(7)),
    ("skeleton.alsoBetweenConsonants", Key::Set(7)),
    ("skeleton.alsoBesideConsonant", Key::Set(7)),
    ("looseSpelling", Key::NotMalayalam(6)),
    ("swipe", Key::NotMalayalam(6)),
    ("fromLatin", Key::NotMalayalam(6)),
    ("optionalFinals", Key::Set(7)),
];

/// The manifest's roles the engine reads. A pack in Malayalam's script needs no more than 1
/// for any of them (engine 1 has Malayalam's tables, suffixes and chat forms in code); in
/// another script every role needs 2 (the script does), `lm` 3, `translit` 4 and `lang` what
/// `Pack::needs` says.
const ENGINE_ROLES: &[&str] = &[
    "lexicon",
    "romanisedLexicon",
    "nextWords",
    "translit",
    "lm",
    "romanisation",
    "suffixes",
    "chatForms",
    "layout",
    "lang",
    "kana",
    "conversionLexicon",
    "connection",
];

/// Roles the app reads, never the engine: an older app ignores them and reads the pack as
/// before. An app from before them has Malayalam's emoji words bundled, the same words
/// Malayalam's pack brings; any other language's pack is only without its emoji words there.
/// `notices` holds licence texts that must travel with the pack's data and that the app shows
/// beside its credits (IPAdic's, for Japanese's dictionary).
const APP_ROLES: &[&str] = &["emojiWords", "emojiSearch", "notices"];

/// True when `value` says something: not null, false, 0, "" or an empty list or object.
fn is_set(value: &serde_json::Value) -> bool {
    use serde_json::Value;
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// What a pack's JSON file needs, by its keys and `table`.
struct KeyNeeds<'a> {
    file: &'a str,
    table: &'a [(&'a str, Key)],
    malayalam: bool,
}

impl KeyNeeds<'_> {
    /// The needs of `object`'s keys, `prefix` naming where it is (`"skeleton."`).
    fn of(
        &self,
        object: &serde_json::Map<String, serde_json::Value>,
        prefix: &str,
        needs: &mut Vec<Need>,
    ) -> Result<(), String> {
        let file = self.file;
        for (name, value) in object {
            let path = format!("{prefix}{name}");
            let Some(&(_, key)) = self.table.iter().find(|(k, _)| *k == path) else {
                return Err(format!(
                    "{file} has {path}, which no engine level is known to read: give it one in \
                     pack.rs (PACK_ENGINE, Pack::needs)"
                ));
            };
            match key {
                Key::Base | Key::App => {}
                Key::Fields => {
                    let inner = value
                        .as_object()
                        .ok_or_else(|| format!("{file}: {path} is not an object"))?;
                    self.of(inner, &format!("{path}."), needs)?;
                }
                Key::NotMalayalam(level) if !self.malayalam => needs.push(Need {
                    level,
                    what: format!("{file} has {path}"),
                }),
                Key::Set(level) if is_set(value) => needs.push(Need {
                    level,
                    what: format!("{file} sets {path}"),
                }),
                Key::NotMalayalam(_) | Key::Set(_) => {}
            }
        }
        Ok(())
    }

    fn of_file(&self, path: &Path, needs: &mut Vec<Need>) -> Result<(), String> {
        let text =
            std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        match serde_json::from_str(&text) {
            Ok(serde_json::Value::Object(object)) => self.of(&object, "", needs),
            Ok(_) => Err(format!("{}: not a JSON object", path.display())),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }
}

impl Pack {
    /// Everything in the pack that needs an engine level above 1, each with the level that
    /// first reads it right, the highest first. The single place that maps a pack's contents
    /// to `PACK_ENGINE`'s levels: whoever adds a pack feature an older engine would ignore or
    /// misread raises `PACK_ENGINE`, documents the level there and adds its rule here.
    ///
    /// An error, in words, when a file can't be read, or the pack names a role or has a field
    /// no level is known for.
    pub fn needs(&self) -> Result<Vec<Need>, String> {
        let script = self.manifest.script.as_str();
        let malayalam = script == "mlym";
        let mut needs = Vec::new();
        for role in self.manifest.files.keys() {
            if !ENGINE_ROLES.contains(&role.as_str()) && !APP_ROLES.contains(&role.as_str()) {
                return Err(format!(
                    "the manifest names role {role}, which no engine level is known to read: \
                     give it one in pack.rs (PACK_ENGINE, Pack::needs)"
                ));
            }
        }
        let mut need = |level, what: String| needs.push(Need { level, what });
        let has = |role: &str| self.manifest.files.contains_key(role);
        let conversion = self.manifest.conversion.is_some() || CONVERSION_ROLES.iter().any(|r| has(r));
        if conversion {
            // Its script is no word script: only the converter reads the pack.
            need(9, "phrase conversion (kana, conversionLexicon, connection)".into());
            if let Some(path) = self.file("kana") {
                let keys = KeyNeeds { file: "kana.json", table: KANA_KEYS, malayalam };
                keys.of_file(&path, &mut needs)?;
            }
            needs.sort_by(|a, b| b.level.cmp(&a.level).then_with(|| a.what.cmp(&b.what)));
            return Ok(needs);
        }
        if !malayalam {
            need(2, format!("script {script}, not Malayalam's"));
            if has("lm") {
                need(3, format!("an lm in script {script}, whose native pieces are its own"));
            }
            if has("translit") {
                need(4, format!("a translit in script {script}, decoded in its own block"));
            }
        }
        let embedded = crate::script::embedded_def(script);
        if embedded.is_none() {
            need(
                4,
                format!("script {script}, which the engine has no copy of: its rules are the lang file"),
            );
        }
        let lang = self.file("lang");
        if let Some(path) = &lang {
            let def = crate::script::data::ScriptDef::open(path)?;
            if embedded.as_ref().is_some_and(|own| *own != def) {
                need(
                    7,
                    format!(
                        "lang.json isn't the engine's own copy of {script}'s rules: older engines \
                         read the pack by that copy, or by another {script} pack's file"
                    ),
                );
            }
            let several = def
                .joins
                .as_ref()
                .and_then(|j| j.nasal.as_ref())
                .is_some_and(|(ending, _)| ending.chars().count() > 1);
            if several && !malayalam {
                need(6, "lang.json's suffixJoins.nasal ending is several code points".into());
            }
        }
        if let Some(path) = &lang {
            let keys = KeyNeeds { file: "lang.json", table: LANG_KEYS, malayalam };
            keys.of_file(path, &mut needs)?;
        }
        if let Some(path) = self.file("romanisation") {
            let keys = KeyNeeds { file: "romanisation.json", table: ROMANISATION_KEYS, malayalam };
            keys.of_file(&path, &mut needs)?;
        }
        self.ranking_needs(malayalam, &mut needs)?;
        needs.sort_by(|a, b| b.level.cmp(&a.level).then_with(|| a.what.cmp(&b.what)));
        Ok(needs)
    }

    /// What the manifest's `ranking` needs: 10 for each choice an older engine makes the other
    /// way (`PACK_ENGINE`), which for a pack in Malayalam's script is each one left off and for
    /// any other each one set. An error for a field no level is known for.
    fn ranking_needs(&self, malayalam: bool, needs: &mut Vec<Need>) -> Result<(), String> {
        let text = std::fs::read_to_string(self.dir.join("manifest.json"))
            .map_err(|e| format!("manifest.json: {e}"))?;
        let raw: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("manifest.json: {e}"))?;
        let fields = self.manifest.ranking.fields();
        if let Some(ranking) = raw.get("ranking") {
            let object = ranking.as_object().ok_or("manifest.json: ranking is not an object")?;
            if let Some(name) = object.keys().find(|k| !fields.iter().any(|(f, _)| f == k)) {
                return Err(format!(
                    "manifest.json has ranking.{name}, which no engine level is known to read: \
                     give it one in pack.rs (PACK_ENGINE, Pack::needs)"
                ));
            }
        }
        for (name, on) in fields {
            if on != malayalam {
                let what = if on {
                    format!("manifest.json sets ranking.{name}")
                } else {
                    format!("manifest.json leaves ranking.{name} off, which older engines set for script mlym")
                };
                needs.push(Need { level: 10, what });
            }
        }
        Ok(())
    }

    /// The lowest engine level that reads everything in the pack right: the `minEngine` it
    /// needs at least (`needs` says why).
    pub fn needed_engine(&self) -> Result<u32, String> {
        Ok(self.needs()?.first().map_or(1, |n| n.level))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const FULL: &str = r#"{
      "format": 1, "id": "ml", "version": "2026.11.06", "name": "Malayalam",
      "nativeName": "മലയാളം", "script": "mlym", "romanised": {"name": "Manglish"},
      "minEngine": 1,
      "files": {"lexicon": "ml.fst", "romanisedLexicon": "manglish.fst", "nextWords": "next.fst",
                "translit": "translit.mlkbnn", "lm": "lm.mlkbnn"},
      "sha256": {"ml.fst": "00"},
      "credits": [{"name": "Malayalam Wikipedia", "licence": "CC BY-SA 4.0"}],
      "ranking": {"startBelowEnglish": true, "swipeIgnoresEnglishMargin": true,
                  "readingsByFrequency": true, "keepLexiconBeam": true,
                  "liftedWordsStayRare": true, "holdOwnSpelling": true}
    }"#;

    /// Every ranking choice set, as Malayalam's manifest has them (what engines before 10 do
    /// for a pack in script mlym).
    const EVERY_CHOICE: &str = r#"{"startBelowEnglish": true, "swipeIgnoresEnglishMargin": true,
        "readingsByFrequency": true, "keepLexiconBeam": true, "liftedWordsStayRare": true,
        "holdOwnSpelling": true}"#;

    fn pack_dir(manifest: &str, files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("manifest.json"), manifest).unwrap();
        for f in files {
            fs::write(dir.path().join(f), b"x").unwrap();
        }
        dir
    }

    #[test]
    fn opens_a_full_pack() {
        let dir = pack_dir(FULL, &["ml.fst", "manglish.fst", "next.fst", "translit.mlkbnn", "lm.mlkbnn"]);
        let pack = Pack::open(dir.path()).unwrap();
        assert_eq!(pack.manifest.id, "ml");
        assert_eq!(pack.manifest.native_name, "മലയാളം");
        assert_eq!(pack.manifest.romanised.as_ref().unwrap().name, "Manglish");
        assert_eq!(pack.manifest.credits.len(), 1);
        assert_eq!(pack.file("lm"), Some(dir.path().join("lm.mlkbnn")));
    }

    #[test]
    fn a_missing_file_or_role_is_none() {
        let dir = pack_dir(FULL, &["ml.fst"]);
        let pack = Pack::open(dir.path()).unwrap();
        assert_eq!(pack.file("translit"), None);
        assert_eq!(pack.file("emojiWords"), None);
    }

    #[test]
    fn file_names_cannot_leave_the_pack() {
        let manifest = FULL.replace("\"lm.mlkbnn\"", "\"../lm.mlkbnn\"");
        let dir = pack_dir(&manifest, &["ml.fst"]);
        let pack = Pack::open(dir.path()).unwrap();
        assert_eq!(pack.file("lm"), None);
    }

    #[test]
    fn refuses_what_it_cannot_read() {
        let cases = [
            (FULL.replace("\"format\": 1", "\"format\": 2"), "Format"),
            (FULL.replace("\"mlym\"", "\"taml\""), "Script"),
            (FULL.replace("\"minEngine\": 1", "\"minEngine\": 99"), "Engine"),
        ];
        for (manifest, want) in cases {
            let dir = pack_dir(&manifest, &["ml.fst"]);
            let err = Pack::open(dir.path()).unwrap_err();
            assert!(format!("{err:?}").starts_with(want), "{err:?}");
        }
        let dir = pack_dir(FULL, &[]);
        assert!(matches!(Pack::open(dir.path()), Err(PackError::NoLexicon)));
        let dir = pack_dir("{", &[]);
        assert!(matches!(Pack::open(dir.path()), Err(PackError::Malformed(_))));
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(Pack::open(dir.path()), Err(PackError::Unreadable(_))));
    }

    #[test]
    fn the_committed_packs_open() {
        let packs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs");
        for (id, script, min_engine) in [("ml", "mlym", 1), ("hi", "deva", 7), ("as", "beng", 6)] {
            let pack = Pack::open(&packs.join(format!("{id}.neopack"))).unwrap();
            assert_eq!(pack.manifest.id, id);
            assert_eq!(pack.manifest.script, script);
            assert_eq!(pack.manifest.min_engine, min_engine);
            assert!(pack.file("romanisation").is_some(), "{id}");
        }
    }

    #[test]
    fn paths_come_from_the_pack() {
        let dir = pack_dir(FULL, &["ml.fst", "manglish.fst", "next.fst", "translit.mlkbnn", "lm.mlkbnn"]);
        let pack = Pack::open(dir.path()).unwrap();
        let en = PathBuf::from("/x/en.fst");
        let user = PathBuf::from("/u");
        let paths = Paths::with_packs(Some(en.clone()), &[pack], Some(user.clone()));
        assert_eq!(paths.english, Some(en));
        assert_eq!(paths.user_dir, Some(user));
        let [ml] = &paths.packs[..] else { panic!("{paths:?}") };
        assert_eq!(ml.id.0, "ml");
        assert_eq!(ml.script, "mlym");
        assert_eq!(ml.lexicon, Some(dir.path().join("ml.fst")));
        assert_eq!(ml.romanised, Some(dir.path().join("manglish.fst")));
        assert_eq!(ml.romanisation, None);
        assert_eq!(ml.next_words, Some(dir.path().join("next.fst")));
        assert_eq!(ml.translit, Some(dir.path().join("translit.mlkbnn")));
        assert_eq!(ml.lm, Some(dir.path().join("lm.mlkbnn")));
    }

    /// A pack in `script` with `files` (role, file name, contents); its lexicon is "x". A pack
    /// in mlym has Malayalam's ranking choices (`EVERY_CHOICE`), any other none.
    fn pack_of(script: &str, files: &[(&str, &str, &str)]) -> (tempfile::TempDir, Pack) {
        let ranking = if script == "mlym" { EVERY_CHOICE } else { "{}" };
        pack_ranked(script, files, ranking)
    }

    /// `pack_of` with `ranking` (JSON) as the manifest's `ranking`.
    fn pack_ranked(script: &str, files: &[(&str, &str, &str)], ranking: &str) -> (tempfile::TempDir, Pack) {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("x.fst"), b"x").unwrap();
        let mut roles = vec![r#""lexicon": "x.fst""#.to_string()];
        for (role, name, text) in files {
            fs::write(dir.path().join(name), text).unwrap();
            roles.push(format!(r#""{role}": "{name}""#));
        }
        let manifest = format!(
            r#"{{"format": 1, "id": "xx", "version": "1", "name": "X", "nativeName": "X",
                "script": "{script}", "minEngine": 1, "files": {{{}}}, "ranking": {ranking}}}"#,
            roles.join(", ")
        );
        fs::write(dir.path().join("manifest.json"), manifest).unwrap();
        let pack = Pack::open(dir.path()).unwrap();
        (dir, pack)
    }

    fn level(script: &str, files: &[(&str, &str, &str)]) -> u32 {
        pack_of(script, files).1.needed_engine().unwrap()
    }

    /// The engine's own copy of a script's lang.json (data/langs/<id>/lang.json).
    fn own_lang(id: &str) -> String {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/langs");
        fs::read_to_string(root.join(id).join("lang.json")).unwrap()
    }

    /// `lang` with `field` set to `value` (JSON).
    fn with_field(lang: &str, field: &str, value: &str) -> String {
        let mut lang: serde_json::Value = serde_json::from_str(lang).unwrap();
        lang[field] = serde_json::from_str(value).unwrap();
        lang.to_string()
    }

    const GUJR: &str = r#"{"script": "gujr", "block": ["0A80", "0AFF"], "virama": "0ACD"}"#;

    /// A romanisation.json's walk tables (what every engine from 2 reads), plus keys of its
    /// top level, of `skeleton` and of `skeleton.dropTrailing`, each starting with a comma.
    fn tables(top: &str, skeleton: &str, trailing: &str) -> String {
        format!(
            r#"{{"format": 1, "script": "deva", "virama": "094D", "inherent": "a",
                "atWordEnd": {{"virama": "", "inherent": ""}}, "independentVowels": {{}},
                "vowelSigns": {{}}, "consonants": {{}}, "finals": {{}}, "clusters": {{}},
                "skeleton": {{"dropTrailing": {{"letters": "a", "afterVowel": false,
                  "minLength": 3{trailing}}}, "pairs": [], "singles": {{}}, "collapseDoubles": true,
                  "dropFinalVowel": {{"vowels": "", "afterConsonant": true, "minLength": 3}}{skeleton}}}
                {top}}}"#
        )
    }

    #[test]
    fn needs_level_1_for_malayalam_whatever_its_roles() {
        let dir = pack_dir(FULL, &["ml.fst", "manglish.fst", "next.fst", "translit.mlkbnn", "lm.mlkbnn"]);
        let pack = Pack::open(dir.path()).unwrap();
        assert_eq!(pack.needs().unwrap(), []);
        assert_eq!(pack.needed_engine().unwrap(), 1);
        // Its own lang.json and tables with Malayalam's habits: engine 1 has them in code.
        let ml = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/ml.neopack");
        let lang = own_lang("ml");
        let romanisation = fs::read_to_string(ml.join("romanisation.json")).unwrap();
        let files = [
            ("lang", "lang.json", lang.as_str()),
            ("romanisation", "r.json", romanisation.as_str()),
            ("suffixes", "s.tsv", "x"),
            ("chatForms", "c.tsv", "x"),
            ("layout", "l.json", "x"),
            ("emojiWords", "e.txt", "x"),
            ("emojiSearch", "f.txt", "x"),
        ];
        assert_eq!(level("mlym", &files), 1);
    }

    #[test]
    fn needs_level_2_for_another_script() {
        let (_dir, pack) = pack_of("deva", &[("chatForms", "c.tsv", "x"), ("layout", "l.json", "x")]);
        assert_eq!(pack.needed_engine().unwrap(), 2);
        assert_eq!(pack.needs().unwrap()[0].what, "script deva, not Malayalam's");
        assert_eq!(level("deva", &[("romanisation", "r.json", &tables("", "", ""))]), 2);
        assert_eq!(level("beng", &[("lang", "lang.json", &own_lang("as"))]), 2, "the engine's own copy");
    }

    #[test]
    fn needs_level_3_for_an_lm_in_another_script() {
        assert_eq!(level("deva", &[("lm", "lm.mlkbnn", "x")]), 3);
        assert_eq!(level("mlym", &[("lm", "lm.mlkbnn", "x")]), 1);
    }

    #[test]
    fn needs_level_4_for_a_script_without_a_copy_or_a_translit_in_another_script() {
        assert_eq!(level("gujr", &[("lang", "lang.json", GUJR)]), 4);
        assert_eq!(level("deva", &[("translit", "t.mlkbnn", "x")]), 4);
        assert_eq!(level("mlym", &[("translit", "t.mlkbnn", "x")]), 1);
    }

    #[test]
    fn needs_level_5_for_the_rules_fields() {
        for field in ["notLetter", "looseSigns", "finalPairs"] {
            let lang = with_field(GUJR, field, r#"[["0A81", "0A83"]]"#);
            assert_eq!(level("gujr", &[("lang", "lang.json", &lang)]), 5, "{field}");
        }
        let joins = r#"{"vowelSigns": [["0A85", ""]], "nasal": ["0A82", "0AAE"]}"#;
        let lang = with_field(GUJR, "suffixJoins", joins);
        assert_eq!(level("gujr", &[("lang", "lang.json", &lang)]), 5);
    }

    #[test]
    fn needs_level_6_for_joiners_and_another_scripts_tables_habits() {
        let lang = with_field(GUJR, "joiners", r#""afterVirama""#);
        assert_eq!(level("gujr", &[("lang", "lang.json", &lang)]), 6);
        let lang = with_field(GUJR, "zwjForms", "[]");
        assert_eq!(level("gujr", &[("lang", "lang.json", &lang)]), 6, "present, even empty");
        let tamil = crate::script::data::TAMIL;
        assert_eq!(level("taml", &[("lang", "lang.json", tamil)]), 6, "ம் as a nasal");
        for top in [r#", "looseSpelling": {}"#, r#", "swipe": {}"#, r#", "fromLatin": {}"#] {
            let r = tables(top, "", "");
            assert_eq!(level("deva", &[("romanisation", "r.json", &r)]), 6, "{top}");
            assert_eq!(level("mlym", &[("romanisation", "r.json", &r)]), 1, "Malayalam's: {top}");
        }
    }

    #[test]
    fn needs_level_7_for_hinglish_habits_in_any_script() {
        let cases = [
            (r#", "optionalFinals": ["0902"]"#, "", ""),
            ("", r#", "alsoBetweenConsonants": "u""#, ""),
            ("", r#", "alsoBesideConsonant": {"e": "h"}"#, ""),
            ("", "", r#", "alsoKept": true"#),
        ];
        for (top, skeleton, trailing) in cases {
            let r = tables(top, skeleton, trailing);
            let what = format!("{top}{skeleton}{trailing}");
            assert_eq!(level("deva", &[("romanisation", "r.json", &r)]), 7, "{what}");
            assert_eq!(level("mlym", &[("romanisation", "r.json", &r)]), 7, "{what}");
        }
        // Empty or false, they say nothing an older engine misses.
        let r = tables(
            r#", "optionalFinals": []"#,
            r#", "alsoBetweenConsonants": """#,
            r#", "alsoKept": false"#,
        );
        assert_eq!(level("deva", &[("romanisation", "r.json", &r)]), 2);
    }

    #[test]
    fn needs_level_7_for_a_languages_own_rules_in_a_script_the_engine_has() {
        let lang = with_field(&own_lang("hi"), "replace", r#"[["अा", "आ"]]"#);
        let (_dir, pack) = pack_of("deva", &[("lang", "lang.json", &lang)]);
        assert_eq!(pack.needed_engine().unwrap(), 7);
        assert!(pack.needs().unwrap()[0].what.contains("own copy of deva's rules"));
        // The builder's keys and notes aren't rules.
        let lang = with_field(&own_lang("hi"), "maxWords", "300000");
        assert_eq!(level("deva", &[("lang", "lang.json", &lang)]), 2);
        let lang = with_field(&own_lang("ml"), "notes", r#""another note""#);
        assert_eq!(level("mlym", &[("lang", "lang.json", &lang)]), 1, "the same rules");
    }

    #[test]
    fn needs_level_8_for_a_character_table() {
        let urdu = r#"{"script": "arab", "block": ["0600", "06FF"],
            "charTable": [["0621", "063A"], ["0641", "064A"], ["06A9", "06A9"], ["06D2", "06D2"]]}"#;
        let (_dir, pack) = pack_of("arab", &[("lang", "lang.json", urdu)]);
        assert_eq!(pack.needed_engine().unwrap(), 8);
        assert_eq!(pack.needs().unwrap()[0].what, "lang.json sets charTable");
        // In any script: an older engine packs from the block whatever rules it has.
        let lang = with_field(&own_lang("hi"), "charTable", r#"[["0900", "097F"]]"#);
        assert_eq!(level("deva", &[("lang", "lang.json", &lang)]), 8);
        let lang = with_field(&own_lang("ml"), "charTable", r#"[["0D00", "0D7F"]]"#);
        assert_eq!(level("mlym", &[("lang", "lang.json", &lang)]), 8);
    }

    /// A phrase-conversion pack whose manifest has `extra` and files `files` (role, name).
    fn conversion_pack(extra: &str, files: &[(&str, &str)]) -> (tempfile::TempDir, Result<Pack, PackError>) {
        let dir = tempfile::tempdir().unwrap();
        let mut roles = Vec::new();
        for (role, name) in files {
            let text = if *role == "kana" { r#"{"format": 1, "rules": [["a", "あ"]]}"# } else { "x" };
            fs::write(dir.path().join(name), text).unwrap();
            roles.push(format!(r#""{role}": "{name}""#));
        }
        let manifest = format!(
            r#"{{"format": 1, "id": "ja", "version": "1", "name": "Japanese", "nativeName": "日本語",
                "script": "jpan", "minEngine": 9, "files": {{{}}}{extra}}}"#,
            roles.join(", ")
        );
        fs::write(dir.path().join("manifest.json"), manifest).unwrap();
        let pack = Pack::open(dir.path());
        (dir, pack)
    }

    const CONVERSION_FILES: &[(&str, &str)] =
        &[("kana", "kana.json"), ("conversionLexicon", "c.dict"), ("connection", "c.bin")];

    #[test]
    fn opens_a_phrase_conversion_pack_without_a_lexicon_or_script_rules() {
        let (dir, pack) = conversion_pack(r#", "conversion": "phrase""#, CONVERSION_FILES);
        let pack = pack.unwrap();
        assert_eq!(pack.manifest.conversion.as_deref(), Some("phrase"));
        let paths = pack.paths();
        assert_eq!(paths.lexicon, None);
        let files = paths.conversion.unwrap();
        assert_eq!(files.kana, dir.path().join("kana.json"));
        assert_eq!(files.dictionary, dir.path().join("c.dict"));
        assert_eq!(files.connection, dir.path().join("c.bin"));
        // A pack of words has none.
        let words = pack_dir(FULL, &["ml.fst"]);
        assert_eq!(Pack::open(words.path()).unwrap().paths().conversion, None);
    }

    #[test]
    fn refuses_a_conversion_it_cannot_do() {
        let (_dir, pack) = conversion_pack(r#", "conversion": "word""#, CONVERSION_FILES);
        assert!(matches!(pack, Err(PackError::Conversion(_))), "{pack:?}");
        let (_dir, pack) = conversion_pack(r#", "conversion": "phrase""#, &CONVERSION_FILES[..2]);
        let err = pack.unwrap_err();
        assert_eq!(err.to_string(), "conversion: no connection file");
        // Without "conversion" it is a pack of words, and jpan has no rules.
        let (_dir, pack) = conversion_pack("", CONVERSION_FILES);
        assert!(matches!(pack, Err(PackError::Script(_))), "{pack:?}");
    }

    #[test]
    fn needs_level_9_for_phrase_conversion() {
        let mut files = CONVERSION_FILES.to_vec();
        files.push(("notices", "NOTICE.txt"));
        let (_dir, pack) = conversion_pack(r#", "conversion": "phrase""#, &files);
        let pack = pack.unwrap();
        let needs = pack.needs().unwrap();
        assert_eq!(needs.len(), 1, "no script needs for a converter: {needs:?}");
        assert_eq!(needs[0].level, 9);
        assert_eq!(pack.needed_engine().unwrap(), 9);
        // A kana.json key no level reads is refused.
        let (dir, pack) = conversion_pack(r#", "conversion": "phrase""#, CONVERSION_FILES);
        fs::write(dir.path().join("kana.json"), r#"{"format": 1, "rules": [], "newRule": 1}"#).unwrap();
        assert!(pack.unwrap().needs().unwrap_err().contains("newRule"));
        // A word pack with a conversion role needs 9 too: an older engine would ignore it.
        assert_eq!(level("deva", &[("kana", "kana.json", r#"{"format": 1, "rules": []}"#)]), 9);
    }

    #[test]
    fn the_direction_needs_no_level() {
        // Read by the apps, never the engine: an older app lays the bar out left to right
        // as before, so the pack needs no newer engine.
        let lang = with_field(GUJR, "direction", r#""rtl""#);
        assert_eq!(level("gujr", &[("lang", "lang.json", &lang)]), 4);
        let lang = with_field(&own_lang("hi"), "direction", r#""ltr""#);
        assert_eq!(level("deva", &[("lang", "lang.json", &lang)]), 2);
        let lang = with_field(&own_lang("ml"), "direction", r#""ltr""#);
        assert_eq!(level("mlym", &[("lang", "lang.json", &lang)]), 1);
    }

    #[test]
    fn needs_refuses_what_it_has_no_level_for() {
        let (_dir, pack) = pack_of("deva", &[("sounds", "s.bin", "x")]);
        assert!(pack.needs().unwrap_err().contains("role sounds"));
        let r = tables(r#", "newHabit": true"#, "", "");
        let (_dir, pack) = pack_of("deva", &[("romanisation", "r.json", &r)]);
        assert!(pack.needs().unwrap_err().contains("newHabit"));
        let r = tables("", r#", "newSkeletonRule": 1"#, "");
        let (_dir, pack) = pack_of("deva", &[("romanisation", "r.json", &r)]);
        assert!(pack.needs().unwrap_err().contains("skeleton.newSkeletonRule"));
        let lang = with_field(GUJR, "newRule", "1");
        let (_dir, pack) = pack_of("gujr", &[("lang", "lang.json", &lang)]);
        assert!(pack.needs().unwrap_err().contains("newRule"));
        let (_dir, pack) = pack_ranked("deva", &[], r#"{"newChoice": true}"#);
        assert!(pack.needs().unwrap_err().contains("ranking.newChoice"));
        // Unknown when the pack opens: ignored, as an older engine ignores the object.
        assert_eq!(pack.manifest.ranking, Ranking::default());
    }

    /// Engines before 10 made every ranking choice for script mlym and none for any other: a
    /// pack that wants the same needs nothing, any other choice needs 10.
    #[test]
    fn ranking_choices_need_10_where_older_engines_made_them_otherwise() {
        assert_eq!(pack_ranked("deva", &[], "{}").1.needed_engine().unwrap(), 2);
        assert_eq!(pack_ranked("mlym", &[], EVERY_CHOICE).1.needed_engine().unwrap(), 1);
        let (_dir, pack) = pack_ranked("deva", &[], r#"{"startBelowEnglish": true}"#);
        assert_eq!(
            pack.needs().unwrap()[0],
            Need { level: 10, what: "manifest.json sets ranking.startBelowEnglish".into() }
        );
        let (_dir, pack) = pack_ranked("deva", &[], r#"{"holdOwnSpelling": false}"#);
        assert_eq!(pack.needed_engine().unwrap(), 2, "false is as left out");
        let off = EVERY_CHOICE.replace(r#""keepLexiconBeam": true"#, r#""keepLexiconBeam": false"#);
        let (_dir, pack) = pack_ranked("mlym", &[], &off);
        let needs = pack.needs().unwrap();
        assert_eq!(needs.len(), 1, "{needs:?}");
        assert_eq!(needs[0].level, 10);
        assert!(needs[0].what.contains("ranking.keepLexiconBeam off"), "{needs:?}");
        assert_eq!(pack_ranked("mlym", &[], "{}").1.needed_engine().unwrap(), 10);
    }

    #[test]
    fn ranking_is_read_from_the_manifest_and_beside_a_file() {
        let (dir, pack) = pack_ranked("deva", &[], r#"{"startBelowEnglish": true, "keepLexiconBeam": true}"#);
        let want = Ranking { start_below_english: true, keep_lexicon_beam: true, ..Ranking::default() };
        assert_eq!(pack.manifest.ranking, want);
        assert_eq!(pack.paths().ranking, want);
        assert_eq!(Ranking::beside(&dir.path().join("x.fst")), want);
        assert_eq!(Ranking::beside(Path::new("/no/such/x.fst")), Ranking::default());
        assert_eq!(pack_of("deva", &[]).1.manifest.ranking, Ranking::default());
        let ml = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/ml.neopack");
        let ml = Pack::open(&ml).unwrap().manifest.ranking;
        assert!(ml.fields().iter().all(|(_, on)| *on), "Malayalam's: {ml:?}");
        assert_eq!(ml.start_prior(), crate::rank::CONTEXT_AT_START);
        assert_eq!(Ranking::default().start_prior(), crate::rank::CONTEXT_AT_START_PACK);
    }

    #[test]
    fn no_committed_pack_needs_more_than_this_engine() {
        let packs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs");
        for entry in fs::read_dir(packs).unwrap() {
            let path = entry.unwrap().path();
            if !path.is_dir() {
                continue; // README.md
            }
            let pack = Pack::open(&path).unwrap();
            assert!(pack.needed_engine().unwrap() <= PACK_ENGINE, "{}", pack.manifest.id);
        }
    }
}
