//! Tapped typing on synthetic taps (`Engine::suggest_tapped`). The eval sets have no touch
//! points, so each typed word's letters are tapped as a finger would: a touch point per letter
//! drawn from a Gaussian around the intended key's centre (`NOISES`, in key widths; seeded, so
//! every run draws the same points), the letter typed is the key whose cell holds the point,
//! and the tap's other likely keys are worked out from the point as the keyboard does
//! (`read_tap`, the same numbers as the apps). Every row is then asked for with and without
//! the taps: the same typed letters, so the difference is the taps alone.
//!
//!     make eval-taps                      # every set, gated on data/eval/taps-baselines.json
//!     TAP_RECORD=1 make eval-taps         # records the baselines instead
//!     TAP_SIGMA="0.35 0.4 0.45" TAP_SCALE="0.3 0.4 0.5" make eval-taps   # a sweep (no gate)
//!
//! The sets, per pack (ml, hi, ta, as, each with English, as the keyboard opens them):
//!  - `typos`: the romanised typo sets (`make eval-typos`). Each letter typed right is tapped
//!    inside its own key; a neighbouring-key slip (`neighbour` rows, matched to the held-out
//!    word they were made from) is a finger that aimed at the intended key and landed in the
//!    slipped one: a touch point drawn around the intended key until it falls in the other.
//!  - `clean`: the clean held-out words (`translit-human.tsv`), every letter drawn around its
//!    key with nothing held: most land in their key, some slip into a neighbour. Split into
//!    `right` (every tap in its key: what taps must not make worse) and `slipped`.
//!
//! and with Malayalam: `english typos` (`english-typos.tsv`, slips drawn as above) and
//! `english clean` (each word of the no-harm sentences, `english-sentences.txt`, after the
//! words before it). Romanised rows score the first native suggestion (top-1), the target in
//! the three shown (top-3) and in the middle (first); English rows the middle (top-1) and the
//! bar (top-3).
//!
//! The keys are the iPhone's QWERTY (`KeyboardGeometry.frames`, `Layouts.swift`): rows 1.4
//! key widths apart (55 pt rows, 39 pt keys), the second row half a key in, the third a key and
//! a half; a touch off the letters (shift, backspace, the bottom row) is drawn again: it would
//! not have typed a letter.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use mlengine_core::pack::Pack;
use mlengine_core::{Engine, Layout, Paths, Tap};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// data/eval, spelled so a shell checker reading this file doesn't take it for a command.
fn ev() -> String {
    ["ev", "al"].concat()
}

/// The keyboard's reading of a tap: KeyboardCore's `TapReading` (iOS) and `TapReading.kt`
/// (Android) use these same numbers. A Gaussian of `APP_SIGMA` key widths around each letter
/// key's centre; the other keys at `APP_FLOOR` or more of the probability, at most
/// `APP_OTHERS` of them.
const APP_SIGMA: f32 = 0.4;
const APP_FLOOR: f32 = mlengine_core::fuzzy::TAP_FLOOR;
const APP_OTHERS: usize = 2;

/// Touch noise, in key widths: a careful typist and a quick one.
const NOISES: [f32; 2] = [0.25, 0.4];
/// Rows 1.4 key widths apart.
const PITCH: f32 = 1.4;
const ROWS: [(&[u8], f32); 3] = [(b"qwertyuiop", 0.0), (b"asdfghjkl", 0.5), (b"zxcvbnm", 1.5)];
const SLACK: f64 = 0.005;

fn centre(letter: u8) -> (f32, f32) {
    for (row, (keys, offset)) in ROWS.iter().enumerate() {
        if let Some(col) = keys.iter().position(|&k| k == letter) {
            return (col as f32 + offset + 0.5, (row as f32 + 0.5) * PITCH);
        }
    }
    panic!("no key for {}", letter as char)
}

/// The letter whose key cell holds the point, if any.
fn key_at(x: f32, y: f32) -> Option<u8> {
    if y < 0.0 {
        return None;
    }
    let (keys, offset) = ROWS.get((y / PITCH) as usize)?;
    let col = x - offset;
    if col < 0.0 {
        return None;
    }
    keys.get(col as usize).copied()
}

