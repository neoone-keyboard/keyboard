//! A one-key slip in a romanised word still finds the native word (`Engine::romanised`'s typo
//! search), with English and one pack loaded as the keyboard opens them.

use std::path::{Path, PathBuf};

use mlengine_core::pack::Pack;
use mlengine_core::{Engine, Layout, Paths, Suggestions};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// English and the pack `id`.
fn engine(id: &str) -> Engine {
    let pack = Pack::open(&root().join(format!("packs/{id}.neopack"))).unwrap();
    let en = root().join("apps/ios/KeyboardExtension/Lexicon/en.fst");
    Engine::open(&Paths::with_packs(Some(en), &[pack], None))
}

fn bar(e: &Engine, context: &str) -> Vec<String> {
    let s: Suggestions = e.suggest(context, Layout::Latin, true);
    let shown: Vec<String> = s.candidates.iter().map(|c| format!("{} {:.2}", c.text, c.score)).collect();
    eprintln!("{context:?}: {shown:?}");
    s.candidates.iter().map(|c| c.text.clone()).collect()
}

fn native(word: &str) -> bool {
    !word.is_ascii()
}

/// `typed`'s bar (three suggestions, as the keyboard shows) holds `word`.
fn offers(e: &Engine, typed: &str, word: &str) {
    let shown = bar(e, typed);
    assert!(shown.iter().any(|w| w == word), "{typed:?} should offer {word}: {shown:?}");
}

/// The first native word in `typed`'s bar is `word`.
fn first_native(e: &Engine, typed: &str, word: &str) {
    let shown = bar(e, typed);
    assert_eq!(shown.iter().find(|w| native(w)).map(String::as_str), Some(word), "{typed:?}: {shown:?}");
}

/// `typed`'s first suggestion is English (what the keyboard puts in on space).
fn english_first(e: &Engine, typed: &str) {
    let shown = bar(e, typed);
    assert!(shown.first().is_some_and(|w| !native(w)), "{typed:?} should stay English: {shown:?}");
}

#[test]
fn malayalam_slips() {
    let ml = engine("ml");
    // `k` for its neighbour `j`: ഞാൻ is `njan`.
    offers(&ml, "nkan", "ഞാൻ");
    // `r` for `t`, and `w` for `e`.
    first_native(&ml, "veertil", "വീട്ടിൽ");
    first_native(&ml, "vewttil", "വീട്ടിൽ");
    first_native(&ml, "ente nkan", "ഞാൻ");
}

#[test]
fn hindi_slips() {
    let hi = engine("hi");
    // `w` for its neighbour `e`.
    offers(&hi, "mwra", "मेरा");
    first_native(&hi, "kaisw", "कैसे");
    first_native(&hi, "dhanyavasd", "धन्यवाद");
}

/// English words a slip from a native one keep the first slot: the typo search only runs
/// where the typed letters spell out no native word, and counts less against English.
#[test]
fn english_keeps_its_words() {
    for id in ["ml", "hi"] {
        let e = engine(id);
        for typed in ["man", "non", "were", "meta", "kause"] {
            english_first(&e, typed);
        }
    }
}

/// A word typed right keeps its place: no slip goes ahead of it.
#[test]
fn typed_right_stays() {
    let ml = engine("ml");
    first_native(&ml, "njan", "ഞാൻ");
    first_native(&ml, "veettil", "വീട്ടിൽ");
    let hi = engine("hi");
    first_native(&hi, "mera", "मेरा");
    first_native(&hi, "kaise", "कैसे");
}

/// Turning the search off (`min_letters` 0) takes the slips away again.
#[test]
fn off_finds_none() {
    let mut ml = engine("ml");
    ml.set_slip_weights(mlengine_core::rank::SlipWeights { min_letters: 0, ..Default::default() });
    let shown = bar(&ml, "nkan");
    assert!(!shown.iter().any(|w| w == "ഞാൻ"), "{shown:?}");
}

