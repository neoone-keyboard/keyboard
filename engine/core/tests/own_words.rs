//! The user's own words: a native-script suggestion tapped in the bar that the pack's lexicon
//! doesn't hold (the transliterator's free spelling, mostly) is from then on a word of that
//! pack. It is kept in the learned-word store with its language, found from the letters it was
//! picked for, from other spellings and from its first letters as the pack's own words are,
//! read by the transliterator's lexicon-held beam, lifted by its uses, and dropped everywhere
//! when it is forgotten or removed.
//!
//! On the Hindi pack (`packs/hi.neopack`), whose lexicon has रोबदार and not रौबदार, and on the
//! Gujarati test pack (`fixtures/gu.neopack`), which has no transliterator.

use std::fs;
use std::path::{Path, PathBuf};

use mlengine_core::lang::LangId;
use mlengine_core::lexicon::{self, Lexicon};
use mlengine_core::manglish;
use mlengine_core::own::OwnWords;
use mlengine_core::pack::Pack;
use mlengine_core::romanisation::Romanisation;
use mlengine_core::{Candidate, Engine, Kind, Layout, Paths};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn en_fst() -> PathBuf {
    root().join("apps/ios/KeyboardExtension/Lexicon/en.fst")
}

fn pack(id: &str) -> Pack {
    let dir = match id {
        "gu" => Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gu.neopack"),
        _ => root().join(format!("packs/{id}.neopack")),
    };
    Pack::open(&dir).unwrap()
}

/// English and the pack `id`, with its transliterator or without it, and the learned-word
/// store in `user` (in memory without one).
fn engine(id: &str, model: bool, user: Option<&Path>) -> Engine {
    let mut paths = Paths::with_packs(Some(en_fst()), &[pack(id)], user.map(Path::to_path_buf));
    if !model {
        paths.packs[0].translit = None;
    }
    Engine::open(&paths)
}

fn lexicon_of(id: &str) -> Lexicon {
    Lexicon::open(&pack(id).file("lexicon").unwrap()).unwrap()
}

fn suggest(e: &Engine, context: &str) -> Vec<Candidate> {
    e.suggest(context, Layout::Latin, true).candidates
}

fn bar(e: &Engine, context: &str) -> Vec<String> {
    suggest(e, context).into_iter().map(|c| c.text).collect()
}

/// A tap on `chosen` in the bar for `typed` after `before`, as the keyboard reports it: the
/// choice with the candidate's language, then the use.
fn tap(e: &Engine, before: &str, typed: &str, chosen: &str) {
    let candidates = suggest(e, &format!("{before}{typed}"));
    let tapped = candidates
        .iter()
        .find(|c| c.text == chosen)
        .unwrap_or_else(|| panic!("{chosen} is not in the bar for {typed}: {candidates:?}"));
    let lang = e.lang_id(tapped.lang);
    e.note_choice(typed, chosen, lang.as_ref());
    e.note_used(before, chosen, Layout::Latin);
}

/// The word the Hindi lexicon has, and the spelling of `raubdar` it doesn't.
const IN_LEXICON: &str = "रोबदार";
const OWN: &str = "रौबदार";

fn hindi(e: &Engine) -> Option<LangId> {
    e.lang_of_id("hi").and_then(|lang| e.lang_id(lang))
}

#[test]
fn the_free_spelling_is_offered_and_the_lexicon_does_not_hold_it() {
    let lex = lexicon_of("hi");
    assert!(lex.get(IN_LEXICON.as_bytes()).is_some());
    assert!(lex.get(OWN.as_bytes()).is_none());
    let e = engine("hi", true, None);
    assert_eq!(bar(&e, "raubdar"), ["raubdar", IN_LEXICON, OWN]);
    // Before it is picked, nothing else finds it.
    assert!(!bar(&e, "raub").contains(&OWN.to_string()));
    assert!(!bar(&e, "robdar").contains(&OWN.to_string()));
    assert!(e.learned_words().is_empty());
}

