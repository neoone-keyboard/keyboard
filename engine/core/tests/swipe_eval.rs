//! Swipe accuracy on synthetic paths (design `2026-11-06-m4-swipe-design.md` §7). Ignored by
//! default; `make eval-swipe` runs it in release:
//!
//!     cargo test -p mlengine-core --release --test swipe_eval -- --ignored --nocapture
//!
//! Every word of the English sentences and of the Manglish rows (chat golden, and the tune
//! sentences whose Latin and Malayalam have as many words) is drawn as a finger would: each key
//! hit off-centre, corners cut, a little shake, from a fixed seed. Reports top-1 and top-3 per
//! language, Manglish also over the words the lexicon has (a swipe can only give those), and
//! the decode time, and fails if a full run drops below `data/eval/baselines-swipe.json`.
//! `SWIPE_WEIGHTS=align,shape,length` tries other weights, `SWIPE_NOISE=aim,shake,cut` other
//! paths, `SWIPE_EVERY=n` every nth row, `SWIPE_SHOW=1` prints the misses.
//!
//! `swipe_eval_packs` does the same for the other packs (`SWIPE_PACKS`, default `hi as ta`):
//! every Latin word of the pack's held-out set (`data/eval/<id>/translit-human.tsv`) drawn the
//! same way with no text before it, on an engine with English and that pack, scored over all
//! rows and over the words the pack's romanised lexicon has. A pack with no entry in the
//! baselines file is only printed.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use mlengine_core::swipe::{KeyMap, Weights, qwerty};
use mlengine_core::{Engine, Paths};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn engine() -> Engine {
    let dir = root().join("apps/ios/KeyboardExtension");
    let pack = mlengine_core::pack::Pack::open(&dir.join("../../../packs/ml.neopack")).unwrap();
    let mut engine = Engine::open(&{
        let mut paths = Paths::with_packs(Some(dir.join("Lexicon/en.fst")), &[pack], None);
        paths.packs[0].translit = None;
        paths
    });
    if let Ok(w) = std::env::var("SWIPE_WEIGHTS") {
        let v: Vec<f32> = w.split(',').map(|x| x.parse().unwrap()).collect();
        engine.set_swipe_weights(Weights {
            align: v[0],
            shape: v[1],
            length: v[2],
            ..Weights::default()
        });
    }
    engine
}

/// xorshift64*, so the paths are the same on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        ((self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32) / (1u64 << 24) as f32
    }

    fn gauss(&mut self) -> f32 {
        let (u, v) = (self.next().max(1e-7), self.next());
        (-2.0 * u.ln()).sqrt() * (std::f32::consts::TAU * v).cos()
    }
}

/// A finger's path for `word` on keys one unit wide: each key's centre moved by `aim` key
/// widths (Gaussian), corners pulled up to `MAX_CUT` toward their neighbours, then points
/// every quarter key with `shake` of sideways jitter.
fn noisy_path(keys: &KeyMap, word: &str, rng: &mut Rng) -> Vec<(f32, f32)> {
    /// The farthest a corner is cut, in key widths.
    const MAX_CUT: f32 = 0.4;
    // SWIPE_NOISE=aim,shake,cut: for checking the decoder on cleaner or rougher paths.
    let noise: Vec<f32> = std::env::var("SWIPE_NOISE").map_or(vec![0.22, 0.04, 1.0], |v| {
        v.split(',').map(|x| x.parse().unwrap()).collect()
    });
    let (aim, shake, cut) = (noise[0], noise[1], noise[2]);
    let mut targets: Vec<(f32, f32)> = Vec::new();
    for b in word.bytes() {
        let Some(c) = keys.centre(b) else { continue };
        if targets.last().is_some_and(|&l| l == c) {
            continue;
        }
        targets.push(c);
    }
    let mut aimed: Vec<(f32, f32)> = targets
        .iter()
        .map(|&(x, y)| (x + aim * rng.gauss(), y + aim * rng.gauss()))
        .collect();
    for i in 1..aimed.len().saturating_sub(1) {
        let (a, b, c) = (aimed[i - 1], aimed[i], aimed[i + 1]);
        let mid = ((a.0 + c.0) / 2.0, (a.1 + c.1) / 2.0);
        // A finger rounds a corner but still reaches near the key: the bend marks the letter.
        let (dx, dy) = (mid.0 - b.0, mid.1 - b.1);
        let reach = (dx * dx + dy * dy).sqrt();
        let pull = (cut * MAX_CUT * rng.next()).min(reach) / reach.max(1e-6);
        aimed[i] = (b.0 + dx * pull, b.1 + dy * pull);
    }
    let mut path = Vec::new();
    for w in aimed.windows(2) {
        let (a, b) = (w[0], w[1]);
        let length = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        let steps = (length * 4.0).ceil().max(1.0) as usize;
        for s in 0..steps {
            let t = s as f32 / steps as f32;
            path.push((
                a.0 + (b.0 - a.0) * t + shake * rng.gauss(),
                a.1 + (b.1 - a.1) * t + shake * rng.gauss(),
            ));
        }
    }
    path.extend(aimed.last());
    path
}

