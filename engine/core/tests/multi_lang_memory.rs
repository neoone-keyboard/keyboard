//! The engine's memory with 1 to `MAX_ACTIVE` languages active, on the real packs, as the
//! keyboard opens them (every installed pack loaded, the user's choice active): the heap after
//! opening, and its peak while typing English, Manglish, Hinglish and Assamese words and
//! native ones. Lexicons and models are memory-mapped and read only, so their pages are clean
//! and iOS doesn't count them in the extension's footprint; the heap is what counts. The
//! mapped files' sizes are printed beside it.
//! One test in this file, so that the counting allocator sees nothing else.
//! `cargo test --release -p mlengine-core --test multi_lang_memory -- --ignored --nocapture`

use std::alloc::{GlobalAlloc, Layout as AllocLayout, System};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicIsize, Ordering};

use mlengine_core::lang::{LangId, MAX_ACTIVE};
use mlengine_core::pack::Pack;
use mlengine_core::{Engine, Layout, Paths};

struct Counting;

static LIVE: AtomicIsize = AtomicIsize::new(0);
static PEAK: AtomicIsize = AtomicIsize::new(0);

fn grew(by: isize) {
    let now = LIVE.fetch_add(by, Ordering::Relaxed) + by;
    PEAK.fetch_max(now, Ordering::Relaxed);
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: AllocLayout) -> *mut u8 {
        grew(layout.size() as isize);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: AllocLayout) {
        LIVE.fetch_sub(layout.size() as isize, Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: AllocLayout, new_size: usize) -> *mut u8 {
        grew(new_size as isize - layout.size() as isize);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn mb(bytes: isize) -> String {
    format!("{:.1} MB", bytes as f64 / 1_048_576.0)
}

/// The size of the files the keyboard maps for these packs (and English).
fn mapped(packs: &[&str]) -> u64 {
    let en = root().join("apps/ios/KeyboardExtension/Lexicon/en.fst");
    let mut total = std::fs::metadata(en).map_or(0, |m| m.len());
    for id in packs {
        let dir = root().join(format!("packs/{id}.neopack"));
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            total += entry.metadata().map_or(0, |m| m.len());
        }
    }
    total
}

/// What a user of these languages types: words of each, letter by letter, then a space.
fn typing(active: &[&str], engine: &Engine) -> Vec<(String, Layout)> {
    let mut words: Vec<(&str, Option<&str>)> = Vec::new();
    for id in active {
        let latin: &[&str] = match *id {
            "en" => &["the", "because", "tomorrow", "beautiful", "teh"],
            "ml" => &["ente", "njan", "veettil", "sukhamano", "kazhichille"],
            "hi" => &["mera", "kya", "nahi", "samjhna", "accha"],
            "as" => &["moi", "tumi", "manuh", "kotha", "bhal"],
            _ => &[],
        };
        words.extend(latin.iter().map(|w| (*w, None)));
        let native: &[&str] = match *id {
            "ml" => &["എന്റെ", "വീട്ടിൽ"],
            "hi" => &["मेरा", "नमस्ते"],
            "as" => &["অসমীয়া", "মানুহ"],
            _ => &[],
        };
        words.extend(native.iter().map(|w| (*w, Some(*id))));
    }
    let mut inputs = Vec::new();
    for (word, native) in words {
        let layout = native.map_or(Layout::Latin, |id| {
            Layout::Native(engine.lang_of_id(id).unwrap())
        });
        let chars: Vec<char> = word.chars().collect();
        for end in 1..=chars.len() {
            let typed: String = chars[..end].iter().collect();
            inputs.push((format!("I said {typed}"), layout));
        }
        inputs.push((format!("{word} "), layout));
    }
    inputs
}

#[test]
#[ignore = "measurement: run in release with --nocapture"]
fn memory_by_active_languages() {
    // (installed packs, active languages)
    let cases: &[(&[&str], &[&str])] = &[
        (&["ml"], &["en"]),
        (&["ml"], &["en", "ml"]),
        (&["ml", "hi"], &["en", "ml", "hi"]),
        (&["ml", "hi", "as"], &["en", "ml", "hi", "as"]),
        // Installed but not active: what loading every installed pack costs.
        (&["ml", "hi", "as"], &["en", "ml"]),
        (&["ml", "hi", "as"], &["ml", "hi", "as"]),
    ];
    println!("installed | active | heap after open | heap peak while typing | mapped files");
    let mut worst = 0isize;
    for (installed, active) in cases {
        if active.len() > MAX_ACTIVE {
            println!("{installed:?} | {active:?} | more than MAX_ACTIVE ({MAX_ACTIVE}), skipped");
            continue;
        }
        let before = LIVE.load(Ordering::Relaxed);
        let packs: Vec<Pack> = installed
            .iter()
            .map(|id| Pack::open(&root().join(format!("packs/{id}.neopack"))).unwrap())
            .collect();
        let en = root().join("apps/ios/KeyboardExtension/Lexicon/en.fst");
        let mut engine = Engine::open(&Paths::with_packs(Some(en), &packs, None));
        let ids: Vec<LangId> = active.iter().map(|id| LangId((*id).into())).collect();
        engine.set_active(&ids).unwrap();
        engine.set_max_suggestions(10);
        let open = LIVE.load(Ordering::Relaxed) - before;
        let inputs = typing(active, &engine);
        PEAK.store(LIVE.load(Ordering::Relaxed), Ordering::Relaxed);
        for _ in 0..2 {
            for (context, layout) in &inputs {
                let _ = engine.suggest(context, *layout, true);
            }
        }
        let peak = PEAK.load(Ordering::Relaxed) - before;
        worst = worst.max(peak);
        println!(
            "{installed:?} | {active:?} | {} | {} | {}",
            mb(open),
            mb(peak),
            mb(mapped(installed) as isize)
        );
        drop(engine);
        drop(packs);
    }
    // The extension sheds its engine at 32 MB (`MemoryBudget.keyboardExtension`), and the rest
    // of the extension (UI, Swift runtime, emoji) takes about 15 MB of it.
    assert!(worst < 16 * 1_048_576, "engine heap peak {}", mb(worst));
}