#[test]
fn a_tapped_free_spelling_becomes_a_word_of_its_language() {
    let e = engine("hi", true, None);
    tap(&e, "", "raubdar", OWN);
    assert_eq!(e.learned_words(), [OWN], "it is in Your words");
    // The letters it was picked for: it leads (`note_choice`).
    assert_eq!(bar(&e, "raubdar")[0], OWN);
    // Another spelling with the same skeleton: offered, as a word of Hindi, but on its first
    // use it does not replace what was typed.
    let loose = suggest(&e, "raubdaar");
    assert_eq!(loose[0].text, "raubdaar", "{loose:?}");
    assert_eq!(loose[1].text, OWN, "{loose:?}");
    assert_eq!((loose[1].kind, e.lang_id(loose[1].lang)), (Kind::Romanised, hindi(&e)));
    // Its first letters: offered as the pack's words are from a prefix.
    let partial = suggest(&e, "raub");
    assert!(partial[0].text != OWN, "a completion never takes the middle: {partial:?}");
    assert!(partial.iter().any(|c| c.text == OWN), "{partial:?}");
    let partial = suggest(&e, "raubd");
    assert!(partial[1..].iter().any(|c| c.text == OWN), "{partial:?}");
}

#[test]
fn the_models_lexicon_reading_accepts_it() {
    // `robdar` has another skeleton (o, not au): only the transliterator reads रौबदार from it,
    // and its lexicon-held beam may now spell it.
    let with = engine("hi", true, None);
    let without = engine("hi", false, None);
    for e in [&with, &without] {
        e.note_choice("raubdar", OWN, hindi(e).as_ref());
        e.note_used("", OWN, Layout::Latin);
    }
    let read = suggest(&with, "robdar");
    assert_eq!(read[0].text, "robdar", "not in the middle on its first use: {read:?}");
    assert!(read.iter().any(|c| c.text == OWN), "{read:?}");
    assert!(read.iter().any(|c| c.text == IN_LEXICON), "{read:?}");
    assert!(!bar(&without, "robdar").contains(&OWN.to_string()));
    // The index needs no model.
    assert_eq!(bar(&without, "raubdaar")[..2], ["raubdaar", OWN]);
    assert!(bar(&without, "raub").contains(&OWN.to_string()));
}

#[test]
fn it_survives_a_restart_with_its_language() {
    let dir = tempfile::tempdir().unwrap();
    {
        let e = engine("hi", true, Some(dir.path()));
        tap(&e, "", "raubdar", OWN);
    }
    let log = fs::read_to_string(dir.path().join("learned-words.log")).unwrap();
    assert!(log.lines().any(|l| l == format!("W\thi\t{OWN}\traubdar")), "{log}");
    assert!(log.lines().any(|l| l == format!("N\t{OWN}")), "the tap is its first use: {log}");
    for model in [true, false] {
        let e = engine("hi", model, Some(dir.path()));
        assert_eq!(e.learned_words(), [OWN]);
        assert_eq!(bar(&e, "raubdar")[0], OWN);
        assert_eq!(bar(&e, "raubdaar")[..2], ["raubdaar", OWN]);
        assert!(bar(&e, "raub").contains(&OWN.to_string()));
    }
}

/// Each pick puts its word into the engine's index; a restart builds the index from the log.
/// The two are the same index: every bar is the same.
#[test]
fn the_index_kept_up_by_picks_is_the_one_a_restart_builds() {
    let dir = tempfile::tempdir().unwrap();
    let live = engine("hi", true, Some(dir.path()));
    let hi = hindi(&live);
    assert!(!bar(&live, "kshitijavar").is_empty(), "the index is read before anything is picked");
    tap(&live, "", "raubdar", OWN);
    tap(&live, "", "kshitijavardhan", "क्षितिजवर्धन");
    live.note_choice("roabdaar", OWN, hi.as_ref());
    live.note_choice("jhumritalaiya", "झुमरीतलैया", hi.as_ref());
    live.forget_word("क्षितिजवर्धन");
    live.note_choice("kshitijvardhan", "क्षितिजवर्धन", hi.as_ref());
    let restarted = engine("hi", true, Some(dir.path()));
    for typed in ["raubdar", "raubdaar", "roabdar", "robdar", "kshitijavardhan", "kshitijvardhn", "jhumritalaiyaa"] {
        for end in 2..=typed.len() {
            assert_eq!(suggest(&live, &typed[..end]), suggest(&restarted, &typed[..end]), "{}", &typed[..end]);
        }
    }
    assert!(bar(&live, "roabdar").contains(&OWN.to_string()), "found by the later letters too");
}