/// A row to swipe: the text before it, the Latin letters drawn, the word expected.
struct Row {
    before: String,
    drawn: String,
    expected: String,
}

fn letters(word: &str) -> String {
    word.chars()
        .filter(|c| c.is_ascii_alphabetic())
        .collect::<String>()
        .to_lowercase()
}

fn english_rows() -> Vec<Row> {
    let text = std::fs::read_to_string(root().join("data/eval/english-sentences.txt")).unwrap();
    let mut rows = Vec::new();
    for line in text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
    {
        let mut before = String::new();
        for raw in line.split_whitespace() {
            let word = raw.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'');
            if word.chars().count() >= 2
                && word.chars().all(|c| c.is_ascii_alphabetic() || c == '\'')
            {
                rows.push(Row {
                    before: before.clone(),
                    drawn: letters(word),
                    expected: word.to_string(),
                });
            }
            before.push_str(raw);
            before.push(' ');
        }
    }
    rows
}

fn manglish_rows() -> Vec<Row> {
    let mut rows = Vec::new();
    let golden = std::fs::read_to_string(root().join("data/eval/chat-golden.tsv")).unwrap();
    for line in golden.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() >= 3 && letters(f[1]).len() >= 2 {
            let before = if f[0].is_empty() {
                String::new()
            } else {
                format!("{} ", f[0])
            };
            rows.push(Row {
                before,
                drawn: letters(f[1]),
                expected: f[2].to_string(),
            });
        }
    }
    let tune = std::fs::read_to_string(root().join("data/eval/next-manglish.tune.tsv")).unwrap();
    for line in tune.lines().filter(|l| !l.starts_with('#')) {
        let Some((latin, malayalam)) = line.split_once('\t') else {
            continue;
        };
        let (l, m): (Vec<&str>, Vec<&str>) = (
            latin.split_whitespace().collect(),
            malayalam.split_whitespace().collect(),
        );
        if l.len() != m.len() {
            continue;
        }
        let mut before = String::new();
        for (lw, mw) in l.iter().zip(&m) {
            if letters(lw).len() >= 2 {
                rows.push(Row {
                    before: before.clone(),
                    drawn: letters(lw),
                    expected: mw.to_string(),
                });
            }
            before.push_str(mw);
            before.push(' ');
        }
    }
    rows
}

struct Score {
    rows: usize,
    top1: usize,
    top3: usize,
    times: Vec<Duration>,
}

