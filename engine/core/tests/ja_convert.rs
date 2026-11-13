//! The Japanese pack's converter on its real data (packs/ja.neopack): conversions a Japanese
//! typist expects, the heap a conversion takes (the files are memory-mapped and clean, so iOS
//! doesn't count them in the keyboard extension's footprint; the heap is what counts) and how
//! long 64 kana take. One measuring test in this file, so the counting allocator sees nothing
//! else while it runs.
//!
//! `cargo test --release -p mlengine-core --test ja_convert -- --ignored --nocapture`

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Instant;

use mlengine_core::convert::Converter;

struct Counting;

static LIVE: AtomicIsize = AtomicIsize::new(0);
static PEAK: AtomicIsize = AtomicIsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let now =
            LIVE.fetch_add(layout.size() as isize, Ordering::Relaxed) + layout.size() as isize;
        PEAK.fetch_max(now, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size() as isize, Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let by = new_size as isize - layout.size() as isize;
        let now = LIVE.fetch_add(by, Ordering::Relaxed) + by;
        PEAK.fetch_max(now, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn pack() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/ja.neopack")
}

fn open() -> Converter {
    let p = pack();
    Converter::open(
        &p.join("kana.json"),
        &p.join("conversion.dict"),
        &p.join("connection.bin"),
    )
    .unwrap()
}

fn mb(bytes: isize) -> String {
    format!("{:.2} MB", bytes as f64 / 1_048_576.0)
}

/// Typed romaji → what a Japanese typist expects, and how far down the candidates it may be
/// (1: first).
const EXPECTED: &[(&str, &str, usize)] = &[
    ("nihongo", "日本語", 1),
    // Second: Mozc's dictionary has フル (full), one of the katakana words it collected from
    // the web, cheaper than 降る after が. Costs adjusted from a corpus are the fix.
    ("kyouhaamegafuru", "今日は雨が降る", 2),
    ("watashihagakuseidesu", "私は学生です", 1),
    ("toukyou", "東京", 1),
    ("arigatou", "ありがとう", 1),
    ("konnnichiha", "こんにちは", 1),
    ("kouennniikimasu", "公園に行きます", 1),
];

#[test]
#[ignore = "real pack data: run in release with --nocapture"]
fn converts_and_measures() {
    let before = LIVE.load(Ordering::Relaxed);
    let started = Instant::now();
    let c = open();
    let open_time = started.elapsed();
    let open_heap = LIVE.load(Ordering::Relaxed) - before;
    println!("open: {open_time:?}, heap {}", mb(open_heap));

    let mut wrong = Vec::new();
    for &(typed, want, within) in EXPECTED {
        let (kana, _) = c.compose(typed, true);
        let conv = c.convert(&kana, 10);
        println!(
            "{typed} → {kana} → {:?} | first {:?}",
            &conv.candidates[..conv.candidates.len().min(5)],
            &conv.first[..conv.first.len().min(5)]
        );
        if !conv.candidates.iter().take(within).any(|c| c == want) {
            wrong.push(format!(
                "{typed}: {:?}, want {want} in the first {within}",
                conv.candidates
            ));
        }
    }

    // 64 kana: the longest text converted in one lattice.
    let long = "きょうはあめがふるのでかさをもっていきますがあしたははれるそうなのでこうえんにいってさんぽをしたいとおもっていますよろしくおねがいします";
    let long: String = long.chars().take(64).collect();
    assert_eq!(long.chars().count(), 64);
    let base = LIVE.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let nodes = mlengine_core::convert::Lattice::new(c.dictionary(), &long).len();
    let mut times = Vec::new();
    let mut best = String::new();
    for _ in 0..20 {
        let t = Instant::now();
        let conv = c.convert(&long, 10);
        times.push(t.elapsed());
        best = conv.candidates[0].clone();
    }
    let peak = PEAK.load(Ordering::Relaxed) - base;
    times.sort();
    println!("64 kana ({nodes} lattice nodes) → {best}");
    println!(
        "64 kana, 10 candidates: median {:?}, max {:?}, heap peak {}",
        times[times.len() / 2],
        times[times.len() - 1],
        mb(peak)
    );
    // The extension sheds its engine at 32 MB, the rest of it takes about 15 MB, and the word
    // engine's own peak is under 9 MB with three packs (multi_lang_memory).
    assert!(peak < 4 * 1_048_576, "conversion heap peak {}", mb(peak));
    assert!(wrong.is_empty(), "{wrong:#?}");
}
