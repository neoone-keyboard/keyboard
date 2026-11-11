//! Script rules. Each script a pack is written in has one [`ScriptRules`], keyed by its ISO
//! 15924 code in lower case, and every script's rules are data: a pack's `lang` file
//! (`lang.json`) defines its script, and [`data::DataScript`] is the plug-in made from it.
//! Nothing here is written for one script.
//!
//! Packs for Malayalam, Hindi and Assamese were published before packs carried that file, so
//! the engine holds a copy of those three ([`EMBEDDED`]: the same `data/langs/<id>/lang.json`
//! their packs now carry), read through the same parser, for a pack in `mlym`, `deva` or `beng`
//! that has no `lang` file. A pack's own file always wins.
//!
//! Rules belong to a language, not to a script: two packs may be written in one script and
//! bring different files (Assamese folds a letter Bengali keeps), so each loaded pack holds
//! its own (`PackSources::rules`) and the engine reads a word by the rules of the language it
//! is a word of (`Engine::read`). What is kept here, for the code that has only text and no
//! engine, is the set of rule sets in use, several of one script among them:
//!
//! - which characters make a word, where a phrase ends, and which script a word is in
//!   (`text::script`, [`is_word_char`], [`ends_phrase`]) hold for any of them ([`all`]);
//! - [`rules`], [`normalize_word`] and `normalize::normalize` answer by the first in use for a
//!   script ([`firsts`]): right for a builder, which installs one language's;
//! - [`of_language`] gives a language's own rules by its pack id, for a caller that knows the
//!   language but has no engine (the keyboard's vowel-sign strip).
//!
//! A pack's rules are in use while the pack is loaded ([`in_use`], held by `PackSources`) or
//! for good ([`install`]). The engine's own copy for a script stands in only while no pack's
//! rules for that script are in use.

pub mod data;

use std::ops::RangeInclusive;
use std::path::Path;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Mutex, OnceLock};

use data::{DataScript, ScriptDef};

/// What the engine needs to know about one native script.
pub trait ScriptRules: Send + Sync + std::fmt::Debug {
    /// ISO 15924, lower case: "mlym".
    fn code(&self) -> &'static str;
    /// The span of the script's characters, first to last. The transliterator's vocabulary,
    /// the LM's native pieces and the romanised-lexicon packing use it.
    fn block(&self) -> RangeInclusive<char>;
    /// The script's characters, as ranges. Two scripts in use never share one.
    fn ranges(&self) -> Vec<RangeInclusive<char>> {
        vec![self.block()]
    }
    /// A letter or sign a word in this script may be made of (ZWJ and ZWNJ are handled by the
    /// caller). Narrower than `block`: digits and punctuation in the block are not letters.
    fn is_letter(&self, c: char) -> bool;
    /// The script's half of the normal form: `normalize::normalize` with the script's
    /// replacements around it.
    fn normalize(&self, word: &str) -> String;
    /// The consonant at the end of `context_before` that a vowel sign typed next would attach
    /// to, if any. The keyboard shows the vowel-sign strip when there is one.
    fn mark_base(&self, context_before: &str) -> Option<char>;
    /// Spellings of `word` with one medial vowel sign dropped (loose spelling inserts them).
    fn vowel_sign_drops(&self, _word: &str) -> Vec<String> {
        Vec::new()
    }
    /// The other spelling of a word whose ending is written two ways, if the script has one.
    fn other_final_u(&self, _word: &str) -> Option<String> {
        None
    }
    /// A stem and a pack suffix (`suffix::Suffixes`) joined the way `how` says, or None when
    /// they don't join. A script without joins never joins, so its packs are never split.
    fn join_suffix(&self, _stem: &str, _suffix: &str, _how: crate::suffix::Join) -> Option<String> {
        None
    }
    /// The script's sandhi between a stem and a suffix (`lang.json`'s `suffixJoins`), if it
    /// has any: `suffix` reads from it which letters a typed stem may lack or carry.
    fn joins(&self) -> Option<&data::Joins> {
        None
    }
    /// A char of the block that is part of a word when splitting text (`text::words`).
    /// `is_letter` unless the script says otherwise.
    fn is_word_char(&self, c: char) -> bool {
        self.is_letter(c)
    }
    /// Punctuation of the script that ends a phrase as a full stop does (the danda): next-word
    /// pairs never span it.
    fn ends_phrase(&self, _c: char) -> bool {
        false
    }
    /// Whether an apostrophe may stand inside a word (Assamese হ'ল), never first or last.
    fn inner_apostrophe(&self) -> bool {
        false
    }
    /// What the script does with ZWJ and ZWNJ in the normal form (`normalize::Joiners`); None
    /// for a script that drops them all.
    fn joiners(&self) -> Option<&crate::normalize::Joiners> {
        None
    }
}