fn run(engine: &Engine, rows: &[Row], seed: u64, same: impl Fn(&str, &str) -> bool) -> Score {
    let keys = qwerty();
    let map = KeyMap::new(&keys).unwrap();
    let mut rng = Rng(seed);
    let mut score = Score {
        rows: rows.len(),
        top1: 0,
        top3: 0,
        times: Vec::new(),
    };
    for row in rows {
        let path = noisy_path(&map, &row.drawn, &mut rng);
        let start = Instant::now();
        let found = engine.swipe(&path, &keys, &row.before, None);
        score.times.push(start.elapsed());
        if found.first().is_some_and(|c| same(&c.text, &row.expected)) {
            score.top1 += 1;
        }
        if found.iter().take(3).any(|c| same(&c.text, &row.expected)) {
            score.top3 += 1;
        } else if std::env::var("SWIPE_SHOW").is_ok() {
            let top: Vec<&str> = found.iter().take(3).map(|c| c.text.as_str()).collect();
            println!("  {} → {} got {top:?}", row.drawn, row.expected);
        }
    }
    score
}

/// Prints a set's numbers and checks them against `data/eval/baselines-swipe.json` (a full run
/// only): top-1 and top-3 may not drop more than `SLACK` below what was recorded.
fn report(name: &str, key: &str, s: &mut Score, gate: bool) {
    const SLACK: f64 = 0.005;
    s.times.sort();
    let p = |q: f64| s.times[((s.times.len() as f64 - 1.0) * q) as usize].as_secs_f64() * 1000.0;
    println!(
        "{name:28} {:5} rows  top-1 {:.4}  top-3 {:.4}  p50 {:.2} ms  p90 {:.2} ms  max {:.2} ms",
        s.rows,
        s.top1 as f64 / s.rows as f64,
        s.top3 as f64 / s.rows as f64,
        p(0.5),
        p(0.9),
        p(1.0)
    );
    if gate {
        let text = std::fs::read_to_string(root().join("data/eval/baselines-swipe.json")).unwrap();
        let base: serde_json::Value = serde_json::from_str(&text).unwrap();
        if base.get(key).is_none() {
            println!("{name:28} no baseline recorded ({key})");
            return;
        }
        for (metric, hits) in [("top1", s.top1), ("top3", s.top3)] {
            let now = hits as f64 / s.rows as f64;
            let was = base[key][metric].as_f64().unwrap();
            assert!(
                now >= was - SLACK,
                "{name} {metric} {now:.4} is below the baseline {was:.4}"
            );
        }
    }
}

#[test]
#[ignore]
fn swipe_eval() {
    let tables = mlengine_core::romanisation::Romanisation::load(&root().join("packs/ml.neopack/romanisation.json")).unwrap();
    let engine = engine();
    // SWIPE_EVERY=n keeps every nth row, for quicker sweeps.
    let every: usize = std::env::var("SWIPE_EVERY").map_or(1, |v| v.parse().unwrap());
    let thin = |rows: Vec<Row>| rows.into_iter().step_by(every).collect::<Vec<Row>>();
    let english = thin(english_rows());
    let manglish = thin(manglish_rows());
    let mg = mlengine_core::lexicon::Lexicon::open(
        &root().join("packs/ml.neopack/manglish.fst"),
    )
    .unwrap();
    let known: Vec<Row> = manglish
        .iter()
        .filter(|r| mlengine_core::manglish::key(&tables, &r.expected).is_some_and(|k| mg.get(&k).is_some()))
        .map(|r| Row {
            before: r.before.clone(),
            drawn: r.drawn.clone(),
            expected: r.expected.clone(),
        })
        .collect();
    let en_same = |a: &str, b: &str| {
        a.to_lowercase().replace('\u{2019}', "'") == b.to_lowercase().replace('\u{2019}', "'")
    };
    let gate = every == 1
        && std::env::var("SWIPE_WEIGHTS").is_err()
        && std::env::var("SWIPE_NOISE").is_err();
    report(
        "english",
        "english",
        &mut run(&engine, &english, 1, en_same),
        gate,
    );
    report(
        "manglish (all)",
        "manglish",
        &mut run(&engine, &manglish, 2, |a, b| a == b),
        gate,
    );
    report(
        "manglish (in lexicon)",
        "manglish_in_lexicon",
        &mut run(&engine, &known, 2, |a, b| a == b),
        gate,
    );
}