fn place(e: &Engine, context: &str, word: &str) -> usize {
    bar(e, context).iter().position(|w| w == word).unwrap_or(usize::MAX)
}

fn score(e: &Engine, context: &str, word: &str) -> f32 {
    suggest(e, context).iter().find(|c| c.text == word).map_or(f32::NEG_INFINITY, |c| c.score)
}

#[test]
fn uses_lift_it_and_a_second_use_lets_it_replace() {
    let e = engine("hi", true, None);
    tap(&e, "", "raubdar", OWN);
    assert_eq!(bar(&e, "raubdaar")[0], "raubdaar", "used once: a side slot");
    assert_eq!(bar(&e, "robdar")[0], "robdar");
    let (loose, partial) = (score(&e, "raubdaar", OWN), score(&e, "raub", OWN));
    // Used again, it is a word like the pack's own: it takes the middle for a loose spelling.
    e.note_used("", OWN, Layout::Latin);
    assert_eq!(bar(&e, "raubdaar")[..2], [OWN, "raubdaar"]);
    assert!(score(&e, "raub", OWN) > partial);
    assert_ne!(bar(&e, "raub")[0], OWN, "its first letters never become it");
    for _ in 0..8 {
        e.note_used("", OWN, Layout::Latin);
    }
    assert!(score(&e, "raubdaar", OWN) > loose);
    assert_eq!(place(&e, "raub", OWN), 2, "{:?}", suggest(&e, "raub"));
    // English stays English.
    assert_eq!(bar(&e, "round")[0], "round");
    assert_eq!(bar(&e, "I said teh")[0], "the");
}

#[test]
fn forgetting_it_drops_it_from_the_list_and_the_index() {
    let e = engine("hi", true, None);
    tap(&e, "", "raubdar", OWN);
    e.note_used("", OWN, Layout::Latin);
    assert_eq!(bar(&e, "raubdaar")[0], OWN);
    e.forget_word(OWN);
    assert!(e.learned_words().is_empty());
    assert!(!bar(&e, "raub").contains(&OWN.to_string()));
    assert!(!bar(&e, "robdar").contains(&OWN.to_string()));
    // Not blocked: it is the model's free spelling again, as before it was picked.
    assert_eq!(bar(&e, "raubdar"), ["raubdar", IN_LEXICON, OWN]);
    assert_eq!(bar(&e, "raubdaar")[0], "raubdaar");
}

#[test]
fn removing_it_from_the_bar_drops_it_everywhere() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine("hi", true, Some(dir.path()));
    tap(&e, "", "raubdar", OWN);
    e.remove_word(OWN);
    assert!(e.learned_words().is_empty());
    for typed in ["raubdar", "raubdaar", "raub", "robdar"] {
        assert!(!bar(&e, typed).contains(&OWN.to_string()), "{typed}");
    }
    let reopened = engine("hi", true, Some(dir.path()));
    assert!(!bar(&reopened, "raubdaar").contains(&OWN.to_string()));
    // Clearing everything drops the index too.
    tap(&reopened, "", "kshitijavardhan", "क्षितिजवर्धन");
    assert!(bar(&reopened, "kshitijavar").contains(&"क्षितिजवर्धन".to_string()));
    reopened.forget_learned();
    assert!(reopened.learned_words().is_empty());
    assert!(!bar(&reopened, "kshitijavar").contains(&"क्षितिजवर्धन".to_string()));
}