/// The `lang.json` of the packs published without one, by script: Malayalam's, Hindi's and
/// Assamese's, as their lexicons were built. Malayalam first: `text::script` asks in this
/// order, and the blocks don't overlap.
///
/// The beng copy is Assamese's, with its র → ৰ fold, as the Assamese pack is the only beng
/// pack there is without the file. A Bengali pack brings its own.
///
/// `tests/script_rules_recorded.rs` holds what these rules gave when they were code. An edit
/// to one of these files changes how every such installed pack is read.
const EMBEDDED: [(&str, &str); 3] = [
    ("mlym", include_str!("../../../../data/langs/ml/lang.json")),
    ("deva", include_str!("../../../../data/langs/hi/lang.json")),
    ("beng", include_str!("../../../../data/langs/as/lang.json")),
];

/// The embedded files as plug-ins, parsed on first use.
fn embedded_rules() -> &'static [&'static dyn ScriptRules] {
    static RULES: OnceLock<Vec<&'static dyn ScriptRules>> = OnceLock::new();
    RULES.get_or_init(|| {
        EMBEDDED
            .iter()
            .map(|(code, json)| {
                let def = ScriptDef::from_json(json)
                    .unwrap_or_else(|e| panic!("the engine's lang.json for {code}: {e}"));
                assert_eq!(def.code, *code, "the engine's lang.json for {code}");
                DataScript::intern(def) as &'static dyn ScriptRules
            })
            .collect()
    })
}

/// The rule sets in use (`table`), once a pack has brought any; null means the embedded ones.
/// Each table is leaked and only ever replaced, so a reader never sees one freed. A new table
/// is made only when the rule sets in use change.
static TABLE: AtomicPtr<Vec<&'static dyn ScriptRules>> = AtomicPtr::new(std::ptr::null_mut());
/// The rule sets packs brought, each with how many users it has (`in_use`), in the order
/// they were first used. `TABLE` is made from it.
static USES: Mutex<Vec<(&'static dyn ScriptRules, usize)>> = Mutex::new(Vec::new());
/// Each language's rules by its pack id (`set_language`), the latest for an id.
static LANGUAGES: Mutex<Vec<(String, &'static dyn ScriptRules)>> = Mutex::new(Vec::new());

/// The rule sets in use: for each script the engine has a copy of, the ones packs brought for
/// it, or else that copy; then the ones packs brought for any other script. Several may share
/// a script code (two languages of one script); two of different codes never share a
/// character.
fn table() -> &'static [&'static dyn ScriptRules] {
    let p = TABLE.load(Ordering::Acquire);
    if p.is_null() {
        embedded_rules()
    } else {
        // SAFETY: every non-null pointer stored in TABLE comes from Box::into_raw and is never
        // freed.
        unsafe { &*p }
    }
}

/// Makes the table `uses` gives, unless it is the one in place.
fn publish(uses: &[(&'static dyn ScriptRules, usize)]) {
    let mut next: Vec<&'static dyn ScriptRules> = Vec::new();
    for own in embedded_rules() {
        let before = next.len();
        next.extend(uses.iter().map(|(r, _)| *r).filter(|r| r.code() == own.code()));
        if next.len() == before {
            next.push(*own);
        }
    }
    next.extend(uses.iter().map(|(r, _)| *r).filter(|r| embedded(r.code()).is_none()));
    let current = table();
    let same = current.len() == next.len()
        && current.iter().zip(&next).all(|(a, b)| std::ptr::addr_eq(*a, *b));
    if !same {
        TABLE.store(Box::into_raw(Box::new(next)), Ordering::Release);
    }
}

/// The rules for a script code: the first in use for it (`table`). With two languages of one
/// script in use this is one of them: a caller that knows the language asks `of_language`, or
/// the engine.
pub fn rules(code: &str) -> Option<&'static dyn ScriptRules> {
    table().iter().copied().find(|r| r.code() == code)
}

/// One rule set for each script in use: the first (`rules`).
pub fn firsts() -> Vec<&'static dyn ScriptRules> {
    let mut found: Vec<&'static dyn ScriptRules> = Vec::new();
    for r in table() {
        if !found.iter().any(|f| f.code() == r.code()) {
            found.push(*r);
        }
    }
    found
}

/// The rules of the language `id` (a pack's id), once a pack of that id was loaded
/// (`Sources::open`) or named (`set_language`). The latest for the id.
pub fn of_language(id: &str) -> Option<&'static dyn ScriptRules> {
    let languages = LANGUAGES.lock().unwrap_or_else(|e| e.into_inner());
    languages.iter().find(|(l, _)| l == id).map(|(_, r)| *r)
}