/// English and `pack` as shipped, without its transliterator (a swipe never asks it).
fn pack_engine(pack: &mlengine_core::pack::Pack) -> Engine {
    let en = root().join("apps/ios/KeyboardExtension/Lexicon/en.fst");
    let mut paths = Paths::with_packs(Some(en), std::slice::from_ref(pack), None);
    paths.packs[0].translit = None;
    let mut engine = Engine::open(&paths);
    if let Ok(w) = std::env::var("SWIPE_WEIGHTS") {
        let v: Vec<f32> = w.split(',').map(|x| x.parse().unwrap()).collect();
        engine.set_swipe_weights(Weights {
            align: v[0],
            shape: v[1],
            length: v[2],
            ..Weights::default()
        });
    }
    engine
}

/// The pack's held-out pairs as rows: the Latin spelling drawn, the native word expected.
fn pack_rows(id: &str) -> Vec<Row> {
    let path = root().join("data/eval").join(id).join("translit-human.tsv");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.len() >= 3 && letters(f[1]).len() >= 2).then(|| Row {
                before: String::new(),
                drawn: letters(f[1]),
                expected: f[2].to_string(),
            })
        })
        .collect()
}

/// A seed for the pack's paths from its id (FNV-1a): the same on every run, never zero.
fn pack_seed(id: &str) -> u64 {
    id.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    }) | 1
}

/// Swipe accuracy for the packs in SWIPE_PACKS (default `hi as ta`), each on its own engine.
#[test]
#[ignore]
fn swipe_eval_packs() {
    let ids = std::env::var("SWIPE_PACKS").unwrap_or_else(|_| "hi as ta".into());
    let every: usize = std::env::var("SWIPE_EVERY").map_or(1, |v| v.parse().unwrap());
    let gate = every == 1
        && std::env::var("SWIPE_WEIGHTS").is_err()
        && std::env::var("SWIPE_NOISE").is_err();
    for id in ids.split_whitespace() {
        let pack = mlengine_core::pack::Pack::open(&root().join(format!("packs/{id}.neopack")))
            .unwrap_or_else(|e| panic!("packs/{id}.neopack: {e}"));
        let tables = mlengine_core::romanisation::Romanisation::load(
            &pack.file("romanisation").expect("the pack's romanisation tables"),
        )
        .unwrap();
        let romanised = mlengine_core::lexicon::Lexicon::open(
            &pack.file("romanisedLexicon").expect("the pack's romanised lexicon"),
        )
        .unwrap();
        let engine = pack_engine(&pack);
        let rows: Vec<Row> = pack_rows(id).into_iter().step_by(every).collect();
        let known: Vec<Row> = rows
            .iter()
            .filter(|r| {
                mlengine_core::manglish::key(&tables, &r.expected)
                    .is_some_and(|k| romanised.get(&k).is_some())
            })
            .map(|r| Row {
                before: r.before.clone(),
                drawn: r.drawn.clone(),
                expected: r.expected.clone(),
            })
            .collect();
        let seed = pack_seed(id);
        report(
            &format!("{id} (all)"),
            id,
            &mut run(&engine, &rows, seed, |a, b| a == b),
            gate,
        );
        report(
            &format!("{id} (in lexicon)"),
            &format!("{id}_in_lexicon"),
            &mut run(&engine, &known, seed, |a, b| a == b),
            gate,
        );
    }
}

