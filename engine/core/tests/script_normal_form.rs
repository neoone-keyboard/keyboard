//! Every word of the Hindi and Assamese lexicons is in its script's normal form and is
//! a word of that script (`text::script`), so a typed word normalised at run time
//! (`script::normalize_word`) meets the stored one. The lexicons were built by lexbuild with
//! the same rules (`lexbuild --lang` for hi, `lexbuild-native` and `data/langs/as/lang.json`
//! for as). Every word of two lexicons, so `#[ignore]`: `make test-data` runs it.

use std::path::{Path, PathBuf};

use mlengine_core::lexicon::Lexicon;
use mlengine_core::script;
use mlengine_core::text::{self, Script};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Checks every word of `packs/<id>.neopack/<file>`; returns (words, words with an apostrophe).
fn check(id: &str, file: &str, code: &'static str) -> (usize, usize) {
    let dir = root().join(format!("packs/{id}.neopack"));
    let lexicon = Lexicon::open(&dir.join(file)).unwrap();
    // The rules the engine uses for this pack, installed as opening it would.
    let pack = mlengine_core::pack::Pack::open(&dir).unwrap();
    let rules = script::for_pack(code, pack.file("lang").as_deref()).unwrap();
    script::install(rules).unwrap();
    let (mut n, mut apostrophes, mut bad) = (0, 0, Vec::new());
    lexicon.words(|key, _| {
        let word = std::str::from_utf8(key).unwrap();
        n += 1;
        if word.contains('\'') {
            apostrophes += 1;
        }
        let normal = rules.normalize(word);
        let whole = script::normalize_word(word);
        let class = text::script(word);
        if (normal != word || whole != word || class != Script::Native(code)) && bad.len() < 20 {
            bad.push(format!(
                "{word:?}: normal {normal:?}, normalize_word {whole:?}, {class:?}"
            ));
        }
    });
    assert!(
        bad.is_empty(),
        "{id}: first mismatches:\n{}",
        bad.join("\n")
    );
    (n, apostrophes)
}

#[test]
#[ignore]
fn hindi_and_assamese_words_are_in_normal_form() {
    let (hi, hi_apostrophes) = check("hi", "hi.fst", "deva");
    eprintln!("hi: {hi} words, {hi_apostrophes} with an apostrophe");
    assert!(hi > 100_000, "{hi} words");
    let (as_, as_apostrophes) = check("as", "as.fst", "beng");
    eprintln!("as: {as_} words, {as_apostrophes} with an apostrophe");
    assert!(as_ > 100_000, "{as_} words");
}