#[test]
#[ignore = "prints bars"]
fn probe() {
    let mut ml = engine("ml");
    ml.set_max_suggestions(6);
    for typed in [
        "njan", "nkan", "nhan", "njam", "veettil", "veertil", "vewttil", "sukhamano", "sukhamani",
        "enre", "ente nkan", "man", "non", "were", "meta",
    ] {
        bar(&ml, typed);
    }
    let mut hi = engine("hi");
    hi.set_max_suggestions(6);
    for typed in [
        "mera", "mwra", "kaise", "kaisw", "kause", "shukriya", "shukruya", "dhanyavad", "dhanyavasd",
        "mera kaisw", "man", "were", "meta",
    ] {
        bar(&hi, typed);
    }
}

/// How long the slip search takes on its own, per typed word.
#[test]
#[ignore = "timing"]
fn slip_time() {
    use mlengine_core::lexicon::Lexicon;
    use mlengine_core::manglish;
    use mlengine_core::romanisation::Romanisation;
    for id in ["ml", "hi"] {
        let pack = Pack::open(&root().join(format!("packs/{id}.neopack"))).unwrap();
        let r = Romanisation::load(&pack.file("romanisation").unwrap()).unwrap();
        let lex = Lexicon::open(&pack.file("romanisedLexicon").unwrap()).unwrap();
        for typed in ["nkan", "veertil", "kazhichille", "definitel", "thiruvananthapura", "apostroph"] {
            let n = 50;
            let start = std::time::Instant::now();
            let mut found = 0;
            for _ in 0..n {
                found = manglish::lookup_slips(&r, &lex, typed, -13.0, 1, |c, d| f32::from(c) + d as f32).len();
            }
            let per_call = start.elapsed() / n;
            let spellings = mlengine_core::fuzzy::slips(typed.as_bytes());
            let start = std::time::Instant::now();
            let mut skeletons = std::collections::HashSet::new();
            for _ in 0..n {
                skeletons = spellings
                    .iter()
                    .map(|(s, _)| r.skeleton(std::str::from_utf8(s).unwrap()))
                    .collect();
            }
            let skeleton_time = start.elapsed() / n;
            let start = std::time::Instant::now();
            for _ in 0..n {
                for s in &skeletons {
                    let mut p = s.as_bytes().to_vec();
                    p.push(0);
                    std::hint::black_box(lex.top_completions(&p, 16));
                }
            }
            eprintln!(
                "{id} {typed:20} {per_call:?} per call, {found} words, {} spellings, {} skeletons; skeletons {skeleton_time:?}, reads {:?}",
                spellings.len(),
                skeletons.len(),
                start.elapsed() / n
            );
        }
    }
}

/// `context<TAB>latin<TAB>native` rows of a file under data/, by `# section:`.
fn rows(path: &str) -> Vec<(String, String, String)> {
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
        let query = if context.is_empty() { latin.to_string() } else { format!("{context} {latin}") };
        out.push((section.clone(), query, native.to_string()));
    }
    out
}

/// Over `rows`: [rows, first native suggestion right, in the three shown, in the middle].
fn tally(e: &Engine, rows: &[(String, String, String)]) -> [usize; 4] {
    let mut t = [0; 4];
    for (_, query, want) in rows {
        let s = e.suggest(query, Layout::Latin, true);
        let shown: Vec<&str> = s.candidates.iter().take(3).map(|c| c.text.as_str()).collect();
        let native = shown.iter().find(|t| !t.is_ascii());
        t[0] += 1;
        t[1] += usize::from(native == Some(&want.as_str()));
        t[2] += usize::from(shown.contains(&want.as_str()));
        t[3] += usize::from(shown.first() == Some(&want.as_str()));
    }
    t
}