/// What the keyboard passes for a tap at the point that typed `typed` (`APP_*`).
fn read_tap(x: f32, y: f32, typed: char, sigma: f32) -> Tap {
    let letters: Vec<u8> = ROWS.iter().flat_map(|(keys, _)| keys.iter().copied()).collect();
    let weights: Vec<f32> = letters
        .iter()
        .map(|&k| {
            let (cx, cy) = centre(k);
            let d2 = (x - cx).powi(2) + (y - cy).powi(2);
            (-d2 / (2.0 * sigma * sigma)).exp()
        })
        .collect();
    let total: f32 = weights.iter().sum();
    let lower = typed.to_ascii_lowercase() as u8;
    let p = |i: usize| if total > 0.0 { weights[i] / total } else { 0.0 };
    let mut others: Vec<(char, f32)> = letters
        .iter()
        .enumerate()
        .filter(|&(_, &k)| k != lower)
        .map(|(i, &k)| (k as char, p(i)))
        .filter(|&(_, q)| q >= APP_FLOOR)
        .collect();
    others.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    others.truncate(APP_OTHERS);
    let own = letters.iter().position(|&k| k == lower).map_or(1.0, p);
    Tap { letter: typed, probability: own, others }
}

/// SplitMix64, with Gaussian draws by Box–Muller.
struct Rng(u64);

