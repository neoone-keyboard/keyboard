//! Recorded suggestion bars for a pack other than Malayalam (`make golden-check-packs`): what
//! `golden_dump.rs` is for Malayalam, driven by the pack's id. Every bar the engine gives over
//! inputs from the pack's own eval data is written to a file, so a ranking change shows which
//! suggestions moved and not only an accuracy score.
//!
//! GOLDEN_PACKS names the packs (ids, space separated; `packs/<id>.neopack`). Each is opened
//! with the app's English lexicon beside it and every file its manifest lists, as on a phone
//! with that one pack. The inputs, from `data/eval/<id>/`:
//!
//! - `translit-human.tsv` (`context<TAB>latin<TAB>native`, required): WORDS rows (default 300)
//!   taken evenly through the file, so every section of it is covered. Section `translit-latin`
//!   types each Latin word on QWERTY; `translit-native` types every third one's native word on
//!   the pack's own layout.
//! - every `next-*` file that isn't a `.tune.` split, when the pack has any (Hindi does): the
//!   first SENTENCES lines (default 30), the first column of each. A line in the pack's script is
//!   typed on its layout, any other on QWERTY. The section is the file's name.
//! - `learn`: a fresh engine picks each of the first LEARN (default 60) of those rows' native
//!   words for its Latin spelling, and is asked after the space and again for the same
//!   spelling, so next words and learned words are covered. The words are spread over the set
//!   like the rest.
//!
//! Every prefix that ends at a character boundary is asked. Each line is
//! `section \t layout \t input \t candidates \t vowel signs`, with candidates as
//! `text|kind:language|score` (score to 3 places). The file is `build/golden/packs/<id>.tsv`
//! (OUT_DIR sets the folder).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use mlengine_core::pack::Pack;
use mlengine_core::{Engine, Layout, Paths};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn engine(id: &str) -> Engine {
    let pack = Pack::open(&root().join(format!("packs/{id}.neopack")))
        .unwrap_or_else(|e| panic!("packs/{id}.neopack: {e}"));
    let en = root().join("apps/ios/KeyboardExtension/Lexicon/en.fst");
    Engine::open(&Paths::with_packs(Some(en), &[pack], None))
}

fn setting(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

/// The rows of a file: no comments, no blank lines.
fn rows(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(str::to_string)
        .collect()
}

/// `(latin, native)` of `count` rows of the pack's held-out set, evenly spaced through it.
fn pairs(dir: &Path, count: usize) -> Vec<(String, String)> {
    let all: Vec<(String, String)> = rows(&dir.join("translit-human.tsv"))
        .iter()
        .filter_map(|l| {
            let mut f = l.split('\t').skip(1);
            Some((f.next()?.to_string(), f.next()?.to_string()))
        })
        .filter(|(latin, native)| !latin.is_empty() && !native.is_empty())
        .collect();
    let count = count.min(all.len());
    (0..count).map(|i| all[i * all.len() / count].clone()).collect()
}

/// The pack's sentence files, by name: `next-*` without the `.tune.` splits.
fn sentence_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            name.starts_with("next-") && !name.contains(".tune.")
        })
        .collect();
    files.sort();
    files
}

/// The pack's own layout for text with a letter outside ASCII, QWERTY for the rest.
fn layout_for(e: &Engine, id: &str, text: &str) -> Layout {
    if text.chars().any(|c| !c.is_ascii() && c.is_alphabetic()) {
        Layout::Native(e.lang_of_id(id).expect("the pack's language"))
    } else {
        Layout::Latin
    }
}

fn row(out: &mut String, section: &str, e: &Engine, ctx: &str, layout: Layout) {
    let s = e.suggest(ctx, layout, true);
    let shown: Vec<String> = s
        .candidates
        .iter()
        .map(|c| {
            let lang = e.lang_id(c.lang).map_or("?".into(), |l| l.0);
            format!("{}|{:?}:{lang}|{:.3}", c.text, c.kind, c.score)
        })
        .collect();
    let signs: String = s.vowel_signs.iter().collect();
    let layout = match layout {
        Layout::Latin => "Latin",
        Layout::Native(_) => "Native",
    };
    writeln!(out, "{section}\t{layout}\t{ctx}\t{}\t{signs}", shown.join(" ")).unwrap();
}

/// A row for every prefix of `text`.
fn prefixes(out: &mut String, section: &str, e: &Engine, text: &str, layout: Layout) {
    for (i, c) in text.char_indices() {
        row(out, section, e, &text[..i + c.len_utf8()], layout);
    }
}

fn dump(id: &str) -> String {
    let dir = root().join("data/eval").join(id);
    let picked = pairs(&dir, setting("WORDS", 300));
    let mut out = String::new();

    let e = engine(id);
    for (latin, _) in &picked {
        prefixes(&mut out, "translit-latin", &e, latin, Layout::Latin);
    }
    for (_, native) in picked.iter().step_by(3) {
        let layout = layout_for(&e, id, native);
        prefixes(&mut out, "translit-native", &e, native, layout);
    }
    for file in sentence_files(&dir) {
        let section = file.file_name().unwrap().to_string_lossy().to_string();
        for line in rows(&file).iter().take(setting("SENTENCES", 30)) {
            let text = line.split('\t').next().unwrap_or("");
            prefixes(&mut out, &section, &e, text, layout_for(&e, id, text));
        }
    }

    // Learning: a fresh engine picks each native word for its Latin spelling.
    let e = engine(id);
    let learn = setting("LEARN", 60).clamp(1, picked.len().max(1));
    for (latin, native) in picked.iter().step_by((picked.len() / learn).max(1)).take(learn) {
        e.note_used("", native, Layout::Latin);
        e.note_committed(native);
        row(&mut out, "learn", &e, &format!("{native} "), Layout::Latin);
        row(&mut out, "learn", &e, latin, Layout::Latin);
    }
    out
}

#[test]
#[ignore]
fn golden_dump_packs() {
    let ids = std::env::var("GOLDEN_PACKS").expect("GOLDEN_PACKS: the pack ids to record");
    let out_dir = std::env::var_os("OUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("build/golden/packs"));
    std::fs::create_dir_all(&out_dir).unwrap();
    for id in ids.split_whitespace() {
        let out = dump(id);
        let path = out_dir.join(format!("{id}.tsv"));
        std::fs::write(&path, &out).unwrap();
        println!("{}: {} bars", path.display(), out.lines().count());
    }
}
