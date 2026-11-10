//! The script rules of Malayalam, Devanagari (Hindi) and Bengali-Assamese (Assamese), which
//! were Rust code (`script/mlym.rs`, `deva.rs`, `beng.rs`, `malayalam.rs`, the joins in
//! `suffix.rs`) and are now `data/langs/<id>/lang.json`, against what that code gave.
//!
//! `fixtures/script-rules/<code>.tsv` was recorded from the code before it was deleted
//! (`RECORD_SCRIPT_RULES=1`, then compared with the data rules line for line). It holds, for
//! one script:
//!
//! - every character of the block, and a few outside it: whether it is a letter, a word
//!   character and a phrase break, its normal form, and the vowel-sign base after it alone,
//!   after a consonant and after a consonant + virama;
//! - every pair of block characters (and the joiners) whose normal form isn't the two normal
//!   forms side by side, or whose vowel-sign base isn't the second character's;
//! - every character before virama + joiner sequences (the old chillu and khanda-ta
//!   encodings);
//! - real words: 200 spread over the pack's lexicon and 150 from its eval sets as people wrote
//!   them, each also in the encodings text arrives in (NFD, joiners after a virama, the old
//!   chillu sequences, precomposed nukta letters, the Bengali ra, visual-order vowel signs, a
//!   curly apostrophe), with the normal form, the other final-u spelling, the spellings with
//!   a vowel sign dropped and, for the first 60, the join with every suffix of Malayalam's
//!   table.
//!
//! `every_lexicon_word_is_as_recorded` does the word lines for every word of the three
//! lexicons and compares a hash of them with the one recorded the same way (`make test-data`).

use std::fmt::Write as _;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use mlengine_core::lexicon::Lexicon;
use mlengine_core::script::ScriptRules;
use mlengine_core::script::data::{DataScript, ScriptDef};
use mlengine_core::suffix::{Suffix, Suffixes};
use unicode_normalization::UnicodeNormalization;

const ZWJ: char = '\u{200D}';
const ZWNJ: char = '\u{200C}';

struct Case {
    code: &'static str,
    id: &'static str,
    lexicon: &'static str,
    virama: char,
    consonant: char,
    /// Text people wrote, not yet normalised: the native words of these files are inputs.
    written: &'static [&'static str],
    /// FNV-1a of every lexicon word's lines (`every_lexicon_word_is_as_recorded`).
    digest: u64,
}

const CASES: [Case; 3] = [
    Case {
        code: "mlym",
        id: "ml",
        lexicon: "ml.fst",
        virama: '\u{0D4D}',
        consonant: 'ക',
        written: &[
            "data/eval/translit-human.tsv",
            "data/eval/next-ml.heldout.txt",
        ],
        digest: 0xebf9_8f84_717d_d741,
    },
    Case {
        code: "deva",
        id: "hi",
        lexicon: "hi.fst",
        virama: '\u{094D}',
        consonant: 'क',
        written: &[
            "data/eval/hi/translit-human.tsv",
            "data/eval/hi/next-hi.heldout.txt",
        ],
        digest: 0x3a0a_665c_10f7_ab20,
    },
    Case {
        code: "beng",
        id: "as",
        lexicon: "as.fst",
        virama: '\u{09CD}',
        consonant: 'ক',
        written: &["data/eval/as/translit-human.tsv"],
        digest: 0x1c08_ef56_a330_41d2,
    },
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The rules made from `data/langs/<id>/lang.json`.
fn data_rules(case: &Case) -> &'static dyn ScriptRules {
    let path = root().join(format!("data/langs/{}/lang.json", case.id));
    DataScript::intern(ScriptDef::open(&path).unwrap())
}

/// The rules written as code.
fn coded_rules(case: &Case) -> &'static dyn ScriptRules {
    mlengine_core::script::for_pack(case.id, case.code, None).unwrap()
}

/// Text for a fixture line: the joiners, which no editor shows, by name.
fn show(text: &str) -> String {
    text.replace(ZWJ, "{ZWJ}").replace(ZWNJ, "{ZWNJ}")
}