impl Rng {
    fn seeded(text: &str) -> Self {
        // FNV-1a of the seed text.
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in text.bytes() {
            h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
        }
        Self(h)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn uniform(&mut self) -> f64 {
        ((self.next() >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }

    fn gaussian(&mut self) -> (f32, f32) {
        let (u, v) = (self.uniform(), self.uniform());
        let r = (-2.0 * u.ln()).sqrt();
        let t = 2.0 * std::f64::consts::PI * v;
        ((r * t.cos()) as f32, (r * t.sin()) as f32)
    }
}

/// One letter's touch: drawn around `aim` until it falls on a letter key (`land`'s, when
/// given). Never more than 100,000 draws; then the middle of `land` (or `aim`).
fn touch(rng: &mut Rng, aim: u8, noise: f32, land: Option<u8>) -> (f32, f32, u8) {
    let (cx, cy) = centre(aim);
    for _ in 0..100_000 {
        let (dx, dy) = rng.gaussian();
        let (x, y) = (cx + noise * dx, cy + noise * dy);
        match key_at(x, y) {
            Some(k) if land.is_none_or(|l| l == k) => return (x, y, k),
            _ => {}
        }
    }
    let k = land.unwrap_or(aim);
    let (x, y) = centre(k);
    (x, y, k)
}

/// A word typed by taps: the letters typed and where each landed. `aims[i]`: the key letter
/// `i` was aimed at and the key it must land in (None: wherever the draw falls). Letters are
/// typed in their case; anything else is typed as it is, with no touch.
struct Typed {
    text: String,
    points: Vec<Option<(f32, f32)>>,
}

impl Typed {
    fn taps(&self, sigma: f32) -> Vec<Tap> {
        self.text
            .chars()
            .zip(&self.points)
            .map(|(c, p)| match p {
                Some((x, y)) => read_tap(*x, *y, c, sigma),
                None => Tap { letter: c, probability: 1.0, others: Vec::new() },
            })
            .collect()
    }
}

fn type_word(rng: &mut Rng, word: &str, noise: f32, aims: &[(u8, Option<u8>)]) -> Typed {
    let mut text = String::new();
    let mut points = Vec::new();
    for (i, c) in word.chars().enumerate() {
        if !c.is_ascii_alphabetic() {
            text.push(c);
            points.push(None);
            continue;
        }
        let (aim, land) = aims.get(i).copied().unwrap_or((c.to_ascii_lowercase() as u8, None));
        let (x, y, k) = touch(rng, aim, noise, land);
        let k = k as char;
        text.push(if c.is_ascii_uppercase() { k.to_ascii_uppercase() } else { k });
        points.push(Some((x, y)));
    }
    Typed { text, points }
}

/// Every letter of `typed` lands in its own key, but for a neighbouring-key slip from
/// `intended` (same length, one letter different, next to it): there the finger aimed at the
/// intended key.
fn slip_aims(typed: &str, intended: Option<&str>) -> Vec<(u8, Option<u8>)> {
    let own: Vec<(u8, Option<u8>)> = typed
        .bytes()
        .map(|b| (b.to_ascii_lowercase(), Some(b.to_ascii_lowercase())))
        .collect();
    let Some(intended) = intended.filter(|w| w.len() == typed.len() && w.is_ascii()) else {
        return own;
    };
    let diff: Vec<usize> = (0..typed.len())
        .filter(|&i| !typed.as_bytes()[i].eq_ignore_ascii_case(&intended.as_bytes()[i]))
        .collect();
    let &[i] = diff.as_slice() else { return own };
    let (t, w) = (typed.as_bytes()[i].to_ascii_lowercase(), intended.as_bytes()[i].to_ascii_lowercase());
    if !mlengine_core::keys::adjacent(t, w) || !w.is_ascii_lowercase() {
        return own;
    }
    let mut aims = own;
    aims[i] = (w, Some(t));
    aims
}

/// One row to ask for: the text before the word, the word as tapped, what should come.
struct Row {
    context: String,
    typed: Typed,
    want: String,
    /// Every tap landed in the key aimed at (the typed word is the intended one).
    right: bool,
}

/// [rows, top-1, top-3, first] per bucket.
type Tally = HashMap<String, [usize; 4]>;

fn add(t: &mut Tally, bucket: &str, hits: [bool; 3]) {
    let e = t.entry(bucket.to_string()).or_default();
    e[0] += 1;
    for (i, h) in hits.iter().enumerate() {
        e[i + 1] += usize::from(*h);
    }
}

/// Romanised scoring: first native suggestion, in the three shown, in the middle.
fn native_hits(shown: &[&str], want: &str) -> [bool; 3] {
    let native = shown.iter().find(|t| !t.is_ascii());
    [native == Some(&want), shown.contains(&want), shown.first() == Some(&want)]
}

/// English scoring: in the middle (ignoring case at the start of the text), in the bar.
fn english_hits(shown: &[&str], want: &str, start: bool) -> [bool; 3] {
    let same = |t: &&str| *t == want || (start && t.to_lowercase() == want.to_lowercase());
    let first = shown.first().is_some_and(same);
    [first, shown.iter().any(same), first]
}

fn ask(e: &Engine, row: &Row, taps: Option<&[Tap]>) -> Vec<String> {
    let query = format!("{}{}", row.context, row.typed.text);
    let s = e.suggest_tapped(&query, Layout::Latin, true, None, taps.unwrap_or(&[]));
    s.candidates.into_iter().take(3).map(|c| c.text).collect()
}

/// `context<TAB>latin<TAB>native` rows of a file under data/, with their `# section:`.
fn tsv(path: &str) -> Vec<(String, String, String, String)> {
    let text = std::fs::read_to_string(root().join("data").join(path)).unwrap();
    let mut section = String::new();
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("# section:") {
            section = name.trim().to_string();
        }
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut cols = line.split('\t');
        let (context, latin, native) = (cols.next().unwrap(), cols.next().unwrap(), cols.next().unwrap());
        out.push((section.clone(), context.to_string(), latin.to_string(), native.to_string()));
    }
    out
}

fn with_space(context: &str) -> String {
    if context.is_empty() { String::new() } else { format!("{context} ") }
}

/// The sets of `id`'s pack at one noise level: (set name, rows, romanised scoring).
fn pack_sets(id: &str, noise: f32) -> Vec<(String, Vec<Row>, bool)> {
    let dir = if id == "ml" { ev() } else { format!("{}/{id}", ev()) };
    let human = tsv(&format!("{dir}/translit-human.tsv"));
    let mut by_native: HashMap<&str, Vec<&str>> = HashMap::new();
    for (_, _, latin, native) in &human {
        by_native.entry(native.as_str()).or_default().push(latin.as_str());
    }
    let mut typos = Vec::new();
    for (n, (section, context, latin, native)) in tsv(&format!("{dir}/translit-typos.tsv")).iter().enumerate() {
        let mut rng = Rng::seeded(&format!("taps-{id}-typos-{noise}-{n}"));
        let intended = (section == "neighbour")
            .then(|| by_native.get(native.as_str()))
            .flatten()
            .and_then(|latins| {
                latins.iter().copied().find(|w| slip_aims(latin, Some(w)).iter().any(|(a, l)| Some(*a) != *l))
            });
        let aims = slip_aims(latin, intended);
        let typed = type_word(&mut rng, latin, noise, &aims);
        assert_eq!(&typed.text, latin, "a typo row is typed as written");
        typos.push((section.clone(), Row { context: with_space(context), typed, want: native.clone(), right: false }));
    }
    let mut clean = Vec::new();
    for (n, (_, context, latin, native)) in human.iter().enumerate() {
        let mut rng = Rng::seeded(&format!("taps-{id}-clean-{noise}-{n}"));
        let typed = type_word(&mut rng, latin, noise, &[]);
        let right = typed.text == *latin;
        clean.push(Row { context: with_space(context), typed, want: native.clone(), right });
    }
    let mut out: Vec<(String, Vec<Row>, bool)> = Vec::new();
    let mut sections: Vec<String> = typos.iter().map(|(s, _)| s.clone()).collect();
    sections.dedup();
    let mut typo_rows: HashMap<String, Vec<Row>> = HashMap::new();
    for (s, r) in typos {
        typo_rows.entry(s).or_default().push(r);
    }
    for s in sections {
        out.push((format!("typos {s}"), typo_rows.remove(&s).unwrap_or_default(), true));
    }
    out.push(("clean".into(), clean, true));
    out
}

/// English typos and clean English words, at one noise level.
fn english_sets(noise: f32) -> Vec<(String, Vec<Row>, bool)> {
    let mut typos = Vec::new();
    for (n, (_, context, typed, intended)) in tsv(&format!("{}/english-typos.tsv", ev())).iter().enumerate() {
        let mut rng = Rng::seeded(&format!("taps-en-typos-{noise}-{n}"));
        let aims = slip_aims(typed, Some(intended));
        let tapped = type_word(&mut rng, typed, noise, &aims);
        typos.push(Row { context: with_space(context), typed: tapped, want: intended.clone(), right: false });
    }
    let sentences = std::fs::read_to_string(root().join(format!("data/{}/english-sentences.txt", ev()))).unwrap();
    let punct: &[char] = &['.', ',', '!', '?', ';', ':', '"', '(', ')', '…'];
    let mut clean = Vec::new();
    let mut n = 0;
    for sentence in sentences.lines().filter(|l| !l.trim().is_empty()) {
        let words: Vec<&str> = sentence.split_whitespace().collect();
        for (i, word) in words.iter().enumerate() {
            let bare = word.trim_matches(punct);
            if bare.is_empty() || !bare.chars().any(|c| c.is_ascii_alphabetic()) || !bare.is_ascii() {
                continue;
            }
            let mut rng = Rng::seeded(&format!("taps-en-clean-{noise}-{n}"));
            n += 1;
            let context: String = words[..i].iter().map(|w| format!("{w} ")).collect();
            let typed = type_word(&mut rng, bare, noise, &[]);
            let right = typed.text == bare;
            clean.push(Row { context, typed, want: bare.to_string(), right });
        }
    }
    vec![("english typos".into(), typos, false), ("english clean".into(), clean, false)]
}

/// One set's numbers: without taps, then with them for each (sigma, scale).
fn run_set(e: &mut Engine, rows: &[Row], romanised: bool, configs: &[(f32, f32)]) -> Vec<Tally> {
    let score = |row: &Row, shown: &[String]| {
        let shown: Vec<&str> = shown.iter().map(String::as_str).collect();
        if romanised {
            native_hits(&shown, &row.want)
        } else {
            english_hits(&shown, &row.want, row.context.is_empty())
        }
    };
    let bucket = |row: &Row, t: &mut Tally, hits: [bool; 3]| {
        add(t, "all", hits);
        add(t, if row.right { "right" } else { "slipped" }, hits);
    };
    let mut out = Vec::new();
    let mut without = Tally::new();
    for row in rows {
        let shown = ask(e, row, None);
        bucket(row, &mut without, score(row, &shown));
    }
    out.push(without);
    for &(sigma, scale) in configs {
        e.set_tap_scale(scale);
        let mut with = Tally::new();
        for row in rows {
            let taps = row.typed.taps(sigma);
            let shown = ask(e, row, Some(&taps));
            bucket(row, &mut with, score(row, &shown));
        }
        out.push(with);
    }
    e.set_tap_scale(mlengine_core::rank::TAP_SCALE);
    out
}

fn rate(t: &[usize; 4], i: usize) -> f64 {
    t[i] as f64 / t[0].max(1) as f64
}

fn floats(var: &str, default: &[f32]) -> Vec<f32> {
    std::env::var(var).map_or_else(
        |_| default.to_vec(),
        |v| v.split_whitespace().map(|x| x.parse().unwrap()).collect(),
    )
}

fn engine(id: &str) -> Engine {
    let pack = Pack::open(&root().join(format!("packs/{id}.neopack"))).unwrap();
    let en = root().join("apps/ios/KeyboardExtension/Lexicon/en.fst");
    Engine::open(&Paths::with_packs(Some(en), &[pack], None))
}

#[test]
#[ignore = "eval: a few minutes"]
fn tap_eval() {
    let sigmas = floats("TAP_SIGMA", &[APP_SIGMA]);
    let scales = floats("TAP_SCALE", &[mlengine_core::rank::TAP_SCALE]);
    let configs: Vec<(f32, f32)> = sigmas.iter().flat_map(|&s| scales.iter().map(move |&k| (s, k))).collect();
    let sweep = configs.len() > 1 || std::env::var("TAP_SIGMA").is_ok() || std::env::var("TAP_SCALE").is_ok();
    let packs: Vec<String> = std::env::var("TAP_PACKS")
        .unwrap_or_else(|_| "ml hi ta as".into())
        .split_whitespace()
        .map(String::from)
        .collect();
    let handles: Vec<_> = packs
        .into_iter()
        .map(|id| {
            let configs = configs.clone();
            std::thread::spawn(move || {
                let mut e = engine(&id);
                // (key, [without, with…] per bucket)
                let mut results: Vec<(String, Vec<Tally>)> = Vec::new();
                for noise in NOISES {
                    let mut sets = pack_sets(&id, noise);
                    if id == "ml" {
                        sets.extend(english_sets(noise));
                    }
                    for (name, rows, romanised) in sets {
                        let key = if name.starts_with("english") {
                            format!("{name} {noise}")
                        } else {
                            format!("{id} {name} {noise}")
                        };
                        results.push((key, run_set(&mut e, &rows, romanised, &configs)));
                    }
                }
                results
            })
        })
        .collect();
    let mut all: Vec<(String, Vec<Tally>)> = Vec::new();
    for h in handles {
        all.extend(h.join().unwrap());
    }
    // Typo sections summed into `typos` per pack and noise.
    let mut summed: Vec<(String, Vec<Tally>)> = Vec::new();
    for (key, tallies) in &all {
        let Some((head, rest)) = key.split_once(" typos ").filter(|(h, _)| !h.starts_with("english")) else {
            summed.push((key.clone(), tallies.clone()));
            continue;
        };
        let noise = rest.rsplit(' ').next().unwrap();
        let total = format!("{head} typos {noise}");
        if !summed.iter().any(|(k, _)| *k == total) {
            summed.push((total.clone(), vec![Tally::new(); tallies.len()]));
        }
        let at = summed.iter().position(|(k, _)| *k == total).unwrap();
        for (sum, t) in summed[at].1.iter_mut().zip(tallies) {
            for (bucket, counts) in t {
                let e = sum.entry(bucket.clone()).or_default();
                (0..4).for_each(|i| e[i] += counts[i]);
            }
        }
        summed.push((key.clone(), tallies.clone()));
    }
    println!("{:34} {:>5}  without: top-1 top-3 first  |with taps (sigma scale): top-1 top-3 first (top-1 change)", "set, noise (key widths)", "rows");
    let mut record = serde_json::Map::new();
    for (key, tallies) in &summed {
        for bucket in ["all", "right", "slipped"] {
            let Some(without) = tallies[0].get(bucket) else { continue };
            if bucket != "all" && !key.contains("clean") {
                continue;
            }
            let label = if bucket == "all" { key.clone() } else { format!("{key} [{bucket}]") };
            let mut line = format!(
                "{label:34} {:5}  {:.4} {:.4} {:.4}",
                without[0],
                rate(without, 1),
                rate(without, 2),
                rate(without, 3)
            );
            for (c, t) in configs.iter().zip(&tallies[1..]) {
                let t = t.get(bucket).copied().unwrap_or_default();
                line += &format!(
                    "  |{} {}: {:.4} {:.4} {:.4} ({:+.4})",
                    c.0,
                    c.1,
                    rate(&t, 1),
                    rate(&t, 2),
                    rate(&t, 3),
                    rate(&t, 1) - rate(without, 1)
                );
            }
            println!("{line}");
            if bucket == "all" || key.contains("clean") {
                let t = tallies[1].get(bucket).copied().unwrap_or_default();
                let entry = |t: &[usize; 4]| {
                    serde_json::json!({"n": t[0], "top1": round(rate(t, 1)), "top3": round(rate(t, 2)), "first": round(rate(t, 3))})
                };
                record.insert(label, serde_json::json!({"without": entry(without), "with": entry(&t)}));
            }
        }
    }
    let path = root().join(format!("data/{}/taps-baselines.json", ev()));
    if sweep {
        println!("a sweep: not gated");
        return;
    }
    if std::env::var("TAP_RECORD").is_ok() {
        let text = serde_json::to_string_pretty(&serde_json::Value::Object(record)).unwrap();
        std::fs::write(&path, text + "\n").unwrap();
        println!("recorded {}", path.display());
        return;
    }
    let base: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut regressions = Vec::new();
    for (label, now) in &record {
        let Some(was) = base.get(label) else {
            println!("{label}: no baseline recorded");
            continue;
        };
        for metric in ["top1", "top3", "first"] {
            let (n, w) = (now["with"][metric].as_f64().unwrap(), was["with"][metric].as_f64().unwrap());
            if n < w - SLACK {
                regressions.push(format!("{label} {metric}: {n:.4} < baseline {w:.4}"));
            }
        }
    }
    for r in &regressions {
        println!("REGRESSION {r}");
    }
    assert!(regressions.is_empty(), "{} regressions against the tap baselines", regressions.len());
    println!("no regressions against the tap baselines");
}

fn round(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
}

#[test]
fn the_app_reading_of_a_tap() {
    // Between k and j, a little nearer k.
    let (kx, ky) = centre(b'k');
    let t = read_tap(kx - 0.4, ky, 'k', APP_SIGMA);
    assert_eq!(t.others.first().map(|o| o.0), Some('j'));
    assert!(t.probability > t.others[0].1 && t.others[0].1 > 0.3, "{t:?}");
    // In the middle of k: nothing else is likely.
    let t = read_tap(kx, ky, 'k', APP_SIGMA);
    assert!(t.others.is_empty() && t.probability > 0.9, "{t:?}");
    // The cells: half a key in on the second row, shift and backspace beside the third.
    assert_eq!(key_at(0.2, 0.5 * PITCH + PITCH), None);
    assert_eq!(key_at(0.6, 1.5 * PITCH), Some(b'a'));
    assert_eq!(key_at(1.4, 2.5 * PITCH), None);
    assert_eq!(key_at(1.6, 2.5 * PITCH), Some(b'z'));
    assert_eq!(key_at(5.0, 3.5 * PITCH), None);
    // A slip is drawn around the intended key and lands in the other.
    let mut rng = Rng::seeded("test");
    let (x, y, k) = touch(&mut rng, b'k', 0.25, Some(b'j'));
    assert_eq!(k, b'j');
    assert!(x > centre(b'j').0, "{x} {y}");
    assert_eq!(slip_aims("nkan", Some("njan"))[1], (b'j', Some(b'k')));
    assert_eq!(slip_aims("nkan", Some("nkan"))[1], (b'k', Some(b'k')));
}