/// How often the expected Manglish word's skeleton is among the beam's finished words, and at
/// which rank: separates the beam's misses from the ranking's.
#[test]
#[ignore]
fn swipe_recall() {
    let tables = mlengine_core::romanisation::Romanisation::load(&root().join("packs/ml.neopack/romanisation.json")).unwrap();
    use mlengine_core::swipe::{MAX_POINTS, STEP, decode, resample};
    let rules = tables.swipe_rules();
    let every: usize = std::env::var("SWIPE_EVERY").map_or(1, |v| v.parse().unwrap());
    let mg = mlengine_core::lexicon::Lexicon::open(
        &root().join("packs/ml.neopack/manglish.fst"),
    )
    .unwrap();
    let keys = qwerty();
    let map = KeyMap::new(&keys).unwrap();
    let mut rng = Rng(2);
    let (mut rows, mut found, mut first, mut top16) = (0, 0, 0, 0);
    for row in manglish_rows().into_iter().step_by(every) {
        let Some(key) = mlengine_core::manglish::key(&tables, &row.expected) else {
            continue;
        };
        if mg.get(&key).is_none() {
            continue;
        }
        let skeleton = &key[..key.iter().position(|&b| b == 0).unwrap()];
        let path = resample(
            &map.scale(&noisy_path(&map, &row.drawn, &mut rng)),
            STEP,
            MAX_POINTS,
        );
        let finished = decode(&mg, &map, &path, Some(rules));
        rows += 1;
        if let Some(rank) = finished.iter().position(|f| f.key == skeleton) {
            found += 1;
            first += usize::from(rank == 0);
            top16 += usize::from(rank < 16);
        } else if std::env::var("SWIPE_SHOW").is_ok() {
            println!(
                "  {} ({}) missed; best {:?}",
                row.drawn,
                String::from_utf8_lossy(skeleton),
                finished
                    .iter()
                    .take(3)
                    .map(|f| String::from_utf8_lossy(&f.key).to_string())
                    .collect::<Vec<_>>()
            );
        }
    }
    println!(
        "{rows} rows: skeleton found {:.4}, first {:.4}, in top 16 {:.4}",
        found as f64 / rows as f64,
        first as f64 / rows as f64,
        top16 as f64 / rows as f64
    );
}

/// The same for the packs in SWIPE_PACKS (default `hi as ta`), over the held-out words the
/// pack's romanised lexicon has, split by whether the spelling drawn has the word's skeleton
/// (a swipe finds a word by its skeleton, so a spelling with another one can't give it).
#[test]
#[ignore]
fn swipe_recall_packs() {
    use mlengine_core::swipe::{MAX_POINTS, STEP, decode, resample};
    let ids = std::env::var("SWIPE_PACKS").unwrap_or_else(|_| "hi as ta".into());
    let every: usize = std::env::var("SWIPE_EVERY").map_or(1, |v| v.parse().unwrap());
    let keys = qwerty();
    let map = KeyMap::new(&keys).unwrap();
    for id in ids.split_whitespace() {
        let pack = mlengine_core::pack::Pack::open(&root().join(format!("packs/{id}.neopack")))
            .unwrap_or_else(|e| panic!("packs/{id}.neopack: {e}"));
        let tables = mlengine_core::romanisation::Romanisation::load(
            &pack.file("romanisation").expect("the pack's romanisation tables"),
        )
        .unwrap();
        let romanised = mlengine_core::lexicon::Lexicon::open(
            &pack.file("romanisedLexicon").expect("the pack's romanised lexicon"),
        )
        .unwrap();
        let rules = tables.swipe_rules();
        // Opening the pack makes its script's rules known, which a word's key is packed by.
        let _engine = pack_engine(&pack);
        let mut rng = Rng(pack_seed(id));
        // Rows, skeleton found, first, in the top 16: [another skeleton, the word's skeleton].
        let mut counts = [[0usize; 4]; 2];
        for row in pack_rows(id).into_iter().step_by(every) {
            let path = noisy_path(&map, &row.drawn, &mut rng);
            let Some(key) = mlengine_core::manglish::key(&tables, &row.expected) else {
                continue;
            };
            if romanised.get(&key).is_none() {
                continue;
            }
            let skeleton = &key[..key.iter().position(|&b| b == 0).unwrap()];
            let spelled = tables.skeleton(&row.drawn).as_bytes() == skeleton;
            let path = resample(&map.scale(&path), STEP, MAX_POINTS);
            let finished = decode(&romanised, &map, &path, Some(rules));
            let c = &mut counts[usize::from(spelled)];
            c[0] += 1;
            if let Some(rank) = finished.iter().position(|f| f.key == skeleton) {
                c[1] += 1;
                c[2] += usize::from(rank == 0);
                c[3] += usize::from(rank < 16);
            } else if spelled && std::env::var("SWIPE_SHOW").is_ok() {
                println!(
                    "  {} ({}) missed; best {:?}",
                    row.drawn,
                    String::from_utf8_lossy(skeleton),
                    finished
                        .iter()
                        .take(3)
                        .map(|f| String::from_utf8_lossy(&f.key).to_string())
                        .collect::<Vec<_>>()
                );
            }
        }
        for (name, c) in [("another skeleton", counts[0]), ("the word's skeleton", counts[1])] {
            let of = |x: usize| x as f64 / c[0].max(1) as f64;
            println!(
                "{id} drawn with {name:20} {:5} rows: skeleton found {:.4}, first {:.4}, in top 16 {:.4}",
                c[0],
                of(c[1]),
                of(c[2]),
                of(c[3])
            );
        }
    }
}