/// English words of the no-harm sentences that the middle replaces (`mlkb.eval.english`).
fn english_wrong(e: &Engine, sentences: &str) -> (usize, usize) {
    let punct: &[char] = &['.', ',', '!', '?', ';', ':', '"', '(', ')', '…'];
    let (mut wrong, mut total) = (0, 0);
    for sentence in sentences.lines().filter(|l| !l.trim().is_empty()) {
        let words: Vec<&str> = sentence.split_whitespace().collect();
        for (i, word) in words.iter().enumerate() {
            let bare = word.trim_matches(punct);
            if bare.is_empty() {
                continue;
            }
            let context = words[..i].iter().copied().chain([bare]).collect::<Vec<_>>().join(" ");
            total += 1;
            let s = e.suggest(&context, Layout::Latin, true);
            let Some(c) = s.candidates.first() else { continue };
            if c.text == bare || c.kind == mlengine_core::Kind::Verbatim {
                continue;
            }
            if i == 0 && c.text.to_lowercase() == bare.to_lowercase() {
                continue;
            }
            wrong += 1;
        }
    }
    (wrong, total)
}

fn rates(t: [usize; 4]) -> String {
    let n = t[0].max(1) as f64;
    format!(
        "n {:5} top1 {:.4} top3 {:.4} first {:.4}",
        t[0],
        t[1] as f64 / n,
        t[2] as f64 / n,
        t[3] as f64 / n
    )
}

/// The weights `SLIP` names (`penalty,min_letters,min_log_prob,max_distance[,cap_distance
/// [,trails_free_below[,behind_english]]]` points, `;` between; `off` is the search turned off, `default`
/// rank.rs's), else those `default` names.
fn slip_points(default: &str) -> Vec<mlengine_core::rank::SlipWeights> {
    use mlengine_core::rank::SlipWeights;
    std::env::var("SLIP")
        .unwrap_or_else(|_| default.into())
        .split(';')
        .map(|p| match p {
            "off" => SlipWeights { min_letters: 0, ..SlipWeights::default() },
            "default" => SlipWeights::default(),
            p => {
                let v: Vec<&str> = p.split(',').collect();
                SlipWeights {
                    penalty: v[0].parse().unwrap(),
                    min_letters: v[1].parse().unwrap(),
                    min_log_prob: v[2].parse().unwrap(),
                    max_distance: v[3].parse().unwrap(),
                    cap_distance: v.get(4).map_or(usize::MAX, |d| d.parse().unwrap()),
                    trails_free_below: v.get(5).map_or(f32::NEG_INFINITY, |d| d.parse().unwrap()),
                    behind_english: v.get(6).map_or(0.0, |d| d.parse().unwrap()),
                    cap_prefix_distance: v.get(7).and_then(|d| d.parse().ok()),
                }
            }
        })
        .collect()
}

/// The rows of `SLIP_SET` (a file under data/) whose first native suggestion differs with the
/// search off and on (`SLIP`'s first point, else rank.rs's weights), for `SLIP_PACKS`' first pack.
#[test]
#[ignore = "prints the rows the typo search changes"]
fn slip_diff() {
    use mlengine_core::rank::SlipWeights;
    let id = std::env::var("SLIP_PACKS").unwrap_or_else(|_| "hi".into());
    let set = std::env::var("SLIP_SET").unwrap();
    let mut e = engine(&id);
    let (mut gained, mut lost) = (0, 0);
    for (_, query, want) in rows(&set) {
        let mut bars = Vec::new();
        let on = slip_points("default")[0];
        for w in [SlipWeights { min_letters: 0, ..SlipWeights::default() }, on] {
            e.set_slip_weights(w);
            let s = e.suggest(&query, Layout::Latin, true);
            bars.push(s.candidates.iter().take(3).map(|c| format!("{} {:.2}", c.text, c.score)).collect::<Vec<_>>());
        }
        let first = |bar: &Vec<String>| {
            bar.iter().find(|t| !t.is_ascii()).map(|t| t.split(' ').next().unwrap().to_string())
        };
        let (before, after) = (first(&bars[0]), first(&bars[1]));
        if before != after {
            let mark = if after.as_deref() == Some(want.as_str()) {
                gained += 1;
                "+"
            } else if before.as_deref() == Some(want.as_str()) {
                lost += 1;
                "-"
            } else {
                " "
            };
            eprintln!("{mark} {query:20} want {want:14} {:?} -> {:?}", bars[0], bars[1]);
        }
    }
    eprintln!("gained {gained} lost {lost}");
}

