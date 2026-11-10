//! Script plug-ins (M6 phase 2). Each script a pack can be written in has one implementation of
//! [`ScriptRules`], keyed by its ISO 15924 code in lower case. Most scripts are data: a pack's
//! `lang` file (`lang.json`) defines its script, and [`data::DataScript`] is the plug-in made
//! from it (engine 4). The coded plug-ins (`mlym`, `deva`, `beng`) are for packs without that
//! file, and Malayalam's for what data can't say (its chillus and suffix joins).
//!
//! The engine serves one pack per script, so the rules in use for a script are global:
//! `Sources::open` installs each pack's rules ([`install`]) before it reads the pack's files,
//! and [`rules`], [`all`], [`normalize_word`] and `text::script` see them from then on. Before
//! any pack is opened they are the coded plug-ins.

mod beng;
pub mod data;
mod deva;
mod mlym;

use std::ops::RangeInclusive;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicPtr, Ordering};

use data::{DataScript, ScriptDef};

/// What the engine needs to know about one native script.
pub trait ScriptRules: Send + Sync + std::fmt::Debug {
    /// ISO 15924, lower case: "mlym".
    fn code(&self) -> &'static str;
    /// The span of the script's characters, first to last. The transliterator's vocabulary,
    /// the LM's native pieces and the romanised-lexicon packing use it.
    fn block(&self) -> RangeInclusive<char>;
    /// The script's characters, as ranges (the block, for a coded plug-in). Two scripts in use
    /// never share one.
    fn ranges(&self) -> Vec<RangeInclusive<char>> {
        vec![self.block()]
    }
    /// A letter or sign a word in this script may be made of (ZWJ and ZWNJ are handled by the
    /// caller). Narrower than `block`: digits and punctuation in the block are not letters.
    fn is_letter(&self, c: char) -> bool;
    /// The script's half of the normal form. The engine normalises every word with
    /// `normalize::normalize`, which today includes Malayalam's chillu rules.
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
    /// Whether this coded plug-in has behaviour a `lang` file can't express, so a pack's file
    /// doesn't replace it (Malayalam's chillus and joins).
    fn needs_code(&self) -> bool {
        false
    }
}

/// The coded plug-ins. Malayalam first: `text::script` asks in this order, and the blocks
/// don't overlap.
static CODED: &[&dyn ScriptRules] = &[&mlym::Mlym, &deva::Deva, &beng::BENG];

/// The rules in use, once a pack has installed any (`install`); null means `CODED`. Each
/// table is leaked and only ever replaced, so a reader never sees one freed. A new table is
/// made only when a pack brings rules other than those in use.
static TABLE: AtomicPtr<Vec<&'static dyn ScriptRules>> = AtomicPtr::new(std::ptr::null_mut());
static WRITER: Mutex<()> = Mutex::new(());

fn table() -> &'static [&'static dyn ScriptRules] {
    let p = TABLE.load(Ordering::Acquire);
    if p.is_null() {
        CODED
    } else {
        // SAFETY: every non-null pointer stored in TABLE comes from Box::into_raw and is never
        // freed.
        unsafe { &*p }
    }
}

/// The rules for a script code: the ones a pack installed, else the coded plug-in.
pub fn rules(code: &str) -> Option<&'static dyn ScriptRules> {
    table().iter().copied().find(|r| r.code() == code)
}

/// The coded plug-in for a script code, whatever is installed.
pub fn coded(code: &str) -> Option<&'static dyn ScriptRules> {
    CODED.iter().copied().find(|r| r.code() == code)
}

/// The scripts this engine has coded rules for. A pack in any other script brings its own
/// (its `lang` file, engine 4).
pub fn supported() -> Vec<&'static str> {
    CODED.iter().map(|r| r.code()).collect()
}

/// Every script in use.
pub fn all() -> impl Iterator<Item = &'static dyn ScriptRules> {
    table().iter().copied()
}

/// Makes `new` the rules for its script from now on, in place of the ones in use. Refused
/// when its characters overlap another script's, as a word could then be in either.
pub fn install(new: &'static dyn ScriptRules) -> Result<(), String> {
    let _writer = WRITER.lock().unwrap_or_else(|e| e.into_inner());
    let current = table();
    if current.iter().any(|r| std::ptr::addr_eq(*r, new)) {
        return Ok(());
    }
    if let Some(other) = current
        .iter()
        .find(|r| r.code() != new.code() && data::overlaps(**r, new))
    {
        return Err(format!(
            "script {}'s characters overlap script {}'s",
            new.code(),
            other.code()
        ));
    }
    let mut next: Vec<&'static dyn ScriptRules> = current.to_vec();
    match next.iter_mut().find(|r| r.code() == new.code()) {
        Some(slot) => *slot = new,
        None => next.push(new),
    }
    TABLE.store(Box::into_raw(Box::new(next)), Ordering::Release);
    Ok(())
}