#[test]
fn only_a_tap_adds_a_word() {
    let e = engine("hi", true, None);
    // A word that went into the text any other way (typed, or put in by autocorrect) is
    // reported as used and committed, never as a choice.
    e.note_used("", OWN, Layout::Latin);
    e.note_committed(OWN);
    assert!(e.learned_words().is_empty());
    assert!(!bar(&e, "raub").contains(&OWN.to_string()));
    // A tapped word the lexicon holds is a choice and nothing more.
    tap(&e, "", "raubdar", IN_LEXICON);
    assert_eq!(bar(&e, "raubdar")[0], IN_LEXICON);
    assert!(e.learned_words().is_empty());
    // Nor is a Latin word, or letters that are no word.
    e.note_choice("raubdar", "robber", None);
    e.note_choice("r", OWN, None);
    assert!(e.learned_words().is_empty());
}

#[test]
fn the_language_is_the_one_named_when_it_writes_the_script_else_the_scripts() {
    for named in [Some("hi"), Some("en"), Some("xx"), Some("gu"), None] {
        let dir = tempfile::tempdir().unwrap();
        let e = engine("hi", false, Some(dir.path()));
        let lang = named.map(|id| LangId(id.into()));
        e.note_choice("raubdar", OWN, lang.as_ref());
        let log = fs::read_to_string(dir.path().join("learned-words.log")).unwrap();
        assert!(log.lines().any(|l| l == format!("W\thi\t{OWN}\traubdar")), "{named:?}: {log}");
    }
}

#[test]
fn letters_that_only_begin_the_word_are_not_kept_as_a_spelling_of_it() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine("hi", true, Some(dir.path()));
    tap(&e, "", "raubdar", OWN);
    // The completion tapped after four letters: a choice for them, and a use.
    tap(&e, "", "raub", OWN);
    assert_eq!(bar(&e, "raub")[0], OWN, "chosen for these letters");
    let log = fs::read_to_string(dir.path().join("learned-words.log")).unwrap();
    assert!(!log.contains("\traub\n"), "{log}");
    // So other letters with that skeleton don't spell the whole word.
    assert_ne!(bar(&e, "raaub")[0], OWN);
}

#[test]
fn an_empty_store_and_words_of_a_language_not_loaded_change_nothing() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("learned-words.log"),
        "W\tas\tপ্ৰাপ্তিতকৈ\tpraptitkoi\nW\tgu\tરૌબદાર\traubdar\n",
    )
    .unwrap();
    let plain = engine("hi", true, None);
    let other = engine("hi", true, Some(dir.path()));
    assert_eq!(other.learned_words().len(), 2, "kept, for when their packs are back");
    for context in ["raubdar", "raub", "robdar", "praptitkoi", "prap", "mera naam", "kshitijav", "I said teh"] {
        for end in 1..=context.len() {
            let typed = &context[..end];
            assert_eq!(suggest(&plain, typed), suggest(&other, typed), "{typed}");
        }
    }
}