/// Makes `rules` the ones `of_language` gives for the language `id` from now on.
pub fn set_language(id: &str, rules: &'static dyn ScriptRules) {
    let mut languages = LANGUAGES.lock().unwrap_or_else(|e| e.into_inner());
    match languages.iter_mut().find(|(l, _)| l == id) {
        Some(slot) => slot.1 = rules,
        None => languages.push((id.to_string(), rules)),
    }
}

/// The engine's own copy of the rules for a script code, whatever is installed.
pub fn embedded(code: &str) -> Option<&'static dyn ScriptRules> {
    embedded_rules().iter().copied().find(|r| r.code() == code)
}

/// The scripts a pack may be in without a `lang` file. A pack in any other script brings its
/// own.
pub fn supported() -> Vec<&'static str> {
    EMBEDDED.iter().map(|(code, _)| *code).collect()
}

/// Every rule set in use (`table`): two of one script when two such languages are loaded.
pub fn all() -> impl Iterator<Item = &'static dyn ScriptRules> {
    table().iter().copied()
}

/// A rule set in use for as long as this is kept (`in_use`).
#[derive(Debug)]
pub struct InUse {
    rules: &'static dyn ScriptRules,
}

impl Drop for InUse {
    fn drop(&mut self) {
        let mut uses = USES.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(at) = uses.iter().position(|(r, _)| std::ptr::addr_eq(*r, self.rules)) {
            uses[at].1 -= 1;
            if uses[at].1 == 0 {
                uses.remove(at);
                publish(&uses);
            }
        }
    }
}

/// Puts `new` among the rule sets in use until what is returned is dropped, beside any other
/// in use for its script (another language's). Refused when its characters overlap another
/// script's, as a word could then be in either.
pub fn in_use(new: &'static dyn ScriptRules) -> Result<InUse, String> {
    let mut uses = USES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(other) = table()
        .iter()
        .find(|r| r.code() != new.code() && data::overlaps(**r, new))
    {
        return Err(format!(
            "script {}'s characters overlap script {}'s",
            new.code(),
            other.code()
        ));
    }
    match uses.iter_mut().find(|(r, _)| std::ptr::addr_eq(*r, new)) {
        Some(found) => found.1 += 1,
        None => {
            uses.push((new, 1));
            publish(&uses);
        }
    }
    Ok(InUse { rules: new })
}

/// `in_use`, for as long as the process runs: a builder's one language, or a pack the
/// keyboard names before the engine opens.
pub fn install(new: &'static dyn ScriptRules) -> Result<(), String> {
    in_use(new).map(std::mem::forget)
}