/// The rules for a pack `id` in `script`, with its `lang` file if it has one:
///
/// - with the file, the script it defines (`data::DataScript`), unless the coded plug-in for
///   the script `needs_code` (Malayalam's), which is then used;
/// - without, the coded plug-in (Assamese's for an `as` pack, see `beng`).
///
/// An error, in words, when the file is bad, names another script, or there is neither.
/// Nothing is installed.
pub fn for_pack(id: &str, script: &str, lang: Option<&Path>) -> Result<&'static dyn ScriptRules, String> {
    let coded = coded(script);
    let Some(path) = lang else {
        if script == "beng" && id == "as" {
            return Ok(&beng::ASSAMESE);
        }
        return coded.ok_or_else(|| format!("no rules for script {script} (the pack has no lang file)"));
    };
    let def = ScriptDef::open(path)?;
    if def.code != script {
        return Err(format!(
            "the lang file defines script {}, the manifest says {script}",
            def.code
        ));
    }
    if let Some(c) = coded.filter(|c| c.needs_code()) {
        return Ok(c);
    }
    let rules = DataScript::intern(def);
    // Checked here as well as at install, so a pack is refused when it is opened.
    if let Some(other) = CODED
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

/// A word's normal form: `normalize::normalize`, then, for a word of a native script
/// (`text::script`), that script's own rules (`ScriptRules::normalize`), which are what built
/// its pack's lexicon. Malayalam's rules are `normalize` itself, so its words are unchanged.
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
    fn only_assamese_folds_the_bengali_ra() {
        // G2: the fold is Assamese's (its lang file's `replace`), not the script's.
        let bn = for_pack("bn", "beng", None).unwrap();
        assert_eq!(bn.normalize("তোমার"), "তোমার");
        let as_ = for_pack("as", "beng", None).unwrap();
        assert_eq!(as_.normalize("তোমার"), "তোমাৰ");
        let file = lang_file(r#"{"script": "beng", "block": ["0980", "09FF"], "replace": [["র", "ৰ"]]}"#);
        let data = for_pack("bn", "beng", Some(file.path())).unwrap();
        assert_eq!(data.normalize("তোমার"), "তোমাৰ", "a pack's own rules");
    }

    #[test]
    fn malayalam_keeps_its_code_and_unknown_scripts_need_a_file() {
        let file = lang_file(r#"{"script": "mlym", "block": ["0D00", "0D7F"]}"#);
        let ml = for_pack("ml", "mlym", Some(file.path())).unwrap();
        assert!(std::ptr::addr_eq(ml, coded("mlym").unwrap()));
        let err = for_pack("ta", "taml", None).unwrap_err();
        assert!(err.contains("no lang file"), "{err}");
    }

    #[test]
    fn install_adds_a_script_and_refuses_an_overlap() {
        let file = lang_file(r#"{"script": "qzzz", "letters": [["F0000", "F00FF"]]}"#);
        let new = for_pack("zz", "qzzz", Some(file.path())).unwrap();
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
    fn blocks_do_not_overlap() {
        let scripts: Vec<_> = all().collect();
        for (i, a) in scripts.iter().enumerate() {
            for b in &scripts[i + 1..] {
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

    #[test]
    fn malayalam_rules_match_the_old_functions() {
        let ml = rules("mlym").unwrap();
        for ctx in ["ക", "ന്റ", "കി", "ക്", "അവൻ", "abc", ""] {
            assert_eq!(
                ml.mark_base(ctx),
                crate::malayalam::vowel_sign_base(ctx),
                "{ctx:?}"
            );
        }
        for w in ["ഇന്ന്", "അതു", "വേണം", "ആവിശ്യം"] {
            assert_eq!(ml.other_final_u(w), crate::malayalam::other_final_u(w));
            assert_eq!(
                ml.vowel_sign_drops(w),
                crate::malayalam::vowel_sign_drops(w)
            );
            assert_eq!(ml.normalize(w), crate::normalize::normalize(w));
        }
    }
}
