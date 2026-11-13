//! The only door between platform code and the engine. Every exported call
//! goes through `guarded`, so a Rust panic becomes a typed `EngineError`
//! instead of killing the keyboard. UniFFI's own `catch_unwind` is the
//! backstop; `guarded` is what gives callers a typed error. Swift callers must
//! use `try` with do/catch, never `try!`.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum EngineError {
    #[error("internal engine failure: {message}")]
    Internal { message: String },
    /// A request the engine refused, such as an active list it can't use.
    #[error("refused: {message}")]
    Refused { message: String },
}

/// Which keyboard the text came from: QWERTY, or a language's native layout by its id
/// ("ml"). A language that isn't loaded is read as Latin.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum InputLayout {
    Latin,
    Native { language: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CandidateKind {
    Verbatim,
    Completion,
    Correction,
    Manglish,
    NextWord,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Candidate {
    pub text: String,
    pub kind: CandidateKind,
    /// The id of the language it is a word of ("en", "ml"): English, a pack for its words in
    /// either script, the layout's for the text as typed. Passed back to `note_choice` when
    /// the user taps it.
    pub language: String,
}

/// `c` for the keyboard, its language by id ("" for one no loaded pack has).
fn candidate(inner: &mlengine_core::Engine, c: mlengine_core::Candidate) -> Candidate {
    Candidate {
        language: inner.lang_id(c.lang).map(|id| id.0).unwrap_or_default(),
        text: c.text,
        kind: c.kind.into(),
    }
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Suggestions {
    /// The word being typed ("" after a space).
    pub typed: String,
    /// `candidates[0]` is the middle slot; when it isn't the typed word, `candidates[1]` is.
    pub candidates: Vec<Candidate>,
    /// Vowel signs, most likely first; empty unless the text ends in a Malayalam consonant.
    pub vowel_signs: Vec<String>,
}

/// A point of a swipe's path, in the keyboard view's points.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct SwipePoint {
    pub x: f32,
    pub y: f32,
}

/// A letter key as drawn, in the same points as the path.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SwipeKey {
    /// One ASCII letter.
    pub letter: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// The composing text of a phrase-conversion language (Japanese) as typed so far: the kana
/// written and the letters still waiting for more (`k` of `kyouk`). The keyboard shows both,
/// kana then letters, as marked text.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Composition {
    pub kana: String,
    pub pending: String,
}

/// One segment of a phrase conversion: a word and the particles or endings after it (今日は),
/// with its reading (きょうは).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PhraseSegment {
    pub reading: String,
    pub surface: String,
}

/// What converting the kana gives (`mlengine_core::convert::Conversion`).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PhraseConversion {
    /// The best conversion, segment by segment; their readings join to the kana converted.
    pub segments: Vec<PhraseSegment>,
    /// Candidates for the whole text, best first: the first is `segments` joined, and the
    /// text in hiragana and katakana are always among them.
    pub candidates: Vec<String>,
    /// Candidates for the first segment (`segments[0].reading`), best first, with its
    /// hiragana and katakana.
    pub first: Vec<String>,
}

impl From<mlengine_core::convert::Conversion> for PhraseConversion {
    fn from(c: mlengine_core::convert::Conversion) -> Self {
        Self {
            segments: c
                .segments
                .into_iter()
                .map(|s| PhraseSegment { reading: s.reading, surface: s.surface })
                .collect(),
            candidates: c.candidates,
            first: c.first,
        }
    }
}

/// File paths of the bundled lexicons and the directory for learned words.
/// Any of them may be nil: that source is simply switched off.
#[derive(Debug, Clone, Default, PartialEq, uniffi::Record)]
pub struct EngineConfig {
    #[uniffi(default = None)]
    pub english: Option<String>,
    /// The English base's own next words (`en-next.fst`), used while English is active and no
    /// active pack has a `next.fst`; nil, no next words for English alone.
    #[uniffi(default = None)]
    pub english_next: Option<String>,
    #[uniffi(default = None)]
    pub malayalam: Option<String>,
    #[uniffi(default = None)]
    pub manglish: Option<String>,
    #[uniffi(default = None)]
    pub next_words: Option<String>,
    /// The transliterator model (`.mlkbnn`); nil keeps Manglish on the lexicons alone.
    #[uniffi(default = None)]
    pub translit: Option<String>,
    /// The language model (`.mlkbnn`); nil, or one built against other lexicons, keeps
    /// ranking on the previous word and `next.fst`.
    #[uniffi(default = None)]
    pub lm: Option<String>,
    #[uniffi(default = None)]
    pub user_dir: Option<String>,
    /// Deprecated: one pack folder, read as one more entry of `pack_dirs` (after them, unless
    /// it is already listed).
    #[uniffi(default = None)]
    pub pack_dir: Option<String>,
    /// Language pack folders (`<id>.neopack`, M6), in order. Each that opens is loaded; each
    /// refused one is left out and `Engine::pack_problems` says why. When none opens (or none
    /// is given), `malayalam`, `manglish`, `next_words`, `translit` and `lm` describe one
    /// Malayalam pack instead.
    #[uniffi(default = [])]
    pub pack_dirs: Vec<String>,
    /// Milliseconds from the start of a suggest after which the transliterator stops and its
    /// finished spellings are used (`mlengine_core::nn::Deadline`); nil lets it run to the end.
    #[uniffi(default = None)]
    pub model_deadline_ms: Option<u32>,
    /// LM cutoffs in milliseconds from the start of a suggest (`mlengine_core::Timing`): the
    /// context prefill while a word is typed, everything the LM does after a space, and
    /// tier-1 rows. Nil lets that stage run to the end.
    #[uniffi(default = None)]
    pub lm_prefill_ms: Option<u32>,
    #[uniffi(default = None)]
    pub lm_after_space_ms: Option<u32>,
    #[uniffi(default = None)]
    pub lm_deadline_ms: Option<u32>,
    /// How long `prepare` may fill the LM's context; nil, until it is done.
    #[uniffi(default = None)]
    pub lm_prepare_ms: Option<u32>,
    /// The active languages' ids in the user's order ("en", "ml"), at most
    /// `mlengine_core::lang::MAX_ACTIVE`. Empty, or a list the engine refuses (a language
    /// that didn't load), means the engine's default: English and the first loaded pack.
    #[uniffi(default = [])]
    pub active: Vec<String>,
    /// How many candidates a suggest returns at most, 1 to `mlengine_core::MAX_SUGGESTIONS`
    /// (`mlengine_core::Engine::set_max_suggestions`); nil, `mlengine_core::DEFAULT_SUGGESTIONS`.
    /// More only adds candidates after the first three.
    #[uniffi(default = None)]
    pub max_suggestions: Option<u32>,
}

fn millis(ms: Option<u32>) -> Option<std::time::Duration> {
    ms.map(|ms| std::time::Duration::from_millis(ms.into()))
}