/// One word over a clean path (seed 0) and a few noisy ones: SWIPE_WORD=randu
/// SWIPE_SKELETON=rant, on Malayalam's pack or SWIPE_PACK's (SWIPE_PACK=hi SWIPE_WORD=karne
/// SWIPE_SKELETON=krne).
#[test]
#[ignore]
fn swipe_word() {
    use mlengine_core::swipe::{MAX_POINTS, STEP, decode, fit, ideal_path, resample};
    let word = std::env::var("SWIPE_WORD").unwrap();
    let skeleton = std::env::var("SWIPE_SKELETON").unwrap();
    let id = std::env::var("SWIPE_PACK").unwrap_or_else(|_| "ml".into());
    let pack = mlengine_core::pack::Pack::open(&root().join(format!("packs/{id}.neopack")))
        .unwrap_or_else(|e| panic!("packs/{id}.neopack: {e}"));
    let mg = mlengine_core::lexicon::Lexicon::open(
        &pack.file("romanisedLexicon").expect("the pack's romanised lexicon"),
    )
    .unwrap();
    let map = KeyMap::new(&qwerty()).unwrap();
    let tables = mlengine_core::romanisation::Romanisation::load(
        &pack.file("romanisation").expect("the pack's romanisation tables"),
    )
    .unwrap();
    let rules = tables.swipe_rules();
    // SWIPE_ONLY="krne kkrne": these skeletons alone, so the beam drops none of them.
    let only = std::env::var("SWIPE_ONLY").ok().map(|list| {
        let mut keys: Vec<Vec<u8>> = list
            .split_whitespace()
            .map(|k| [k.as_bytes(), b"\0x"].concat())
            .collect();
        keys.sort();
        let built = mlengine_core::lexicon::build(keys.into_iter().map(|k| (k, 1))).unwrap();
        mlengine_core::lexicon::Lexicon::from_bytes(built).unwrap()
    });
    for seed in 0..6 {
        let raw = if seed == 0 {
            ideal_path(&map, &word, 4.0)
        } else {
            noisy_path(&map, &word, &mut Rng(seed))
        };
        let path = resample(&raw, STEP, MAX_POINTS);
        let finished = decode(&mg, &map, &path, Some(rules));
        let best: Vec<String> = finished
            .iter()
            .take(3)
            .map(|f| format!("{} {:.2}", String::from_utf8_lossy(&f.key), f.cost))
            .collect();
        let rank = finished.iter().position(|f| f.key == skeleton.as_bytes());
        println!(
            "seed {seed}: start {:?} end {:?}; {skeleton} fit {:.2} rank {rank:?} cost {:?}; best {best:?}",
            raw[0],
            raw[raw.len() - 1],
            fit(&map, &path, &skeleton),
            rank.map(|r| finished[r].cost)
        );
        if let Some(only) = &only {
            let alone: Vec<String> = decode(only, &map, &path, Some(rules))
                .iter()
                .map(|f| format!("{} {:.2}", String::from_utf8_lossy(&f.key), f.cost))
                .collect();
            println!("  alone: {alone:?}");
        }
    }
}