fn hex(text: &str) -> String {
    text.chars()
        .map(|c| format!("{:04X}", c as u32))
        .collect::<Vec<_>>()
        .join(" ")
}

fn base(rules: &dyn ScriptRules, context: &str) -> String {
    rules
        .mark_base(context)
        .map_or("-".to_string(), |c| format!("{:04X}", c as u32))
}

/// The characters observed one by one: the block, the joiners, the apostrophes, the dandas
/// (Devanagari's, which every Indic script borrows) and a Latin letter.
fn alphabet(block: RangeInclusive<char>) -> Vec<char> {
    let mut chars: Vec<char> = block.collect();
    for c in [ZWJ, ZWNJ, '\'', '\u{2019}', '।', '॥', 'a'] {
        if !chars.contains(&c) {
            chars.push(c);
        }
    }
    chars
}

/// The lines about characters and short sequences.
fn character_lines(case: &Case, rules: &dyn ScriptRules) -> Vec<String> {
    let (k, v) = (case.consonant, case.virama);
    let chars = alphabet(rules.block());
    let mut out = vec![format!(
        "script\t{}\t{:04X}-{:04X}\tapostrophe {}",
        rules.code(),
        *rules.block().start() as u32,
        *rules.block().end() as u32,
        rules.inner_apostrophe()
    )];
    for &c in &chars {
        let s = c.to_string();
        out.push(format!(
            "c\t{:04X}\t{}{}{}\t{}\t{} {} {}",
            c as u32,
            if rules.is_letter(c) { 'L' } else { '-' },
            if rules.is_word_char(c) { 'W' } else { '-' },
            if rules.ends_phrase(c) { 'P' } else { '-' },
            hex(&rules.normalize(&s)),
            base(rules, &s),
            base(rules, &format!("{k}{c}")),
            base(rules, &format!("{k}{v}{c}")),
        ));
    }
    for &a in &chars {
        let alone = rules.normalize(&a.to_string());
        for &b in &chars {
            let pair = format!("{a}{b}");
            let normal = rules.normalize(&pair);
            let apart = format!("{alone}{}", rules.normalize(&b.to_string()));
            let (got, second) = (base(rules, &pair), base(rules, &b.to_string()));
            if normal != apart || got != second {
                out.push(format!("p\t{}\t{}\t{got}", hex(&pair), hex(&normal)));
            }
        }
    }
    for &c in &chars {
        for seq in [
            format!("{c}{v}{ZWJ}"),
            format!("{c}{v}{ZWNJ}"),
            format!("{c}{v}{ZWJ}{k}"),
            format!("{c}{v}{ZWNJ}{k}"),
            format!("{k}{ZWNJ}{c}"),
            format!("{k}{c}{ZWJ}"),
        ] {
            out.push(format!("t\t{}\t{}", hex(&seq), hex(&rules.normalize(&seq))));
        }
    }
    out
}

/// `word` in the other encodings text arrives in; never `word` itself.
fn variants(word: &str, virama: char) -> Vec<String> {
    let v = virama.to_string();
    let mut out = vec![
        word.nfd().collect::<String>(),
        word.replace(virama, &format!("{v}{ZWJ}")),
        word.replace(virama, &format!("{v}{ZWNJ}")),
        format!("{word}{ZWNJ}"),
        word.replace('\'', "\u{2019}"),
        // Malayalam's chillus as consonant + virama + ZWJ.
        word.replace('ൻ', "ന\u{0D4D}\u{200D}")
            .replace('ർ', "ര\u{0D4D}\u{200D}")
            .replace('ൽ', "ല\u{0D4D}\u{200D}")
            .replace('ൾ', "ള\u{0D4D}\u{200D}")
            .replace('ൺ', "ണ\u{0D4D}\u{200D}")
            .replace('ൿ', "ക\u{0D4D}\u{200D}"),
        // The Bengali ra, the halves of ো in visual order, অ + া, khanda ta the old way.
        word.replace('ৰ', "র"),
        word.replace('ো', "\u{09BE}\u{09C7}"),
        word.replace('আ', "অ\u{09BE}"),
        word.replace('ৎ', "ত\u{09CD}\u{200D}"),
    ];
    // Devanagari's and Bengali's precomposed nukta letters, which NFC takes apart.
    let mut precomposed = word.to_string();
    for c in ('\u{0958}'..='\u{095F}').chain(['\u{09DC}', '\u{09DD}', '\u{09DF}']) {
        let apart: String = c.to_string().nfd().collect();
        precomposed = precomposed.replace(&apart, &c.to_string());
    }
    out.push(precomposed);
    out.sort();
    out.dedup();
    out.retain(|w| w != word);
    out
}