/// The engine's normal form (`normalize::normalize_with`) of `text` with `own`'s joiner rules
/// for its script, whichever rule sets are in use for it, and the first in use for every
/// other script.
pub fn plain(text: &str, own: &dyn ScriptRules) -> String {
    plain_with(text, own.code(), own.joiners())
}

/// As `plain`, for rules given by their code and joiner rules.
pub(crate) fn plain_with(
    text: &str,
    code: &str,
    joiners: Option<&crate::normalize::Joiners>,
) -> String {
    let others = firsts();
    let rules: Vec<&crate::normalize::Joiners> = others
        .iter()
        .filter(|r| r.code() != code)
        .filter_map(|r| r.joiners())
        .chain(joiners)
        .collect();
    crate::normalize::normalize_with(text, &rules)
}

/// The rules for a pack in `script`: the script its `lang` file defines, or, for a pack
/// without one, the engine's own copy for that script (`EMBEDDED`).
///
/// An error, in words, when the file is bad, names another script, or there is neither.
/// Nothing is installed.
pub fn for_pack(script: &str, lang: Option<&Path>) -> Result<&'static dyn ScriptRules, String> {
    let Some(path) = lang else {
        return embedded(script)
            .ok_or_else(|| format!("no rules for script {script} (the pack has no lang file)"));
    };
    let mut def = ScriptDef::open(path)?;
    // A file written before `joiners` and `zwjForms` says neither: it is read as it was then,
    // when the engine's own rules for the script decided (Malayalam's chillus and joiners).
    if !def.says_joiners
        && let Some(own) = embedded(&def.code)
    {
        def.joiners = own.joiners().cloned();
    }
    if def.code != script {
        return Err(format!(
            "the lang file defines script {}, the manifest says {script}",
            def.code
        ));
    }
    let rules = DataScript::intern(def);
    // Checked here as well as at install, so a pack is refused when it is opened.
    if let Some(other) = embedded_rules()
        .iter()
        .find(|r| r.code() != script && data::overlaps(**r, rules))
    {
        return Err(format!(
            "script {script}'s characters overlap script {}'s",
            other.code()
        ));
    }
    Ok(rules)
}

/// True if `c` is part of a word in any script in use (`ScriptRules::is_word_char`).
pub fn is_word_char(c: char) -> bool {
    table().iter().any(|r| r.is_word_char(c))
}

/// True if `c` ends a phrase in any script in use (`ScriptRules::ends_phrase`).
pub fn ends_phrase(c: char) -> bool {
    table().iter().any(|r| r.ends_phrase(c))
}