/// The pack folders `c` names: `pack_dirs`, then the deprecated `pack_dir` if it isn't one.
fn pack_dirs(c: &EngineConfig) -> Vec<String> {
    let mut dirs = c.pack_dirs.clone();
    if let Some(dir) = c.pack_dir.as_ref().filter(|d| !dirs.contains(d)) {
        dirs.push(dir.clone());
    }
    dirs
}

/// The engine's paths for `c`, and why each refused pack was refused ("<folder>: <reason>").
fn paths(c: &EngineConfig) -> (mlengine_core::Paths, Vec<String>) {
    let path = |s: &Option<String>| s.as_ref().map(std::path::PathBuf::from);
    let mut packs = Vec::new();
    let mut problems = Vec::new();
    for dir in pack_dirs(c) {
        let dir = std::path::PathBuf::from(dir);
        match mlengine_core::pack::Pack::open(&dir) {
            Ok(pack) => packs.push(pack),
            Err(e) => {
                let name = dir.file_name().map_or_else(
                    || dir.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                );
                problems.push(format!("{name}: {e}"));
            }
        }
    }
    if packs.is_empty() {
        return (c.into(), problems);
    }
    let mut paths = mlengine_core::Paths::with_packs(path(&c.english), &packs, path(&c.user_dir));
    paths.english_next = path(&c.english_next);
    (paths, problems)
}

impl From<&EngineConfig> for mlengine_core::Paths {
    fn from(c: &EngineConfig) -> Self {
        let path = |s: &Option<String>| s.as_ref().map(std::path::PathBuf::from);
        // Without a pack folder, the explicit fields describe one Malayalam pack. Its tables
        // sit beside the romanised lexicon, as they do in packs/ml.neopack.
        let beside = |name: &str| {
            path(&c.manglish)
                .map(|m| m.with_file_name(name))
                .filter(|p| p.exists())
        };
        let ml = mlengine_core::PackPaths {
            lexicon: path(&c.malayalam),
            romanised: path(&c.manglish),
            romanisation: beside("romanisation.json"),
            suffixes: beside("suffixes.tsv"),
            chat_forms: beside("chat-forms.tsv"),
            layout: beside("layout.json"),
            next_words: path(&c.next_words),
            translit: path(&c.translit),
            lm: path(&c.lm),
            ..mlengine_core::PackPaths::new("ml", "mlym")
        };
        Self {
            english: path(&c.english),
            english_next: path(&c.english_next),
            packs: vec![ml],
            user_dir: path(&c.user_dir),
        }
    }
}

impl From<mlengine_core::Kind> for CandidateKind {
    fn from(kind: mlengine_core::Kind) -> Self {
        use mlengine_core::Kind;
        match kind {
            Kind::Verbatim => Self::Verbatim,
            Kind::Completion => Self::Completion,
            Kind::Correction => Self::Correction,
            // Swift still calls a romanised word `manglish`.
            Kind::Romanised => Self::Manglish,
            Kind::NextWord => Self::NextWord,
        }
    }
}

#[derive(uniffi::Object)]
pub struct Engine {
    inner: std::sync::RwLock<mlengine_core::Engine>,
    model_deadline: Option<std::time::Duration>,
    lm_prefill: Option<std::time::Duration>,
    lm_after_space: Option<std::time::Duration>,
    lm_deadline: Option<std::time::Duration>,
    lm_prepare: Option<std::time::Duration>,
    pack_problems: Vec<String>,
}

/// The engine's layout for `layout`; a language that isn't loaded is Latin.
fn core_layout(engine: &mlengine_core::Engine, layout: &InputLayout) -> mlengine_core::Layout {
    match layout {
        InputLayout::Latin => mlengine_core::Layout::Latin,
        InputLayout::Native { language } => engine
            .lang_of_id(language)
            .map_or(mlengine_core::Layout::Latin, mlengine_core::Layout::Native),
    }
}

/// `ids` as the engine's active list; empty is its default list.
fn set_active(engine: &mut mlengine_core::Engine, ids: &[String]) -> Result<(), String> {
    if ids.is_empty() {
        engine.set_default_active();
        return Ok(());
    }
    let ids: Vec<mlengine_core::lang::LangId> = ids
        .iter()
        .map(|id| mlengine_core::lang::LangId(id.clone()))
        .collect();
    engine.set_active(&ids).map_err(|e| e.to_string())
}

impl Engine {
    /// The engine, read-locked: only `set_active_languages` writes. A poisoned lock still
    /// holds a usable engine.
    fn inner(&self) -> std::sync::RwLockReadGuard<'_, mlengine_core::Engine> {
        self.inner.read().unwrap_or_else(|p| p.into_inner())
    }
}

#[uniffi::export]
impl Engine {
    /// Maps the lexicon files and loads learned words. Never fails because a
    /// file is missing or corrupt: that source is just disabled.
    #[uniffi::constructor]
    pub fn new(config: EngineConfig) -> Result<Arc<Self>, EngineError> {
        guarded(|| {
            let (paths, mut pack_problems) = paths(&config);
            let mut inner = mlengine_core::Engine::open(&paths);
            // The manifests refused, then the packs that opened but couldn't be loaded.
            pack_problems.extend(inner.pack_problems());
            if set_active(&mut inner, &config.active).is_err() {
                inner.set_default_active();
            }
            if let Some(n) = config.max_suggestions {
                inner.set_max_suggestions(n as usize);
            }
            Arc::new(Self {
                inner: std::sync::RwLock::new(inner),
                pack_problems,
                model_deadline: millis(config.model_deadline_ms),
                lm_prefill: millis(config.lm_prefill_ms),
                lm_after_space: millis(config.lm_after_space_ms),
                lm_deadline: millis(config.lm_deadline_ms),
                lm_prepare: millis(config.lm_prepare_ms),
            })
        })
    }

    pub fn suggest(
        &self,
        context_before: String,
        layout: InputLayout,
        autocorrect: bool,
    ) -> Result<Suggestions, EngineError> {
        guarded(|| {
            let inner = self.inner();
            let layout = core_layout(&inner, &layout);
            let start = std::time::Instant::now();
            let elapsed = || start.elapsed();
            let timing = mlengine_core::Timing {
                elapsed: &elapsed,
                translit: self.model_deadline,
                lm_prefill: self.lm_prefill,
                lm_after_space: self.lm_after_space,
                lm_deadline: self.lm_deadline,
            };
            let s = inner.suggest_timed(&context_before, layout, autocorrect, Some(&timing));
            Suggestions {
                typed: s.typed,
                candidates: s
                    .candidates
                    .into_iter()
                    .map(|c| candidate(&inner, c))
                    .collect(),
                vowel_signs: s.vowel_signs.into_iter().map(String::from).collect(),
            }
        })
    }