/// One line about `input`: its normal form (`=` when unchanged), the other final-u spelling,
/// the vowel-sign drops, and the joins with `suffixes` (index and joined word, where they
/// join).
fn word_line(rules: &dyn ScriptRules, input: &str, suffixes: &[&Suffix]) -> String {
    let normal = rules.normalize(input);
    let mut line = format!(
        "w\t{}\t{}\t{}\t",
        show(input),
        if normal == input {
            "=".to_string()
        } else {
            show(&normal)
        },
        rules.other_final_u(input).map_or("-".to_string(), |w| show(&w)),
    );
    let drops = rules.vowel_sign_drops(input);
    if drops.is_empty() {
        line.push('-');
    } else {
        line.push_str(&show(&drops.join(",")));
    }
    line.push('\t');
    for (i, suffix) in suffixes.iter().enumerate() {
        if let Some(joined) = rules.join_suffix(input, &suffix.native, suffix.join) {
            write!(line, "{i}:{} ", show(&joined)).unwrap();
        }
    }
    line.truncate(line.trim_end_matches(' ').len());
    line
}

/// Malayalam's suffix table, the only one there is: every script is asked to join with it.
fn suffix_table() -> Suffixes {
    Suffixes::load(&root().join("packs/ml.neopack/suffixes.tsv")).unwrap()
}

fn lexicon_words(case: &Case) -> Vec<String> {
    let path = root().join(format!("packs/{}.neopack/{}", case.id, case.lexicon));
    let lexicon = Lexicon::open(&path).unwrap();
    let mut words = Vec::new();
    lexicon.words(|key, _| words.push(String::from_utf8(key.to_vec()).unwrap()));
    words
}

/// The native words of the case's eval sets, as written, in file order, each once.
fn written_words(case: &Case, rules: &dyn ScriptRules) -> Vec<String> {
    let block = rules.block();
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for file in case.written {
        let text = std::fs::read_to_string(root().join(file)).unwrap();
        for line in text.lines().filter(|l| !l.starts_with('#')) {
            for token in line.split_whitespace() {
                let word = token.trim_matches(|c: char| {
                    !block.contains(&c) && c != ZWJ && c != ZWNJ
                });
                if word.chars().any(|c| block.contains(&c)) && seen.insert(word.to_string()) {
                    out.push(word.to_string());
                }
            }
        }
    }
    out
}

/// The fixture's lines for `rules`.
fn fixture_lines(case: &Case, rules: &dyn ScriptRules) -> Vec<String> {
    let table = suffix_table();
    let suffixes: Vec<&Suffix> = table.iter().collect();
    let mut out = character_lines(case, rules);
    let words = lexicon_words(case);
    let step = (words.len() / 200).max(1);
    let sample = words.iter().step_by(step).take(200);
    let written = written_words(case, rules);
    for (i, word) in sample.chain(written.iter().take(150)).enumerate() {
        out.push(word_line(rules, word, if i < 60 { &suffixes } else { &[] }));
        for variant in variants(word, case.virama) {
            out.push(word_line(rules, &variant, &[]));
        }
    }
    out
}

fn fixture_path(case: &Case) -> PathBuf {
    root().join(format!(
        "engine/core/tests/fixtures/script-rules/{}.tsv",
        case.code
    ))
}