/// A word's normal form without an engine: `normalize::normalize`, then, for a word of a
/// native script (`text::script`), the rules in use for that script (`rules`: the first, when
/// two languages of it are in use), which are what built its pack's lexicon. The engine reads
/// a word by its language's (`Engine::read`).
pub fn normalize_word(word: &str) -> String {
    let normal = crate::normalize::normalize(word);
    match crate::text::script(&normal) {
        crate::text::Script::Native(code) => match rules(code) {
            Some(r) => r.normalize(&normal),
            None => normal,
        },
        _ => normal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_are_registered() {
        assert_eq!(supported(), vec!["mlym", "deva", "beng"]);
        let ml = rules("mlym").unwrap();
        assert_eq!(ml.code(), "mlym");
        assert!(ml.block().contains(&'ക'));
        assert!(rules("deva").unwrap().block().contains(&'क'));
        assert!(rules("beng").unwrap().block().contains(&'ৰ'));
        assert!(rules("taml").is_none());
    }

    fn lang_file(json: &str) -> tempfile::NamedTempFile {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), json).unwrap();
        file
    }

    #[test]
    fn a_beng_pack_without_a_file_is_assamese_and_a_file_wins() {
        // G2: the র → ৰ fold is Assamese's (its lang.json's `replace`), not the script's. The
        // engine's copy for beng is that file, for the Assamese pack published without it.
        let old = for_pack("beng", None).unwrap();
        assert_eq!(old.normalize("তোমার"), "তোমাৰ");
        assert!(std::ptr::addr_eq(old, embedded("beng").unwrap()));
        // A Bengali pack brings its own rules and keeps its ra.
        let file = lang_file(r#"{"script": "beng", "block": ["0980", "09FF"]}"#);
        let bn = for_pack("beng", Some(file.path())).unwrap();
        assert_eq!(bn.normalize("তোমার"), "তোমার", "a pack's own rules");
        assert!(!std::ptr::addr_eq(bn, old));
    }

    #[test]
    fn a_packs_own_file_wins_for_every_script_and_unknown_scripts_need_one() {
        // Malayalam too: nothing about it is code.
        let file = lang_file(r#"{"script": "mlym", "block": ["0D00", "0D7F"]}"#);
        let bare = for_pack("mlym", Some(file.path())).unwrap();
        assert!(!std::ptr::addr_eq(bare, embedded("mlym").unwrap()));
        assert_eq!(bare.other_final_u("ഇന്ന്"), None, "the file names no final pairs");
        assert_eq!(
            embedded("mlym").unwrap().other_final_u("ഇന്ന്").as_deref(),
            Some("ഇന്നു")
        );
        let err = for_pack("taml", None).unwrap_err();
        assert!(err.contains("no lang file"), "{err}");
    }

    #[test]
    fn a_file_from_before_the_joiner_fields_keeps_the_scripts_own() {
        // The Malayalam pack of 2026.11.11 carries a lang.json without `joiners` or `zwjForms`.
        let old = lang_file(r#"{"script": "mlym", "block": ["0D00", "0D7F"], "virama": "0D4D"}"#);
        let rules = for_pack("mlym", Some(old.path())).unwrap();
        assert_eq!(rules.joiners(), embedded("mlym").unwrap().joiners());
        assert_eq!(rules.normalize("അവന\u{0D4D}\u{200D}"), "അവൻ");
        assert_eq!(rules.normalize("ഗ്\u{200D}ക"), "ഗ്\u{200D}ക");
        // A file that says so drops them, and has no chillus.
        let none = lang_file(
            r#"{"script": "mlym", "block": ["0D00", "0D7F"], "virama": "0D4D", "joiners": "drop"}"#,
        );
        let rules = for_pack("mlym", Some(none.path())).unwrap();
        assert_eq!(rules.joiners(), None);
        assert_eq!(rules.normalize("ഗ്\u{200D}ക"), "ഗ്ക");
        assert_eq!(rules.normalize("അവന\u{0D4D}\u{200D}"), "അവന\u{0D4D}");
        // A Devanagari file may ask for its joiners; the engine's own copy doesn't.
        let half = lang_file(
            r#"{"script": "deva", "block": ["0900", "097F"], "virama": "094D", "joiners": "afterVirama"}"#,
        );
        let rules = for_pack("deva", Some(half.path())).unwrap();
        assert_eq!(rules.normalize("क्\u{200D}ष"), "क्\u{200D}ष");
        assert_eq!(embedded("deva").unwrap().normalize("क्\u{200D}ष"), "क्ष");
    }

    #[test]
    fn a_pack_carrying_the_engines_file_gets_the_same_rules() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for (code, id) in [("mlym", "ml"), ("deva", "hi"), ("beng", "as")] {
            let path = root.join(format!("data/langs/{id}/lang.json"));
            let own = for_pack(code, Some(&path)).unwrap();
            assert!(std::ptr::addr_eq(own, embedded(code).unwrap()), "{code}");
        }
    }

    #[test]
    fn install_adds_a_script_and_refuses_an_overlap() {
        let file = lang_file(r#"{"script": "qzzz", "letters": [["F0000", "F00FF"]]}"#);
        let new = for_pack("qzzz", Some(file.path())).unwrap();
        install(new).unwrap();
        install(new).unwrap();
        assert!(std::ptr::addr_eq(rules("qzzz").unwrap(), new));
        assert_eq!(all().filter(|r| r.code() == "qzzz").count(), 1);
        assert_eq!(
            crate::text::script("\u{F0001}\u{F0002}"),
            crate::text::Script::Native("qzzz")
        );
        let clash = DataScript::intern(
            ScriptDef::from_json(r#"{"script": "qzzy", "letters": [["F0080", "F0180"]]}"#).unwrap(),
        );
        let err = install(clash).unwrap_err();
        assert!(err.contains("overlap script qzzz"), "{err}");
        assert!(rules("qzzy").is_none());
    }

    #[test]
    fn two_languages_of_one_script_are_both_in_use_while_kept() {
        // A private-use script, so no other test's rules are in the way.
        let first = format!(
            r#"{{"script": "qzzx", "letters": [["F1000", "F10FF"]], "replace": [["{}", "{}"]]}}"#,
            '\u{F1001}', '\u{F1002}'
        );
        let first = DataScript::intern(ScriptDef::from_json(&first).unwrap());
        let second = format!(
            r#"{{"script": "qzzx", "letters": [["F1000", "F107F"]], "phraseBreaks": ["{}"]}}"#,
            '\u{F1090}'
        );
        let second = DataScript::intern(ScriptDef::from_json(&second).unwrap());
        assert!(rules("qzzx").is_none());
        let a = in_use(first).unwrap();
        let b = in_use(second).unwrap();
        let again = in_use(second).unwrap();
        let both: Vec<_> = all().filter(|r| r.code() == "qzzx").collect();
        assert_eq!(both.len(), 2);
        assert!(std::ptr::addr_eq(rules("qzzx").unwrap(), first), "the first in use");
        assert_eq!(firsts().iter().filter(|r| r.code() == "qzzx").count(), 1);
        // A word is in the script if either language makes it of its letters, and a phrase
        // ends where either says.
        assert_eq!(
            crate::text::script("\u{F1081}"),
            crate::text::Script::Native("qzzx"),
            "a letter of the first alone"
        );
        assert!(ends_phrase('\u{F1090}'));
        // Each keeps its own normal form.
        assert_eq!(first.normalize("\u{F1001}"), "\u{F1002}");
        assert_eq!(second.normalize("\u{F1001}"), "\u{F1001}");
        drop(a);
        assert!(std::ptr::addr_eq(rules("qzzx").unwrap(), second));
        assert_eq!(crate::text::script("\u{F1081}"), crate::text::Script::Other);
        drop(b);
        assert!(rules("qzzx").is_some(), "still in use once more");
        drop(again);
        assert!(rules("qzzx").is_none());
    }

    #[test]
    fn a_languages_rules_are_found_by_its_id() {
        let file = lang_file(r#"{"script": "beng", "block": ["0980", "09FF"]}"#);
        let plain = for_pack("beng", Some(file.path())).unwrap();
        assert!(of_language("qq-test").is_none());
        set_language("qq-test", plain);
        assert!(std::ptr::addr_eq(of_language("qq-test").unwrap(), plain));
        set_language("qq-test", embedded("beng").unwrap());
        assert!(std::ptr::addr_eq(of_language("qq-test").unwrap(), embedded("beng").unwrap()));
    }

    #[test]
    fn blocks_do_not_overlap() {
        let scripts: Vec<_> = all().collect();
        for (i, a) in scripts.iter().enumerate() {
            // Two languages of one script share its block.
            for b in scripts[i + 1..].iter().filter(|b| b.code() != a.code()) {
                let overlap =
                    a.block().start() <= b.block().end() && b.block().start() <= a.block().end();
                assert!(!overlap, "{} and {}", a.code(), b.code());
            }
        }
    }

    #[test]
    fn malayalam_word_chars_are_its_whole_block() {
        // As before the other scripts came: text::words splits Malayalam text exactly as then.
        for c in '\u{0D00}'..='\u{0D7F}' {
            assert!(is_word_char(c), "{c:?}");
        }
        assert!(!is_word_char('।') && !is_word_char('০') && is_word_char('্'));
    }

    #[test]
    fn words_are_normalised_by_their_script() {
        assert_eq!(normalize_word("মােৰ"), "মোৰ");
        assert_eq!(normalize_word("\u{095B}रा"), "ज\u{093C}रा");
        assert_eq!(normalize_word("അവന\u{0D4D}\u{200D}"), "അവൻ");
        assert_eq!(normalize_word("don\u{2019}t"), "don't");
        assert_eq!(
            normalize_word("ab তোমার"),
            "ab তোমার",
            "not one word of the script"
        );
    }

    // What the coded plug-ins' own tests asserted (mlym.rs, malayalam.rs, deva.rs, beng.rs),
    // now of the embedded rules. tests/script_rules_recorded.rs has the whole blocks.

    #[test]
    fn malayalam_vowel_sign_bases() {
        let ml = embedded("mlym").unwrap();
        assert_eq!(ml.mark_base("ക"), Some('ക'));
        assert_eq!(ml.mark_base("അവ"), Some('വ'));
        // ന + ് + റ: the next vowel sign attaches to റ.
        assert_eq!(ml.mark_base("ന്റ"), Some('റ'));
        for none in ["കി", "ക്", "അവൻ", "ക ", "അ", "abc", ""] {
            assert_eq!(ml.mark_base(none), None, "{none:?}");
        }
        for (c, base) in [
            ('\u{0D15}', true),
            ('\u{0D3A}', true),
            ('\u{0D14}', false),
            ('\u{0D3B}', false),
            ('\u{0D7B}', false),
        ] {
            assert_eq!(ml.mark_base(&c.to_string()).is_some(), base, "{c:?}");
        }
    }

    #[test]
    fn malayalam_final_u_and_vowel_sign_drops() {
        let ml = embedded("mlym").unwrap();
        assert_eq!(ml.other_final_u("ഇന്ന്").as_deref(), Some("ഇന്നു"));
        assert_eq!(ml.other_final_u("അതു").as_deref(), Some("അത്"));
        assert_eq!(ml.other_final_u("വേണം"), None);
        assert_eq!(ml.other_final_u("ു"), None);
        assert_eq!(ml.vowel_sign_drops("ആവിശ്യം"), ["ആവശ്യം"]);
        assert_eq!(ml.vowel_sign_drops("കിളി"), ["കളി"], "the final ി is kept");
        assert!(ml.vowel_sign_drops("വന്നോ").is_empty());
        assert!(ml.vowel_sign_drops("അവൻ").is_empty());
        assert_eq!(ml.normalize("അവന\u{0D4D}\u{200D}"), "അവൻ");
    }

    #[test]
    fn malayalam_letters_and_word_chars() {
        let ml = embedded("mlym").unwrap();
        for c in ['ക', 'അ', 'ാ', '്', 'ം', 'ൻ', '\u{0D00}', '\u{0D63}'] {
            assert!(ml.is_letter(c), "{c:?}");
        }
        for c in ['൦', '൯', '\u{0D64}', '\u{0D79}', 'a', 'क'] {
            assert!(!ml.is_letter(c), "{c:?}");
        }
        assert!(ml.is_word_char('൦') && !ml.ends_phrase('।') && !ml.inner_apostrophe());
    }

    #[test]
    fn devanagari_normal_form_letters_and_bases() {
        let d = embedded("deva").unwrap();
        // ज़ (U+095B) is a composition exclusion: NFC gives ज + nukta.
        assert_eq!(d.normalize("\u{095B}िंदगी"), "\u{091C}\u{093C}िंदगी");
        assert_eq!(d.normalize("\u{095E}िल्म"), "\u{092B}\u{093C}िल्म");
        // ऩ ऱ ऴ compose and stay one code point.
        assert_eq!(d.normalize("\u{0928}\u{093C}"), "\u{0929}");
        assert_eq!(d.normalize("\u{0931}"), "\u{0931}");
        // A joiner after a virama only changes the drawing; the lexicon has none.
        assert_eq!(d.normalize("क्\u{200D}ष"), "क्ष");
        assert_eq!(d.normalize("क्\u{200C}ष"), "क्ष");
        assert_eq!(d.normalize("कह’"), "कह'");
        for c in ['।', '॥', '०', '९', '॰', 'a', 'ക'] {
            assert!(!d.is_letter(c), "{c}");
        }
        for c in ['क', 'अ', 'ा', '्', '़', 'ं', 'ँ', 'ः', 'ॐ', 'ऽ', '\u{0929}'] {
            assert!(d.is_letter(c), "{c}");
        }
        assert!(d.ends_phrase('।') && d.ends_phrase('॥') && !d.ends_phrase('.'));
        assert_eq!(d.mark_base("नमस"), Some('स'));
        assert_eq!(d.mark_base("ज\u{093C}"), Some('ज'), "consonant + nukta");
        assert_eq!(d.mark_base("\u{095B}"), Some('\u{095B}'));
        for none in ["कि", "क्", "अ", "कं", "\u{093C}", "abc", ""] {
            assert_eq!(d.mark_base(none), None, "{none:?}");
        }
        assert!(d.vowel_sign_drops("किताब").is_empty() && d.other_final_u("बात").is_none());
    }

    #[test]
    fn assamese_normal_form_letters_and_bases() {
        let b = embedded("beng").unwrap();
        for (written, normal) in [
            ("ৰাজ্য", "ৰাজ্য"),
            ("ৱিকি", "ৱিকি"),
            ("বাট", "বাট"),
            ("তোমার", "তোমাৰ"),
            ("প্রথম", "প্ৰথম"),
            // Khanda ta the old way, the halves of ো in visual order, অ + া.
            ("উত\u{09CD}\u{200D}সৱ", "উৎসৱ"),
            ("মােৰ", "মোৰ"),
            ("অাৰু", "আৰু"),
            ("ম\u{09C7}\u{09BE}ৰ", "মোৰ"),
            // The nukta letters are composition exclusions.
            ("\u{09DF}", "\u{09AF}\u{09BC}"),
            ("\u{09DC}", "\u{09A1}\u{09BC}"),
            ("\u{09DD}", "\u{09A2}\u{09BC}"),
            ("ক্\u{200C}ষ", "ক্ষ"),
            ("ক্\u{200D}ষ", "ক্ষ"),
            ("\u{200C}মই", "মই"),
            ("হ’ল", "হ'ল"),
        ] {
            assert_eq!(b.normalize(written), normal, "{written:?}");
        }
        for c in ['০', '৯', '৳', '৷', '৺', '\u{09FE}', '।'] {
            assert!(!b.is_letter(c), "{c}");
        }
        for c in ['ক', 'অ', 'া', '্', '়', 'ং', 'ঁ', 'ঃ', 'ৎ', 'ৰ', 'ৱ', '\u{09DC}'] {
            assert!(b.is_letter(c), "{c}");
        }
        assert!(b.ends_phrase('।') && b.ends_phrase('৷') && !b.ends_phrase(','));
        assert!(b.inner_apostrophe());
        assert_eq!(b.mark_base("মোৰ"), Some('ৰ'));
        assert_eq!(b.mark_base("ৱ"), Some('ৱ'));
        assert_eq!(b.mark_base("য\u{09BC}"), Some('য'), "consonant + nukta");
        for none in ["কি", "ক্", "উৎ", "অ", "হ'", ""] {
            assert_eq!(b.mark_base(none), None, "{none:?}");
        }
    }

    proptest::proptest! {
        #[test]
        fn a_malayalam_base_is_the_last_char_and_a_consonant(s in "\\PC{0,24}") {
            if let Some(c) = embedded("mlym").unwrap().mark_base(&s) {
                let consonant = ('\u{0D15}'..='\u{0D3A}').contains(&c);
                proptest::prop_assert!(consonant);
                proptest::prop_assert_eq!(s.chars().last(), Some(c));
            }
        }

        #[test]
        fn malayalam_text_never_panics(s in "[\u{0D00}-\u{0D7F}\u{200C}\u{200D} ]{0,24}") {
            let ml = embedded("mlym").unwrap();
            let _ = (ml.mark_base(&s), ml.vowel_sign_drops(&s), ml.other_final_u(&s));
        }
    }
}