    /// The words a swipe across the letter keys could have drawn, best first (design
    /// `2026-11-06-m4-swipe-design.md`): English words and Manglish words in Malayalam script.
    /// The LM's context is filled within `lm_after_space_ms`, as after a space.
    pub fn swipe(
        &self,
        path: Vec<SwipePoint>,
        keys: Vec<SwipeKey>,
        context_before: String,
    ) -> Result<Vec<Candidate>, EngineError> {
        guarded(|| {
            let path: Vec<(f32, f32)> = path.iter().map(|p| (p.x, p.y)).collect();
            let keys: Vec<mlengine_core::swipe::Key> = keys
                .iter()
                .filter_map(|k| {
                    let &[letter] = k.letter.as_bytes() else { return None };
                    letter.is_ascii_alphabetic().then(|| mlengine_core::swipe::Key {
                        letter: letter.to_ascii_lowercase(),
                        x: k.x,
                        y: k.y,
                        width: k.width,
                        height: k.height,
                    })
                })
                .collect();
            let inner = self.inner();
            inner
                .swipe(&path, &keys, &context_before, self.lm_after_space)
                .into_iter()
                .map(|c| candidate(&inner, c))
                .collect()
        })
    }

    /// Fills the LM's context for the text before the cursor, within `lm_prepare_ms`, so the
    /// first suggest in a field already has it. Call when the keyboard appears.
    pub fn prepare(&self, context_before: String) -> Result<(), EngineError> {
        guarded(|| self.inner().prepare(&context_before, self.lm_prepare))
    }

    /// The user undid an autocorrect or tapped what they typed: never replace this word.
    pub fn learn(&self, word: String) -> Result<(), EngineError> {
        guarded(|| self.inner().learn(&word))
    }

    /// A word was committed as typed; an unknown word is learned the second time.
    pub fn note_committed(&self, word: String) -> Result<(), EngineError> {
        guarded(|| self.inner().note_committed(&word))
    }

    /// A word was committed after `context_before` (typed, corrected or tapped), from
    /// `layout`: learns its use, the phrase it continues and the language habit. Never call
    /// it for a password field.
    pub fn note_used(
        &self,
        context_before: String,
        word: String,
        layout: InputLayout,
    ) -> Result<(), EngineError> {
        guarded(|| {
            let inner = self.inner();
            inner.note_used(&context_before, &word, core_layout(&inner, &layout))
        })
    }

    /// The user tapped `chosen` for `typed` though the bar's middle was something else.
    /// `language`: the tapped candidate's (`Candidate::language`); nil reads it from the word's
    /// script. A native word its pack's lexicon doesn't hold becomes one of the user's own
    /// words of that language. Call it before `note_used` for the same tap, so that the tap
    /// counts as the word's first use.
    pub fn note_choice(
        &self,
        typed: String,
        chosen: String,
        language: Option<String>,
    ) -> Result<(), EngineError> {
        let language = language.map(mlengine_core::lang::LangId);
        guarded(|| self.inner().note_choice(&typed, &chosen, language.as_ref()))
    }

    /// Forgets learned words and words removed from the bar.
    pub fn forget_learned(&self) -> Result<(), EngineError> {
        guarded(|| self.inner().forget_learned())
    }

    /// The user held a suggestion and chose Remove: never suggest it again until learned.
    pub fn remove_word(&self, word: String) -> Result<(), EngineError> {
        guarded(|| self.inner().remove_word(&word))
    }

    /// Every learned word, as shown, most recently used first: for the "Your words" screen.
    pub fn learned_words(&self) -> Result<Vec<String>, EngineError> {
        guarded(|| self.inner().learned_words())
    }

    /// The words removed from the bar, most recently removed first (ASCII lowercased).
    pub fn removed_words(&self) -> Result<Vec<String>, EngineError> {
        guarded(|| self.inner().removed_words())
    }

    /// Forgets one learned word without blocking it, unlike `remove_word`.
    pub fn forget_word(&self, word: String) -> Result<(), EngineError> {
        guarded(|| self.inner().forget_word(&word))
    }

    /// Un-removes a word removed from the bar; it is not learned by this.
    pub fn restore_word(&self, word: String) -> Result<(), EngineError> {
        guarded(|| self.inner().restore_word(&word))
    }

    /// The learned emails and URLs: the keyboard treats letters that begin one as one word.
    pub fn learned_addresses(&self) -> Result<Vec<String>, EngineError> {
        guarded(|| self.inner().learned_addresses())
    }

    /// Makes `languages` the active ones, in that order ("en", "ml"; at most
    /// `mlengine_core::lang::MAX_ACTIVE`): only they offer suggestions and layouts. Empty
    /// means the engine's default: English and the first loaded pack. Refused, keeping the
    /// list as it was, for too many, a repeat, or a language that isn't loaded.
    pub fn set_active_languages(&self, languages: Vec<String>) -> Result<(), EngineError> {
        let mut inner = self.inner.write().unwrap_or_else(|p| p.into_inner());
        guarded(|| set_active(&mut inner, &languages))?
            .map_err(|message| EngineError::Refused { message })
    }

    /// How many candidates a suggest returns at most, from 1 to
    /// `mlengine_core::MAX_SUGGESTIONS` (`EngineConfig::max_suggestions`). Cheap: nothing is
    /// reopened.
    pub fn set_max_suggestions(&self, n: u32) -> Result<(), EngineError> {
        let mut inner = self.inner.write().unwrap_or_else(|p| p.into_inner());
        guarded(|| inner.set_max_suggestions(n as usize))
    }

    /// How many candidates a suggest returns at most.
    pub fn max_suggestions(&self) -> Result<u32, EngineError> {
        guarded(|| u32::try_from(self.inner().max_suggestions()).unwrap_or(u32::MAX))
    }

    /// The active languages' ids, in order.
    pub fn active_languages(&self) -> Result<Vec<String>, EngineError> {
        guarded(|| {
            self.inner()
                .active_languages()
                .into_iter()
                .map(|id| id.0)
                .collect()
        })
    }

    /// The layouts the language key cycles through, in order: Latin first if an active
    /// language types in Latin, then each active pack's native layout.
    pub fn layouts(&self) -> Result<Vec<InputLayout>, EngineError> {
        guarded(|| {
            let inner = self.inner();
            inner
                .layouts()
                .into_iter()
                .filter_map(|layout| match layout {
                    mlengine_core::Layout::Latin => Some(InputLayout::Latin),
                    mlengine_core::Layout::Native(lang) => inner
                        .lang_id(lang)
                        .map(|id| InputLayout::Native { language: id.0 }),
                })
                .collect()
        })
    }

    /// The ids of the loaded phrase-conversion packs (Japanese), in load order. They are no
    /// languages of `suggest` or `layouts`: the keyboard types them on the Latin keys and
    /// composes and converts with `compose` and `convert`.
    pub fn conversion_languages(&self) -> Result<Vec<String>, EngineError> {
        guarded(|| self.inner().conversion_languages().into_iter().map(|id| id.0).collect())
    }