fn fnv(hash: &mut u64, line: &str) {
    for b in line.bytes().chain([b'\n']) {
        *hash ^= u64::from(b);
        *hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
}

/// Every lexicon word's line with every suffix, its variants' lines, and every eval word's
/// with its variants', through `visit`; returns how many lines.
fn every_word_line(case: &Case, rules: &dyn ScriptRules, mut visit: impl FnMut(&str)) -> usize {
    let table = suffix_table();
    let suffixes: Vec<&Suffix> = table.iter().collect();
    let mut n = 0;
    let words = lexicon_words(case);
    let written = written_words(case, rules);
    for word in words.iter().chain(&written) {
        visit(&word_line(rules, word, &suffixes));
        n += 1;
        for variant in variants(word, case.virama) {
            visit(&word_line(rules, &variant, &[]));
            n += 1;
        }
    }
    n
}

fn digest(case: &Case, rules: &dyn ScriptRules) -> (u64, usize) {
    let mut hash = 0xCBF2_9CE4_8422_2325;
    let n = every_word_line(case, rules, |line| fnv(&mut hash, line));
    (hash, n)
}

/// Before the code is deleted: the rules made from each `lang.json` give, line for line, what
/// the coded plug-in gives, on the fixture's lines.
#[test]
fn data_rules_equal_the_coded_rules() {
    for case in &CASES {
        let (coded, data) = (coded_rules(case), data_rules(case));
        assert!(!std::ptr::addr_eq(coded, data), "{}: two plug-ins", case.code);
        let (want, got) = (fixture_lines(case, coded), fixture_lines(case, data));
        assert_eq!(want.len(), got.len(), "{}", case.code);
        for (w, g) in want.iter().zip(&got) {
            assert_eq!(w, g, "{}: coded, then data", case.code);
        }
        eprintln!("{}: {} lines equal", case.code, want.len());
    }
}

/// The same on every word of the three lexicons and eval sets, with every suffix.
#[test]
#[ignore]
fn data_rules_equal_the_coded_rules_on_every_word() {
    for case in &CASES {
        let (coded, data) = (coded_rules(case), data_rules(case));
        let mut want = Vec::new();
        every_word_line(case, coded, |line| want.push(line.to_string()));
        let mut i = 0;
        let n = every_word_line(case, data, |line| {
            assert_eq!(want[i], line, "{}: coded, then data", case.code);
            i += 1;
        });
        assert_eq!(n, want.len());
        let (hash, _) = digest(case, coded);
        eprintln!("{}: {n} lines equal; coded digest {hash:#018x}", case.code);
    }
}

/// Writes the fixtures from the coded rules: `RECORD_SCRIPT_RULES=1 cargo test … record`.
#[test]
#[ignore]
fn record() {
    if std::env::var_os("RECORD_SCRIPT_RULES").is_none() {
        return;
    }
    for case in &CASES {
        let path = fixture_path(case);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut text = fixture_lines(case, coded_rules(case)).join("\n");
        text.push('\n');
        std::fs::write(&path, text).unwrap();
        let (hash, n) = digest(case, coded_rules(case));
        eprintln!("{}: digest {hash:#018x} over {n} lines", case.code);
    }
}

/// The rules in use give what was recorded.
#[test]
fn rules_are_as_recorded() {
    for case in &CASES {
        let path = fixture_path(case);
        let Ok(recorded) = std::fs::read_to_string(&path) else {
            panic!("{}: not recorded yet", path.display());
        };
        let got = fixture_lines(case, data_rules(case));
        let want: Vec<&str> = recorded.lines().collect();
        for (i, (w, g)) in want.iter().zip(&got).enumerate() {
            assert_eq!(w, g, "{} line {}: recorded, then now", case.code, i + 1);
        }
        assert_eq!(want.len(), got.len(), "{}", case.code);
    }
}

/// Every word of the three lexicons, hashed, against the hash recorded from the coded rules.
#[test]
#[ignore]
fn every_lexicon_word_is_as_recorded() {
    for case in &CASES {
        let (hash, n) = digest(case, data_rules(case));
        eprintln!("{}: {n} lines, digest {hash:#018x}", case.code);
        assert_eq!(hash, case.digest, "{}: {hash:#018x}", case.code);
    }
}