/// A copy of the Gujarati test pack whose lexicons leave `word` out, as if the pack had been
/// built without it (`lexbuild-native` writes the romanised keys with `manglish::entries`).
fn pack_without(word: &str) -> tempfile::TempDir {
    let source = pack("gu");
    let r = Romanisation::load(&source.file("romanisation").unwrap()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    for entry in fs::read_dir(&source.dir).unwrap() {
        let path = entry.unwrap().path();
        fs::copy(&path, dir.path().join(path.file_name().unwrap())).unwrap();
    }
    let mut words = words_of("gu");
    assert!(words.iter().any(|(w, _)| w == word), "{word} is in the pack");
    words.retain(|(w, _)| w != word);
    let native = words.iter().map(|(w, count)| (w.as_bytes().to_vec(), *count));
    fs::write(dir.path().join("gu.fst"), lexicon::build(native).unwrap()).unwrap();
    let romanised = lexicon::build(manglish::entries(&r, &words)).unwrap();
    fs::write(dir.path().join("romanised.fst"), romanised).unwrap();
    dir
}

/// The pack's words with their counts, the most frequent first.
fn words_of(id: &str) -> Vec<(String, u64)> {
    let mut words: Vec<(String, u64)> = Vec::new();
    lexicon_of(id).words(|key, count| words.push((String::from_utf8(key.to_vec()).unwrap(), count)));
    words.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    words
}

/// Loose spellings of a word's romanisation: vowels shortened, lengthened and respelled.
fn loose_forms(r: &Romanisation, word: &str) -> Vec<String> {
    let roman = r.romanize(word);
    let mut forms = vec![roman.clone(), roman.replace("aa", "a"), roman.replace('a', "aa")];
    forms.push(roman.replace("ee", "i").replace("oo", "u"));
    forms.sort();
    forms.dedup();
    forms
}

/// `loose_forms`, and every beginning of the romanisation of two letters or more.
fn typed_forms(r: &Romanisation, word: &str) -> Vec<String> {
    let roman = r.romanize(word);
    let mut forms = loose_forms(r, word);
    forms.extend((2..roman.len()).map(|end| roman[..end].to_string()));
    forms.sort();
    forms.dedup();
    forms
}

/// The pack's tables and its romanised lexicon, as `lexbuild-native` built it. The keys are
/// packed from the first character of the pack's own rules, as the engine sets it for a
/// loaded pack (`Romanisation::packed_from`).
fn built(id: &str) -> (Romanisation, Lexicon) {
    let pack = pack(id);
    let rules = mlengine_core::script::for_pack(&pack.manifest.script, pack.file("lang").as_deref())
        .unwrap();
    (
        Romanisation::load(&pack.file("romanisation").unwrap())
            .unwrap()
            .packed_from(*rules.block().start()),
        Lexicon::open(&pack.file("romanisedLexicon").unwrap()).unwrap(),
    )
}

#[test]
fn the_key_of_an_own_word_is_the_key_the_pack_builder_writes() {
    for id in ["gu", "hi", "as"] {
        let (r, romanised) = built(id);
        let words = words_of(id);
        let mut keyed = 0;
        for (word, _) in words.iter().step_by(words.len() / 2_000 + 1) {
            let Some(key) = manglish::key(&r, word) else {
                continue;
            };
            assert!(romanised.get(&key).is_some(), "{id} {word}: not keyed as the pack keys it");
            keyed += 1;
        }
        assert!(keyed > 30, "{id}: {keyed}");
    }
}

/// Every typed form that finds a word in the pack's romanised lexicon finds the same word
/// added on the phone, as near or as far: the same letters still to type, the same fit.
#[test]
fn a_word_added_on_the_phone_is_found_from_the_typed_forms_that_find_it_in_the_pack() {
    for id in ["gu", "hi", "as"] {
        let (r, romanised) = built(id);
        let words = words_of(id);
        let (mut tried, mut found) = (0, 0);
        for (word, _) in words.iter().step_by(words.len() / 300 + 1) {
            let own = OwnWords::build(&r, std::iter::once((word.as_str(), &[][..])));
            for typed in typed_forms(&r, word) {
                tried += 1;
                let fit = |m: &manglish::Match| (m.missing, m.distance, m.spelled);
                let added = own.lookup(&r, &typed, &|_| 0);
                let in_pack = manglish::lookup(&r, &romanised, &typed);
                let Some(in_pack) = in_pack.iter().find(|m| m.word == *word) else {
                    continue;
                };
                found += 1;
                assert_eq!(added.len(), 1, "{id} {word} from {typed}");
                assert_eq!(fit(&added[0]), fit(in_pack), "{id} {word} from {typed}");
            }
        }
        assert!(found > 100, "{id}: {found} of {tried} typed forms find the pack's word");
    }
}

/// The same through the engine, on the Gujarati test pack rebuilt without the word: where the
/// pack's word is offered for a spelling of it, the added word is.
#[test]
fn a_word_added_on_the_phone_is_offered_as_the_same_word_built_into_the_pack() {
    let (r, _) = built("gu");
    let mut with = engine("gu", false, None);
    with.set_max_suggestions(mlengine_core::MAX_SUGGESTIONS);
    let words = words_of("gu");
    let (mut tried, mut found) = (0, 0);
    for (word, _) in words.iter().step_by(4) {
        let dir = pack_without(word);
        let without = Pack::open(dir.path()).unwrap();
        let mut added = Engine::open(&Paths::with_packs(Some(en_fst()), &[without], None));
        added.set_max_suggestions(mlengine_core::MAX_SUGGESTIONS);
        assert!(
            typed_forms(&r, word).iter().all(|typed| !bar(&added, typed).contains(word)),
            "{word} is gone from the pack"
        );
        added.note_choice(&r.romanize(word), word, Some(&LangId("gu".into())));
        if r.romanize(word).len() < 2 {
            continue;
        }
        assert_eq!(added.learned_words(), [word.as_str()]);
        for typed in loose_forms(&r, word) {
            tried += 1;
            if bar(&with, &typed).contains(word) {
                found += 1;
                assert!(
                    bar(&added, &typed).contains(word),
                    "{word} from {typed}: the pack's word is offered, the added one is not"
                );
            }
        }
    }
    assert!(found > 10, "{found} of {tried} typed forms find the pack's word");

}

/// The held-out transliteration pairs of a pack (`data/eval/<id>/translit-human.tsv`): the
/// Latin spellings of each native word, in file order.
fn held_out(id: &str) -> Vec<(String, Vec<String>)> {
    let text = fs::read_to_string(root().join(format!("data/eval/{id}/translit-human.tsv"))).unwrap();
    let mut words: Vec<(String, Vec<String>)> = Vec::new();
    let mut place: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let fields: Vec<&str> = line.split('\t').collect();
        let [_, latin, native] = fields[..] else {
            continue;
        };
        if !latin.bytes().all(|b| b.is_ascii_lowercase()) || latin.len() < 4 {
            continue;
        }
        let at = *place.entry(native.to_string()).or_insert_with(|| {
            words.push((native.to_string(), Vec::new()));
            words.len() - 1
        });
        words[at].1.push(latin.to_string());
    }
    words
}