    /// The composing text of `typed`, the letters typed since the composition began, in the
    /// conversion language `language`. `finish` writes the letters still waiting too (`n` →
    /// ん), as before converting. Nil when `language` is no conversion language.
    pub fn compose(
        &self,
        language: String,
        typed: String,
        finish: bool,
    ) -> Result<Option<Composition>, EngineError> {
        guarded(|| {
            self.inner()
                .compose(&language, &typed, finish)
                .map(|(kana, pending)| Composition { kana, pending })
        })
    }

    /// Up to `limit` candidates for `kana` converted whole and for its first segment, and the
    /// best segments. Nil when `language` is no conversion language.
    pub fn convert(
        &self,
        language: String,
        kana: String,
        limit: u32,
    ) -> Result<Option<PhraseConversion>, EngineError> {
        guarded(|| {
            self.inner()
                .convert(&language, &kana, limit as usize)
                .map(PhraseConversion::from)
        })
    }

    /// `convert` with the first segment fixed to the first `first_length` characters
    /// (Unicode scalar values) of `kana`, as when the user shrinks or grows it: the first
    /// segment's candidates are for that reading, and the rest is converted after it. Nil when
    /// `language` is no conversion language or `first_length` isn't from 1 to the length of
    /// `kana`.
    pub fn convert_with_first(
        &self,
        language: String,
        kana: String,
        first_length: u32,
        limit: u32,
    ) -> Result<Option<PhraseConversion>, EngineError> {
        guarded(|| {
            let end = match first_length as usize {
                0 => return None,
                n => kana.char_indices().nth(n).map_or_else(
                    || (kana.chars().count() == n).then_some(kana.len()),
                    |(i, _)| Some(i),
                )?,
            };
            self.inner()
                .convert_with_first(&language, &kana, end, limit as usize)
                .map(PhraseConversion::from)
        })
    }

    /// Which sources loaded, for diagnostics ("english", "ml:lexicon", "ml:lm", ...).
    /// Why each refused pack folder was refused, one line per pack ("xx.neopack: manifest
    /// unreadable: ..."); empty when every pack opened or none was given.
    pub fn pack_problems(&self) -> Vec<String> {
        self.pack_problems.clone()
    }

    pub fn loaded_sources(&self) -> Result<Vec<String>, EngineError> {
        guarded(|| self.inner().loaded_sources())
    }
}

#[uniffi::export]
pub fn engine_version() -> Result<String, EngineError> {
    guarded(|| env!("CARGO_PKG_VERSION").to_string())
}

/// The transliterator's int8 product kernel on this CPU, "sdot" or "scalar": for diagnostics.
#[uniffi::export]
pub fn nn_kernel() -> String {
    mlengine_core::nn::ops::kernel().to_string()
}

/// What packs this engine reads, for the app's installer: the manifest `format`, the highest
/// `minEngine` (`PACK_ENGINE`) and the scripts it has rules for.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct PackSupport {
    pub format: u32,
    pub engine: u32,
    /// ISO 15924 codes a pack may be in without a `lang` file (the engine has a copy of
    /// their rules), lower case: "mlym", "deva", "beng".
    pub scripts: Vec<String>,
    /// A pack in any other script is read when it brings its script's rules (a `lang` file,
    /// engine 4).
    pub pack_scripts: bool,
}

#[uniffi::export]
pub fn pack_support() -> PackSupport {
    PackSupport {
        format: mlengine_core::pack::FORMAT,
        engine: mlengine_core::pack::PACK_ENGINE,
        scripts: mlengine_core::script::supported()
            .into_iter()
            .map(String::from)
            .collect(),
        pack_scripts: true,
    }
}

/// Opens the pack folder at `pack_dir` as the engine would (`Pack::open`: the manifest, the
/// lexicon, the script's rules) and says why it can't be used, or nil. The app's installer
/// asks before it puts a pack where the keyboard looks.
#[uniffi::export]
pub fn pack_problem(pack_dir: String) -> Result<Option<String>, EngineError> {
    guarded(|| {
        mlengine_core::pack::Pack::open(std::path::Path::new(&pack_dir))
            .err()
            .map(|e| e.to_string())
    })
}

/// Makes the pack at `pack_dir`'s rules the ones `language_mark_base` uses for its language
/// (the manifest's `id`), before the engine has opened (which does the same), and puts them
/// among the rules in use for its script. Returns the script code, or nil when the pack can't
/// be used.
#[uniffi::export]
pub fn use_pack_script(pack_dir: String) -> Result<Option<String>, EngineError> {
    guarded(|| {
        let pack = mlengine_core::pack::Pack::open(std::path::Path::new(&pack_dir)).ok()?;
        let m = &pack.manifest;
        let rules = mlengine_core::script::for_pack(&m.script, pack.file("lang").as_deref()).ok()?;
        mlengine_core::script::install(rules).ok()?;
        mlengine_core::script::set_language(&m.id, rules);
        Some(rules.code().to_string())
    })
}

/// The letter at the end of `context_before` that a vowel sign typed next would attach to, if
/// any (`ScriptRules::mark_base`), by the rules of the language `lang` (a pack manifest's
/// `id`, "ml"): the pack's own, as `use_pack_script` or an opened engine named them. Two
/// packs of one script may differ (a vowel takes a sign in one language and not in the
/// other), so the keyboard asks by language. Nil for a language no pack was named for. A
/// free function, not an `Engine` method: the keyboard asks on every key, on the main thread,
/// before and without the engine.
#[uniffi::export]
pub fn language_mark_base(lang: String, context_before: String) -> Result<Option<String>, EngineError> {
    guarded(|| {
        mlengine_core::script::of_language(&lang)
            .and_then(|rules| rules.mark_base(&context_before))
            .map(String::from)
    })
}

/// `language_mark_base` by script code (a pack manifest's `script`, "mlym"), for callers
/// from before it: the first rules in use for the script, which are one language's when two
/// packs of the script are loaded. Nil for a script this engine has no rules for.
#[uniffi::export]
pub fn native_mark_base(script: String, context_before: String) -> Result<Option<String>, EngineError> {
    guarded(|| {
        mlengine_core::script::rules(&script)
            .and_then(|rules| rules.mark_base(&context_before))
            .map(String::from)
    })
}

/// Timings from `NnBenchmark::run`, for the runtime spike's debug screen.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct NnBench {
    /// Mapping the file and resolving every tensor.
    pub load_ms: f64,
    /// Per word: one encode plus 10 decoder steps over 4 beams.
    pub p50_us: u64,
    pub p99_us: u64,
    /// Per word: the encode alone.
    pub encode_p50_us: u64,
}

/// A loaded `.mlkbnn` transliterator, timed the way a beam search would use it. Loading and
/// running are separate calls so the app can read its memory footprint in between.
#[derive(uniffi::Object)]
pub struct NnBenchmark {
    model: mlengine_core::nn::Transformer,
    load_ms: f64,
}