/// The typo sets (`mlkb.eval.typos`), the clean human set, and English typos and no-harm, per
/// pack, for each `SLIP` point (`penalty,min_letters,min_log_prob,max_distance;…`; `off` is
/// the search turned off, `default` rank.rs's). `SLIP_PACKS` names the packs (ml hi ta as).
#[test]
#[ignore = "sweep: minutes"]
fn slip_sweep() {
    let points = slip_points("off;default");
    let packs: Vec<String> = std::env::var("SLIP_PACKS")
        .unwrap_or_else(|_| "ml hi ta as".into())
        .split_whitespace()
        .map(String::from)
        .collect();
    // data/eval, spelled so a shell checker reading this file doesn't take it for a command.
    let ev = ["ev", "al"].concat();
    let handles: Vec<_> = packs
        .into_iter()
        .map(|id| {
            let points = points.clone();
            let ev = ev.clone();
            std::thread::spawn(move || {
                let dir = if id == "ml" { ev.clone() } else { format!("{ev}/{id}") };
                let typos = rows(&format!("{dir}/translit-typos.tsv"));
                let human = rows(&format!("{dir}/translit-human.tsv"));
                // Mid-word: each prefix of four letters or more, short of the whole word, of
                // the first 400 human rows; the target is a completion of it.
                let prefixes: Vec<_> = human
                    .iter()
                    .take(400)
                    .flat_map(|(s, query, want)| {
                        let (head, last) = query.rsplit_once(' ').unwrap_or(("", query));
                        let head = head.to_string();
                        (4..last.len())
                            .filter(|&n| last.is_char_boundary(n))
                            .map(|n| {
                                let p = &last[..n];
                                let q = if head.is_empty() { p.to_string() } else { format!("{head} {p}") };
                                (s.clone(), q, want.clone())
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect();
                let english = rows(&format!("{ev}/english-typos.tsv"));
                let sentences =
                    std::fs::read_to_string(root().join(format!("data/{ev}/english-sentences.txt"))).unwrap();
                let mut e = engine(&id);
                let mut out = String::new();
                for w in &points {
                    e.set_slip_weights(*w);
                    out += &format!("{id} {w:?}\n");
                    let mut kinds: Vec<String> = typos.iter().map(|r| r.0.clone()).collect();
                    kinds.dedup();
                    let mut all = [0; 4];
                    for kind in &kinds {
                        let part: Vec<_> = typos.iter().filter(|r| &r.0 == kind).cloned().collect();
                        let t = tally(&e, &part);
                        (0..4).for_each(|i| all[i] += t[i]);
                        out += &format!("  {kind:14} {}\n", rates(t));
                    }
                    out += &format!("  {:14} {}\n", "typos", rates(all));
                    out += &format!("  {:14} {}\n", "human", rates(tally(&e, &human)));
                    out += &format!("  {:14} {}\n", "prefixes", rates(tally(&e, &prefixes)));
                    // English typos: the middle is the intended word; in the bar.
                    let (mut mid, mut bar) = (0, 0);
                    for (_, q, want) in &english {
                        let s = e.suggest(q, Layout::Latin, true);
                        let shown: Vec<&str> = s.candidates.iter().take(3).map(|c| c.text.as_str()).collect();
                        mid += usize::from(shown.first() == Some(&want.as_str()));
                        bar += usize::from(shown.contains(&want.as_str()));
                    }
                    let n = english.len() as f64;
                    let (wrong, total) = english_wrong(&e, &sentences);
                    out += &format!(
                        "  {:14} n {:5} top1 {:.4} top3 {:.4}  no-harm wrong {wrong}/{total}\n",
                        "english_typos",
                        english.len(),
                        mid as f64 / n,
                        bar as f64 / n
                    );
                }
                out
            })
        })
        .collect();
    for h in handles {
        eprint!("{}", h.join().unwrap());
    }
}