/// The numbers behind `rank::OWN_WORD_LOG_PROB` and `rank::OWN_WORD_REPLACES_AFTER`, on each
/// pack's held-out transliteration pairs. Not a check: it prints one line per pack for the
/// constants as they are, and was run at each value in the table in `rank.rs`.
///
///     cargo test --release -p mlengine-core --test own_words own_word_scores -- --ignored --nocapture
///
/// Own words: the held-out words the lexicon doesn't hold that have two spellings or more,
/// each picked for its first spelling. Reach: its other spellings, and the first 60% of the
/// letters of the first, that show it in the three slots (before: with nothing picked). Harm:
/// the held-out words the lexicon does hold, typed with the own words in the store, each used
/// once and then each used three times: how many show the wanted word, and how many put an
/// own word in the middle. English: the typo set's middle slot. Every engine has the same
/// language habit (`rank::HABIT_WEIGHT`), which the uses would otherwise move.
#[test]
#[ignore = "a measurement for rank::OWN_WORD_LOG_PROB, not a check"]
fn own_word_scores() {
    let typos: Vec<(String, String)> = fs::read_to_string(root().join("data/eval/english-typos.tsv"))
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let fields: Vec<&str> = l.split('\t').collect();
            Some((format!("{}{}", fields.first()?, fields.get(1)?), fields.get(2)?.to_string()))
        })
        .collect();
    println!(
        "OWN_WORD_LOG_PROB {} OWN_WORD_REPLACES_AFTER {}",
        mlengine_core::rank::OWN_WORD_LOG_PROB,
        mlengine_core::rank::OWN_WORD_REPLACES_AFTER
    );
    for id in ["hi", "as", "ta"] {
        let lex = lexicon_of(id);
        let lang = LangId(id.into());
        let words = held_out(id);
        let (known, unknown): (Vec<_>, Vec<_>) =
            words.iter().partition(|(w, _)| lex.get(w.as_bytes()).is_some());
        let own: Vec<&(String, Vec<String>)> =
            unknown.iter().copied().filter(|(_, latin)| latin.len() >= 2).take(400).collect();
        let known: Vec<&(String, Vec<String>)> = known.into_iter().take(1_000).collect();
        let empty = engine(id, true, None);
        let once = engine(id, true, None);
        let thrice = engine(id, true, None);
        // The same language habit in each, so that only the own words differ: a native word
        // that is in no lexicon and is never picked, typed from the Latin layout.
        let stranger = unknown.iter().find(|(_, latin)| latin.len() < 2).map(|(w, _)| w.as_str()).unwrap();
        for e in [&empty, &once, &thrice] {
            for _ in 0..200 {
                e.note_used("", stranger, Layout::Latin);
            }
        }
        for (word, latin) in &own {
            for e in [&once, &thrice] {
                e.note_choice(&latin[0], word, Some(&lang));
                e.note_used("", word, Layout::Latin);
            }
            thrice.note_used("", word, Layout::Latin);
            thrice.note_used("", word, Layout::Latin);
        }
        assert_eq!(once.learned_words().len(), own.len());
        let is_own = |text: &str| own.iter().any(|(w, _)| w == text);
        // Reach.
        let shown = |e: &Engine, typed: &str, word: &str| bar(e, typed).iter().any(|w| w == word);
        let (mut others, mut other_before, mut other_once, mut other_thrice) = (0, 0, 0, 0);
        let (mut middle_once, mut middle_thrice) = (0, 0);
        let (mut partials, mut partial_before, mut partial_once, mut partial_thrice) = (0, 0, 0, 0);
        for (word, latin) in &own {
            for typed in &latin[1..] {
                others += 1;
                other_before += usize::from(shown(&empty, typed, word));
                other_once += usize::from(shown(&once, typed, word));
                other_thrice += usize::from(shown(&thrice, typed, word));
                middle_once += usize::from(bar(&once, typed)[0] == *word);
                middle_thrice += usize::from(bar(&thrice, typed)[0] == *word);
            }
            let typed = &latin[0][..(latin[0].len() * 3).div_ceil(5)];
            partials += 1;
            partial_before += usize::from(shown(&empty, typed, word));
            partial_once += usize::from(shown(&once, typed, word));
            partial_thrice += usize::from(shown(&thrice, typed, word));
        }
        // Harm.
        let mut rows = 0;
        let mut shown_in = [0usize; 3];
        let mut first_in = [0usize; 3];
        let mut own_middle = [0usize; 3];
        for (word, latin) in &known {
            for typed in latin.iter() {
                rows += 1;
                for (i, e) in [&empty, &once, &thrice].into_iter().enumerate() {
                    let bar = bar(e, typed);
                    shown_in[i] += usize::from(bar.iter().any(|w| w == word));
                    first_in[i] += usize::from(bar[0] == *word);
                    own_middle[i] += usize::from(is_own(&bar[0]));
                }
            }
        }
        let mut english = [0usize; 3];
        let mut english_own = [0usize; 3];
        for (context, intended) in &typos {
            for (i, e) in [&empty, &once, &thrice].into_iter().enumerate() {
                let bar = bar(e, context);
                english[i] += usize::from(bar.first() == Some(intended));
                english_own[i] += usize::from(bar.first().is_some_and(|w| is_own(w)));
            }
        }
        println!(
            "{id}: {} own words | other spellings {others}: shown {other_before} -> {other_once} (used once) / \
             {other_thrice} (thrice), in the middle {middle_once} / {middle_thrice} | first letters {partials}: shown \
             {partial_before} -> {partial_once} / {partial_thrice} | lexicon words {rows} rows: shown {shown_in:?}, \
             first {first_in:?}, an own word in the middle {own_middle:?} | English typos {}: middle right \
             {english:?}, an own word {english_own:?}",
            own.len(),
            typos.len()
        );
    }
}