#[uniffi::export]
impl NnBenchmark {
    /// Maps `path` and resolves the model's tensors.
    #[uniffi::constructor]
    pub fn new(path: String) -> Result<Arc<Self>, EngineError> {
        guarded(|| {
            let start = std::time::Instant::now();
            let file = mlengine_core::nn::ModelFile::open(std::path::Path::new(&path))?;
            let model = mlengine_core::nn::Transformer::new(&file)?;
            Ok(Arc::new(Self {
                model,
                load_ms: start.elapsed().as_secs_f64() * 1e3,
            }))
        })?
    }

    /// Milliseconds the constructor took.
    pub fn load_ms(&self) -> f64 {
        self.load_ms
    }

    /// For each of `words` synthetic Latin words (4–12 letters): one encode, then 10 batched
    /// decoder steps over 4 beams, forking the beams by clone after every step.
    pub fn run(&self, words: u32) -> Result<NnBench, EngineError> {
        guarded(|| {
            let (mut total, mut encode) = bench_words(&self.model, words as usize);
            total.sort_unstable();
            encode.sort_unstable();
            NnBench {
                load_ms: self.load_ms,
                p50_us: percentile_us(&total, 0.5),
                p99_us: percentile_us(&total, 0.99),
                encode_p50_us: percentile_us(&encode, 0.5),
            }
        })
    }
}

/// Loads `path` and runs `NnBenchmark::run(words)` on it.
#[uniffi::export]
pub fn nn_benchmark(path: String, words: u32) -> Result<NnBench, EngineError> {
    NnBenchmark::new(path)?.run(words)
}

impl From<mlengine_core::nn::NnError> for EngineError {
    fn from(e: mlengine_core::nn::NnError) -> Self {
        Self::Internal {
            message: e.to_string(),
        }
    }
}

const BENCH_BEAMS: usize = 4;
const BENCH_STEPS: usize = 10;

/// (encode + steps, encode alone) per word.
fn bench_words(
    model: &mlengine_core::nn::Transformer,
    words: usize,
) -> (Vec<std::time::Duration>, Vec<std::time::Duration>) {
    use mlengine_core::nn::DecoderState;
    use std::time::Instant;
    let vocab = model.config().tgt_vocab;
    let argmax = |l: &[f32]| {
        (3..vocab)
            .max_by(|&a, &b| l[a].total_cmp(&l[b]))
            .unwrap_or(3) as u32
    };
    // A 64-bit LCG: Latin tokens are bos = 1, eos = 2, a..z = 3..28.
    let mut seed: u64 = 7;
    let mut next = move || {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        seed >> 33
    };
    let (mut total, mut encode) = (Vec::with_capacity(words), Vec::with_capacity(words));
    for _ in 0..words {
        let len = 4 + (next() % 9) as usize;
        let mut src = vec![1];
        src.extend((0..len).map(|_| 3 + (next() % 26) as u32));
        src.push(2);

        let start = Instant::now();
        let enc = model.encode(&src);
        encode.push(start.elapsed());
        let mut dec = model.decoder(&enc);
        let mut beams: Vec<DecoderState> = vec![dec.start(); BENCH_BEAMS];
        let mut tokens = vec![1; BENCH_BEAMS];
        for _ in 0..BENCH_STEPS {
            let logits = dec.step_batch(&mut beams, &tokens);
            for (b, token) in tokens.iter_mut().enumerate() {
                *token = argmax(&logits[b * vocab..(b + 1) * vocab]);
            }
            beams = (0..BENCH_BEAMS)
                .map(|b| beams[(b + 1) % BENCH_BEAMS].clone())
                .collect();
        }
        total.push(start.elapsed());
    }
    (total, encode)
}

fn percentile_us(sorted: &[std::time::Duration], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize].as_micros() as u64
}

fn guarded<T>(f: impl FnOnce() -> T) -> Result<T, EngineError> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|payload| {
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".to_string());
        EngineError::Internal { message }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guarded_passes_values_through() {
        assert_eq!(guarded(|| 7).unwrap(), 7);
    }

    #[test]
    fn guarded_turns_panics_into_errors() {
        let err = guarded(|| -> u8 { panic!("boom") }).unwrap_err();
        let EngineError::Internal { message } = err else {
            panic!("not an internal error")
        };
        assert!(message.contains("boom"), "message was {message:?}");
    }

    #[test]
    fn guarded_reports_formatted_string_payloads() {
        let err = guarded(|| -> u8 { panic!("x {}", 1) }).unwrap_err();
        let EngineError::Internal { message } = err else {
            panic!("not an internal error")
        };
        assert!(message.contains("x 1"), "message was {message:?}");
    }

    #[test]
    fn guarded_reports_unknown_payloads() {
        let err = guarded(|| -> u8 { std::panic::panic_any(5_u8) }).unwrap_err();
        let EngineError::Internal { message } = err else {
            panic!("not an internal error")
        };
        assert_eq!(message, "unknown panic");
    }

    fn japanese() -> Arc<Engine> {
        let ja = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/ja.neopack");
        Engine::new(EngineConfig { pack_dirs: vec![ja.into()], ..Default::default() }).unwrap()
    }

    #[test]
    fn phrase_conversion_crosses_ffi() {
        let engine = japanese();
        assert_eq!(engine.conversion_languages().unwrap(), ["ja"]);
        assert_eq!(
            engine.compose("ja".into(), "kyouk".into(), false).unwrap(),
            Some(Composition { kana: "きょう".into(), pending: "k".into() })
        );
        let kana = engine.compose("ja".into(), "kyouhaiitenkidesune".into(), true).unwrap().unwrap().kana;
        assert_eq!(kana, "きょうはいいてんきですね");
        let c = engine.convert("ja".into(), kana.clone(), 10).unwrap().unwrap();
        assert_eq!(c.candidates[0], "今日はいい天気ですね");
        assert!(c.candidates.contains(&"キョウハイイテンキデスネ".to_string()), "{:?}", c.candidates);
        assert_eq!(c.segments.iter().map(|s| s.reading.as_str()).collect::<String>(), kana);
        assert_eq!(c.segments[0].reading, "きょうは");
        assert_eq!(c.first[0], c.segments[0].surface);
        // The first segment as three characters (きょう), counted in characters, not bytes.
        let moved = engine.convert_with_first("ja".into(), kana.clone(), 3, 10).unwrap().unwrap();
        assert_eq!(moved.segments[0].reading, "きょう");
        assert_eq!(moved.first[0], "今日");
        assert_eq!(engine.convert_with_first("ja".into(), kana.clone(), 0, 10).unwrap(), None);
        assert_eq!(engine.convert_with_first("ja".into(), kana.clone(), 99, 10).unwrap(), None);
        // Not a conversion language.
        assert_eq!(engine.compose("en".into(), "a".into(), true).unwrap(), None);
        assert_eq!(engine.convert("xx".into(), "あ".into(), 5).unwrap(), None);
        // The word engine never sees it.
        assert!(engine.layouts().unwrap().iter().all(|l| l == &InputLayout::Latin));
    }

    #[test]
    fn engine_version_is_reported() {
        assert_eq!(engine_version().unwrap(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn engine_without_lexicons_suggests_the_typed_word() {
        let engine = Engine::new(EngineConfig::default()).unwrap();
        let s = engine.suggest("hello wor".into(), InputLayout::Latin, true).unwrap();
        assert_eq!(s.typed, "wor");
        assert_eq!(s.candidates, vec![Candidate { text: "wor".into(), kind: CandidateKind::Verbatim, language: "en".into() }]);
        assert!(engine.loaded_sources().unwrap().is_empty());
    }

    #[test]
    fn the_lm_and_its_cutoffs_cross_ffi() {
        let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/fixtures");
        let config = EngineConfig {
            lm: Some(format!("{fixtures}/lm-tiny.mlkbnn")),
            model_deadline_ms: Some(8),
            lm_prefill_ms: Some(3),
            lm_after_space_ms: Some(10),
            lm_deadline_ms: Some(11),
            lm_prepare_ms: Some(6),
            ..EngineConfig::default()
        };
        let engine = Engine::new(config).unwrap();
        // No lexicons: the LM was built against others, so it is not loaded.
        assert!(!engine.loaded_sources().unwrap().contains(&"ml:lm".to_string()));
        engine.prepare("hello there ".into()).unwrap();
        let s = engine.suggest("hello th".into(), InputLayout::Latin, true).unwrap();
        assert_eq!(s.typed, "th");
    }

    #[test]
    fn a_pack_folder_fills_the_malayalam_sources() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        let config = EngineConfig {
            english: Some(format!("{root}/apps/ios/KeyboardExtension/Lexicon/en.fst")),
            pack_dir: Some(format!("{root}/packs/ml.neopack")),
            ..EngineConfig::default()
        };
        let engine = Engine::new(config).unwrap();
        assert!(engine.pack_problems().is_empty());
        assert_eq!(
            engine.loaded_sources().unwrap(),
            ["english", "ml:lexicon", "ml:romanised", "ml:next_words", "ml:translit", "ml:lm"]
        );
        let ml = InputLayout::Native { language: "ml".into() };
        assert_eq!(engine.layouts().unwrap(), [InputLayout::Latin, ml.clone()]);
        let native = engine.suggest("എന്".into(), ml, true).unwrap();
        assert_eq!(native.candidates[0].text, "എന്");
        // A language that isn't loaded is typed on Latin.
        let unknown = InputLayout::Native { language: "xx".into() };
        let latin = engine.suggest("ente".into(), InputLayout::Latin, true).unwrap();
        assert_eq!(engine.suggest("ente".into(), unknown, true).unwrap(), latin);

        // The active list: refused when too long, empty is the default.
        let five = ["en", "ml", "en", "ml", "en"].map(String::from).to_vec();
        let refused = engine.set_active_languages(five);
        assert!(
            matches!(refused, Err(EngineError::Refused { .. })),
            "{refused:?}"
        );
        engine.set_active_languages(vec!["ml".into()]).unwrap();
        assert_eq!(engine.active_languages().unwrap(), ["ml"]);
        let s = engine
            .suggest("hello wor".into(), InputLayout::Latin, true)
            .unwrap();
        assert_eq!(s.candidates[0].text, "wor");
        engine.set_active_languages(vec![]).unwrap();
        assert_eq!(engine.active_languages().unwrap(), ["en", "ml"]);
        // In the config, as at start; a list that can't be used is the default.
        let config = |active: &[&str]| EngineConfig {
            english: Some(format!("{root}/apps/ios/KeyboardExtension/Lexicon/en.fst")),
            pack_dir: Some(format!("{root}/packs/ml.neopack")),
            active: active.iter().map(|id| id.to_string()).collect(),
            ..EngineConfig::default()
        };
        let engine = Engine::new(config(&["ml"])).unwrap();
        assert_eq!(engine.active_languages().unwrap(), ["ml"]);
        let engine = Engine::new(config(&["hi"])).unwrap();
        assert_eq!(engine.active_languages().unwrap(), ["en", "ml"]);
    }

    #[test]
    fn the_number_of_suggestions_crosses_ffi() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        let config = |max: Option<u32>| EngineConfig {
            english: Some(format!("{root}/apps/ios/KeyboardExtension/Lexicon/en.fst")),
            pack_dir: Some(format!("{root}/packs/ml.neopack")),
            max_suggestions: max,
            ..EngineConfig::default()
        };
        let three = Engine::new(config(None)).unwrap();
        assert_eq!(three.max_suggestions().unwrap(), 3);
        let ten = Engine::new(config(Some(10))).unwrap();
        assert_eq!(ten.max_suggestions().unwrap(), 10);
        let ask = |e: &Engine| e.suggest("hello th".into(), InputLayout::Latin, true).unwrap();
        let (short, long) = (ask(&three), ask(&ten));
        assert_eq!(short.candidates.len(), 3);
        assert!(long.candidates.len() > 3, "{long:?}");
        assert_eq!(short.candidates[..], long.candidates[..3]);
        // Set on a running engine; out of range is clamped.
        three.set_max_suggestions(10).unwrap();
        assert_eq!(ask(&three), long);
        three.set_max_suggestions(500).unwrap();
        assert_eq!(three.max_suggestions().unwrap(), 20);
        three.set_max_suggestions(0).unwrap();
        assert_eq!(three.max_suggestions().unwrap(), 1);
    }

    #[test]
    fn each_pack_folder_opens_or_says_why() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        let config = EngineConfig {
            english: Some(format!("{root}/apps/ios/KeyboardExtension/Lexicon/en.fst")),
            pack_dirs: vec![
                "/nonexistent/xx.neopack".into(),
                format!("{root}/packs/ml.neopack"),
            ],
            // The deprecated field, naming a folder already listed, is not opened twice.
            pack_dir: Some(format!("{root}/packs/ml.neopack")),
            ..EngineConfig::default()
        };
        let engine = Engine::new(config).unwrap();
        let problems = engine.pack_problems();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("xx.neopack: "), "{problems:?}");
        let ml = InputLayout::Native { language: "ml".into() };
        assert_eq!(engine.layouts().unwrap(), [InputLayout::Latin, ml]);
        assert!(engine.loaded_sources().unwrap().contains(&"ml:lexicon".to_string()));
    }

    /// A pack folder whose manifest is fine but whose lexicon isn't an FST, in a fresh
    /// temporary folder.
    fn broken_pack(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("mlengine-ffi-{name}-{}", std::process::id()))
            .join("xx.neopack");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("manifest.json"),
            r#"{"format": 1, "id": "xx", "version": "1", "name": "X", "nativeName": "X",
                "script": "mlym", "minEngine": 1, "files": {"lexicon": "x.fst"}}"#,
        )
        .unwrap();
        std::fs::write(dir.join("x.fst"), b"not an fst").unwrap();
        dir
    }

    #[test]
    fn pack_support_is_the_engines() {
        let support = pack_support();
        assert_eq!(support.format, 1);
        assert_eq!(support.engine, mlengine_core::pack::PACK_ENGINE);
        assert_eq!(support.scripts, ["mlym", "deva", "beng"]);
    }

    #[test]
    fn a_missing_and_a_broken_pack_leave_malayalam_working() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        let broken = broken_pack("beside");
        let config = EngineConfig {
            english: Some(format!("{root}/apps/ios/KeyboardExtension/Lexicon/en.fst")),
            pack_dirs: vec![
                "/nonexistent/gone.neopack".into(),
                broken.to_string_lossy().into_owned(),
                format!("{root}/packs/ml.neopack"),
            ],
            ..EngineConfig::default()
        };
        let engine = Engine::new(config).unwrap();
        let problems = engine.pack_problems();
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems[0].starts_with("gone.neopack: manifest unreadable"), "{problems:?}");
        assert!(problems[1].starts_with("xx.neopack: lexicon unreadable"), "{problems:?}");
        let s = engine.suggest("ente".into(), InputLayout::Latin, true).unwrap();
        assert_eq!(s.candidates[0].text, "എന്റെ");
        let _ = std::fs::remove_dir_all(broken.parent().unwrap());
    }

    #[test]
    fn a_broken_pack_alone_leaves_english_working() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        let broken = broken_pack("alone");
        let config = EngineConfig {
            english: Some(format!("{root}/apps/ios/KeyboardExtension/Lexicon/en.fst")),
            english_next: Some(format!("{root}/apps/ios/KeyboardExtension/Lexicon/en-next.fst")),
            pack_dirs: vec![broken.to_string_lossy().into_owned()],
            ..EngineConfig::default()
        };
        let engine = Engine::new(config).unwrap();
        assert_eq!(engine.pack_problems().len(), 1);
        assert_eq!(engine.loaded_sources().unwrap(), ["english", "english_next"]);
        let next = engine.suggest("I ".into(), InputLayout::Latin, true).unwrap();
        assert!(next.candidates.iter().any(|c| c.text == "am"), "{next:?}");
        assert_eq!(engine.layouts().unwrap(), [InputLayout::Latin]);
        assert_eq!(engine.active_languages().unwrap(), ["en"]);
        let s = engine.suggest("teh".into(), InputLayout::Latin, true).unwrap();
        assert_eq!(s.candidates[0].text, "the");
        let _ = std::fs::remove_dir_all(broken.parent().unwrap());
    }

    #[test]
    fn a_refused_pack_says_why_and_uses_the_explicit_paths() {
        let config = EngineConfig {
            pack_dir: Some("/nonexistent/xx.neopack".into()),
            ..EngineConfig::default()
        };
        let engine = Engine::new(config).unwrap();
        let problems = engine.pack_problems();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("xx.neopack: manifest unreadable"), "{problems:?}");
        assert!(engine.loaded_sources().unwrap().is_empty());
    }

    #[test]
    fn missing_files_disable_sources_instead_of_failing() {
        let config = EngineConfig { english: Some("/nonexistent/en.fst".into()), ..EngineConfig::default() };
        let engine = Engine::new(config).unwrap();
        assert!(engine.loaded_sources().unwrap().is_empty());
    }

    #[test]
    fn learning_crosses_ffi() {
        let dir = std::env::temp_dir().join(format!("mlengine-ffi-test-{}", std::process::id()));
        let config = EngineConfig { user_dir: Some(dir.to_string_lossy().into_owned()), ..EngineConfig::default() };
        let engine = Engine::new(config.clone()).unwrap();
        engine.learn("Thom".into()).unwrap();
        engine.note_committed("Anu".into()).unwrap();
        engine.learn("sam@example.com".into()).unwrap();
        assert_eq!(engine.learned_addresses().unwrap(), ["sam@example.com"]);
        engine.forget_learned().unwrap();
        assert!(engine.learned_addresses().unwrap().is_empty());
        drop(engine);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A tap on the transliterator's free spelling, as the keyboard reports it: the choice with
    /// the candidate's language, then the use. The word is the user's own from then on.
    #[test]
    fn a_picked_native_word_crosses_ffi_with_its_language_and_persists() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        let dir = std::env::temp_dir().join(format!("mlengine-ffi-own-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let config = EngineConfig {
            english: Some(format!("{root}/apps/ios/KeyboardExtension/Lexicon/en.fst")),
            pack_dirs: vec![format!("{root}/packs/hi.neopack")],
            user_dir: Some(dir.to_string_lossy().into_owned()),
            ..EngineConfig::default()
        };
        let bar = |engine: &Engine, context: &str| -> Vec<Candidate> {
            engine.suggest(context.into(), InputLayout::Latin, true).unwrap().candidates
        };
        let texts = |engine: &Engine, context: &str| -> Vec<String> {
            bar(engine, context).into_iter().map(|c| c.text).collect()
        };
        let engine = Engine::new(config.clone()).unwrap();
        let offered = bar(&engine, "raubdar");
        assert_eq!(offered[0].language, "en", "{offered:?}");
        let free = offered[2].clone();
        assert_eq!((free.text.as_str(), free.language.as_str()), ("रौबदार", "hi"), "{offered:?}");
        assert_eq!(free.kind, CandidateKind::Manglish);
        engine.note_choice("raubdar".into(), free.text.clone(), Some(free.language.clone())).unwrap();
        engine.note_used("".into(), free.text.clone(), InputLayout::Latin).unwrap();
        assert_eq!(engine.learned_words().unwrap(), [free.text.as_str()]);
        drop(engine);
        let engine = Engine::new(config).unwrap();
        assert_eq!(texts(&engine, "raubdar")[0], free.text);
        assert_eq!(texts(&engine, "raubdaar")[..2], ["raubdaar".to_string(), free.text.clone()]);
        assert!(texts(&engine, "raub").contains(&free.text));
        engine.forget_word(free.text.clone()).unwrap();
        assert!(engine.learned_words().unwrap().is_empty());
        assert!(!texts(&engine, "raub").contains(&free.text));
        drop(engine);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn phrases_and_choices_cross_ffi_and_persist() {
        let dir = std::env::temp_dir().join(format!("mlengine-ffi-phrase-test-{}", std::process::id()));
        let config = EngineConfig { user_dir: Some(dir.to_string_lossy().into_owned()), ..EngineConfig::default() };
        let texts = |engine: &Engine, context: &str| -> Vec<String> {
            let s = engine.suggest(context.into(), InputLayout::Latin, true).unwrap();
            s.candidates.into_iter().map(|c| c.text).collect()
        };
        let engine = Engine::new(config.clone()).unwrap();
        engine.learn("Thom".into()).unwrap();
        engine.learn("Anu".into()).unwrap();
        engine.note_used("".into(), "Thom".into(), InputLayout::Latin).unwrap();
        engine.note_used("Thom ".into(), "Anu".into(), InputLayout::Latin).unwrap();
        engine.note_choice("thmo".into(), "Thom".into(), None).unwrap();
        drop(engine);
        let engine = Engine::new(config).unwrap();
        assert_eq!(texts(&engine, "Thom "), ["Anu"]);
        assert_eq!(texts(&engine, "thmo")[0], "Thom");
        engine.forget_learned().unwrap();
        assert!(texts(&engine, "Thom ").is_empty());
        drop(engine);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn phone_numbers_cross_ffi_and_persist() {
        let dir = std::env::temp_dir().join(format!("mlengine-ffi-number-test-{}", std::process::id()));
        let config = EngineConfig { user_dir: Some(dir.to_string_lossy().into_owned()), ..EngineConfig::default() };
        let engine = Engine::new(config.clone()).unwrap();
        for _ in 0..2 {
            engine.note_used("call +91 55501 ".into(), "12345".into(), InputLayout::Latin).unwrap();
        }
        drop(engine);
        let engine = Engine::new(config).unwrap();
        assert_eq!(engine.learned_addresses().unwrap(), ["+91 55501 12345"]);
        let s = engine.suggest("call +91 55".into(), InputLayout::Latin, true).unwrap();
        assert_eq!(s.typed, "+91 55");
        assert_eq!(s.candidates[1].text, "+91 55501 12345");
        engine.remove_word("+91 55501 12345".into()).unwrap();
        assert!(engine.learned_addresses().unwrap().is_empty());
        assert_eq!(engine.removed_words().unwrap(), ["+91 55501 12345"]);
        drop(engine);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn removal_crosses_ffi_and_persists() {
        let dir = std::env::temp_dir().join(format!("mlengine-ffi-remove-test-{}", std::process::id()));
        let config = EngineConfig { user_dir: Some(dir.to_string_lossy().into_owned()), ..EngineConfig::default() };
        let texts = |engine: &Engine| -> Vec<String> {
            let s = engine.suggest("thoma".into(), InputLayout::Latin, true).unwrap();
            s.candidates.into_iter().map(|c| c.text).collect()
        };
        let engine = Engine::new(config.clone()).unwrap();
        engine.learn("Thomasina".into()).unwrap();
        assert!(texts(&engine).contains(&"Thomasina".to_string()));
        engine.remove_word("Thomasina".into()).unwrap();
        assert!(!texts(&engine).contains(&"Thomasina".to_string()));
        drop(engine);
        let engine = Engine::new(config).unwrap();
        assert!(!texts(&engine).contains(&"Thomasina".to_string()), "the removal was kept");
        drop(engine);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn your_words_cross_ffi_and_persist() {
        let dir = std::env::temp_dir().join(format!(
            "mlengine-ffi-your-words-test-{}",
            std::process::id()
        ));
        let config = EngineConfig {
            user_dir: Some(dir.to_string_lossy().into_owned()),
            ..EngineConfig::default()
        };
        let engine = Engine::new(config.clone()).unwrap();
        engine.learn("Thom".into()).unwrap();
        engine.learn("Anu".into()).unwrap();
        engine.remove_word("hello".into()).unwrap();
        engine.remove_word("world".into()).unwrap();
        assert_eq!(engine.learned_words().unwrap(), ["Anu", "Thom"]);
        assert_eq!(engine.removed_words().unwrap(), ["world", "hello"]);
        engine.forget_word("Thom".into()).unwrap();
        engine.restore_word("hello".into()).unwrap();
        drop(engine);
        let engine = Engine::new(config).unwrap();
        assert_eq!(
            engine.learned_words().unwrap(),
            ["Anu"],
            "the forget was kept"
        );
        assert_eq!(
            engine.removed_words().unwrap(),
            ["world"],
            "the restore was kept"
        );
        drop(engine);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn nn_benchmark_times_the_tiny_fixture() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../core/tests/fixtures/translit-tiny.mlkbnn"
        );
        let bench = nn_benchmark(path.into(), 5).unwrap();
        assert!(bench.load_ms >= 0.0);
        assert!(bench.p50_us <= bench.p99_us);
        assert!(bench.encode_p50_us <= bench.p99_us);
    }

    #[test]
    fn nn_benchmark_reports_a_missing_model_as_an_error() {
        assert!(nn_benchmark("/nonexistent/model.mlkbnn".into(), 1).is_err());
    }

    #[test]
    fn native_mark_base_crosses_ffi() {
        let base = |script: &str, text: &str| native_mark_base(script.into(), text.into()).unwrap();
        assert_eq!(base("mlym", "ന്റ"), Some("റ".to_string()));
        assert_eq!(base("mlym", "കി"), None);
        // A script without rules has no strip.
        assert_eq!(base("zzzz", "ക"), None);
    }

    #[test]
    fn language_mark_base_answers_by_each_packs_own_rules() {
        // Two packs of one script whose rules differ: a vowel is a base in one only.
        let dir =
            std::env::temp_dir().join(format!("mlengine-ffi-language-test-{}", std::process::id()));
        let pack = |id: &str, bases: &str| {
            let folder = dir.join(format!("{id}.neopack"));
            std::fs::create_dir_all(&folder).unwrap();
            let lang = format!(
                r#"{{"script": "beng", "block": ["0980", "09FF"], "bases": [{bases}]}}"#
            );
            std::fs::write(folder.join("lang.json"), lang).unwrap();
            let lexicon = mlengine_core::lexicon::build([("ক".as_bytes().to_vec(), 1)]).unwrap();
            std::fs::write(folder.join("x.fst"), lexicon).unwrap();
            let manifest = format!(
                r#"{{"format": 1, "id": "{id}", "version": "1", "name": "X", "nativeName": "X",
                    "script": "beng", "minEngine": 4,
                    "files": {{"lexicon": "x.fst", "lang": "lang.json"}}}}"#
            );
            std::fs::write(folder.join("manifest.json"), manifest).unwrap();
            folder.to_string_lossy().into_owned()
        };
        let first = pack("qa-ffi", r#"["0995", "09B9"]"#);
        let second = pack("qb-ffi", r#"["0985", "0985"], ["0995", "09B9"]"#);
        let base = |lang: &str, text: &str| language_mark_base(lang.into(), text.into()).unwrap();
        assert_eq!(base("qa-ffi", "ক"), None, "no pack named yet");
        // In either order, and again: each language keeps its own.
        for dir in [&first, &second, &first] {
            assert_eq!(use_pack_script(dir.clone()).unwrap().as_deref(), Some("beng"));
            assert_eq!(base("qa-ffi", "অ"), None);
            assert_eq!(base("qa-ffi", "ক"), Some("ক".to_string()));
        }
        assert_eq!(base("qb-ffi", "অ"), Some("অ".to_string()));
        assert_eq!(base("zz-ffi", "ক"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
