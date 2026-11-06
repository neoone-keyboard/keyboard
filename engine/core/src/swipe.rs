//! Swipe typing (design `2026-11-06-m4-swipe-design.md`): the finger's path across the QWERTY
//! keys becomes the words it could have drawn.
//!
//! The path is resampled to evenly spaced points, then a beam walks a lexicon's FST one letter
//! at a time. Each partial word keeps, for every path point, the cheapest way to have drawn its
//! letters with the last one at that point: adding a letter takes the running minimum up to
//! the point and adds how far the point is from the letter's key. Keys the path only passes
//! over cost nothing, and a doubled letter costs nothing extra, since both copies sit on the
//! same point. A word is finished when the FST ends it (English) or when its skeleton is
//! followed by the separator (`manglish.fst`'s `skeleton\0word`). `shape_cost` then checks the
//! finished words the other way round: how far the path strays from the line through their keys.
//!
//! Distances are in key widths. Everything is bounded: points, beam width, word length and
//! the number of finished words, so time grows with the path only.

use fst::raw::{Fst, Output};

use crate::lexicon::{FIRST_WORD_BYTE, Lexicon, SEPARATOR};

/// Most points a path is resampled to; a longer path gets a wider spacing.
pub const MAX_POINTS: usize = 64;
/// Spacing of resampled points, in key widths.
pub const STEP: f32 = 0.4;
/// Partial words kept at each length.
pub const BEAM: usize = 256;
/// Longest word looked for.
pub const MAX_LETTERS: usize = 24;
/// Finished words kept for rescoring.
pub const FINISHED: usize = 64;

/// Skipping past a bend in the path costs this per radian (per-point bends are stored
/// already weighted): the path bends at its letters, so
/// a word that leaves a bend unmatched (`so` for a path drawn through s-u-k-h-a-m-a-n-o) is
/// not the word drawn. Turns below `BEND_NOISE` at a point are a wobbly finger, not a bend.
const BEND: f32 = 3.0;
const BEND_NOISE: f32 = 0.25;

/// A letter's cost is its squared distance from the path point, over this: a point half a key
/// from the letter's centre costs 1.
const SPREAD: f32 = 0.25;
/// A letter farther than this (squared, in key widths) from every point can't be in the word.
const FAR: f32 = 2.0 * 2.0;
/// The first letter may sit a few points into the path, and the last a few before its end
/// (a finger lands and lifts a little off the key), at this cost per point.
const END_SLACK: usize = 3;
const END_SKIP: f32 = 1.0;

/// A letter key as the keyboard draws it, in any unit (the app sends points).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Key {
    pub letter: u8,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// The letter keys' centres in key widths (the median letter key width is 1).
#[derive(Debug, Clone)]
pub struct KeyMap {
    centres: [Option<(f32, f32)>; 26],
    unit: f32,
}

impl KeyMap {
    /// None without any a–z key or with keys of no width.
    pub fn new(keys: &[Key]) -> Option<Self> {
        let mut widths: Vec<f32> = keys
            .iter()
            .filter(|k| k.letter.is_ascii_lowercase() && k.width > 0.0)
            .map(|k| k.width)
            .collect();
        if widths.is_empty() {
            return None;
        }
        widths.sort_by(f32::total_cmp);
        let unit = widths[widths.len() / 2];
        let mut centres = [None; 26];
        for k in keys.iter().filter(|k| k.letter.is_ascii_lowercase()) {
            centres[usize::from(k.letter - b'a')] =
                Some(((k.x + k.width / 2.0) / unit, (k.y + k.height / 2.0) / unit));
        }
        Some(Self { centres, unit })
    }

    /// The centre of a letter's key (either case); None for anything else.
    pub fn centre(&self, letter: u8) -> Option<(f32, f32)> {
        let l = letter.to_ascii_lowercase();
        l.is_ascii_lowercase()
            .then(|| self.centres[usize::from(l - b'a')])
            .flatten()
    }

    /// A path in the keys' unit, in key widths.
    pub fn scale(&self, path: &[(f32, f32)]) -> Vec<(f32, f32)> {
        path.iter()
            .map(|&(x, y)| (x / self.unit, y / self.unit))
            .collect()
    }
}

fn dist2(a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (a.0 - b.0, a.1 - b.1);
    dx * dx + dy * dy
}

/// Points `step` apart along `path` (wider when that would make more than `max`), keeping
/// its first and last point.
pub fn resample(path: &[(f32, f32)], step: f32, max: usize) -> Vec<(f32, f32)> {
    let Some(&first) = path.first() else {
        return Vec::new();
    };
    let length: f32 = path.windows(2).map(|w| dist2(w[0], w[1]).sqrt()).sum();
    if length <= f32::EPSILON || max < 2 {
        return vec![first];
    }
    let step = step.max(length / (max - 1) as f32);
    let mut out = vec![first];
    // Distance walked since the last point placed.
    let mut carried = 0.0;
    for w in path.windows(2) {
        let (a, b) = (w[0], w[1]);
        let segment = dist2(a, b).sqrt();
        let mut along = step - carried;
        while along <= segment && out.len() < max - 1 {
            let t = along / segment;
            out.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
            along += step;
        }
        carried = segment - (along - step);
    }
    let last = *path.last().unwrap();
    if dist2(*out.last().unwrap(), last) > (step / 4.0).powi(2) || out.len() == 1 {
        out.push(last);
    } else {
        *out.last_mut().unwrap() = last;
    }
    out
}

/// A word the beam finished: its key bytes as stored, its count (0 for a Manglish skeleton,
/// whose words are looked up after it) and how well it fits the path (lower is better).
#[derive(Debug, Clone, PartialEq)]
pub struct Finished {
    pub key: Vec<u8>,
    pub count: u64,
    pub cost: f32,
}

struct Hyp {
    addr: usize,
    out: Output,
    key: Vec<u8>,
    /// Per path point: the cheapest fit with the last letter there.
    cost: Vec<f32>,
    /// Length of the line through the word's keys so far, and its last key's centre.
    line: f32,
    last: Option<(f32, f32)>,
    /// The beam's ranking (`rank`).
    best: f32,
}

/// Per key width between the line through a partial word's keys and the path up to its
/// best point (`rank`).
const PROGRESS: f32 = 1.0;

/// Ranks a partial word in the beam: its fit at the point where it fits best, plus how far the
/// line through its keys is from the path's length up to that point. Without the second term,
/// prefixes that jump ahead along the path look as good as ones that follow it.
fn rank(cost: &[f32], arc: &[f32], line: f32) -> f32 {
    cost.iter()
        .zip(arc)
        .map(|(&c, &a)| c + PROGRESS * (a - line).abs())
        .fold(f32::INFINITY, f32::min)
}

/// What a byte on an FST transition means for the path.
enum Step {
    Letter(usize),
    /// An apostrophe or hyphen: in the word, but not drawn.
    Silent,
    Skip,
}

fn step_for(b: u8) -> Step {
    match b {
        b'a'..=b'z' => Step::Letter(usize::from(b - b'a')),
        b'A'..=b'Z' => Step::Letter(usize::from(b - b'A')),
        b'\'' | b'-' => Step::Silent,
        _ => Step::Skip,
    }
}

/// The keys a Manglish skeleton letter may be drawn through (the pack's skeleton folds them):
/// `k` is also `c`, `q` and the `g` of `gh`; `t` is `d`; `p` is `f`; `v` is `w`; `i` is the
/// `e` of `ee`; `u` is the `o` of `oo`.
fn skeleton_keys(letter: u8) -> &'static [u8] {
    match letter {
        b'k' => b"kcqg",
        b't' => b"td",
        b'p' => b"pf",
        b'v' => b"vw",
        b'i' => b"ie",
        b'u' => b"uo",
        _ => std::slice::from_ref(&LETTERS[usize::from(letter - b'a')]),
    }
}

const LETTERS: [u8; 26] = *b"abcdefghijklmnopqrstuvwxyz";

/// Skeleton letters people write with an `h` the skeleton drops (`th`, `kh`, `sh` …): the
/// path may pass through `h` after them, bending there, at no cost.
const ASPIRATED: &[u8] = b"ktpbcjsz";

/// The keys a skeleton was most likely drawn through: each letter's key (`skeleton_keys`)
/// nearest the path. For `shape_cost` and `length_cost`, which compare a drawing with the path.
pub fn drawn(keys: &KeyMap, path: &[(f32, f32)], skeleton: &[u8]) -> Vec<u8> {
    let near = |k: u8| {
        keys.centre(k).map_or(f32::INFINITY, |c| {
            path.iter()
                .map(|&p| dist2(p, c))
                .fold(f32::INFINITY, f32::min)
        })
    };
    skeleton
        .iter()
        .map(|&l| {
            if !l.is_ascii_lowercase() {
                return l;
            }
            skeleton_keys(l)
                .iter()
                .copied()
                .min_by(|&a, &b| near(a).total_cmp(&near(b)))
                .unwrap_or(l)
        })
        .collect()
}

/// Letters a skeleton drops from the end of a word (`veedu` → `veed`, `entey` → `ent`), at most
/// `TRAILING` of them, which the path may still draw after the skeleton's last letter.
const TRAIL: &[u8] = b"euyh";
const TRAILING: usize = 2;

/// Per letter: its cost at each path point, or None for a letter far from the whole path.
fn letter_costs(keys: &KeyMap, path: &[(f32, f32)]) -> Vec<Option<Vec<f32>>> {
    (0..26u8)
        .map(|l| {
            let centre = keys.centre(b'a' + l)?;
            let row: Vec<f32> = path.iter().map(|&p| dist2(p, centre)).collect();
            (row.iter().copied().fold(f32::INFINITY, f32::min) <= FAR)
                .then(|| row.into_iter().map(|d| d / SPREAD).collect())
        })
        .collect()
}

/// The path's measures the walk uses.
struct Track<'a> {
    bends: Vec<f32>,
    arc: Vec<f32>,
    keys: &'a KeyMap,
}

/// Per point: the path's length up to it.
fn arc(path: &[(f32, f32)]) -> Vec<f32> {
    let mut sum = 0.0;
    let mut out = Vec::with_capacity(path.len());
    for (i, &p) in path.iter().enumerate() {
        if i > 0 {
            sum += dist2(path[i - 1], p).sqrt();
        }
        out.push(sum);
    }
    out
}

/// Per point: the bends up to and including it (radians past `BEND_NOISE`, summed).
fn bends(path: &[(f32, f32)]) -> Vec<f32> {
    let mut out = vec![0.0; path.len()];
    let mut sum = 0.0;
    for i in 1..path.len().saturating_sub(1) {
        let (a, b, c) = (path[i - 1], path[i], path[i + 1]);
        let (u, v) = ((b.0 - a.0, b.1 - a.1), (c.0 - b.0, c.1 - b.1));
        let turn = (u.0 * v.1 - u.1 * v.0).atan2(u.0 * v.0 + u.1 * v.1).abs();
        sum += BEND * (turn - BEND_NOISE).max(0.0);
        out[i] = sum;
    }
    if let Some(last) = out.last_mut() {
        *last = sum;
    }
    out
}

/// The finished words of `lexicon` for a resampled `path`, best first. With `skeletons`, keys
/// are `skeleton\0…` and a word ends where the separator follows; otherwise where a key ends.
pub fn decode(
    lexicon: &Lexicon,
    keys: &KeyMap,
    path: &[(f32, f32)],
    skeletons: bool,
) -> Vec<Finished> {
    let n = path.len();
    if n == 0 {
        return Vec::new();
    }
    let plain = letter_costs(keys, path);
    let track = Track {
        bends: bends(path),
        arc: arc(path),
        keys,
    };
    if !skeletons {
        return walk(lexicon.raw(), &plain, &track, None);
    }
    let costs: Vec<Option<Vec<f32>>> = LETTERS
        .iter()
        .map(|&l| {
            let rows = skeleton_keys(l)
                .iter()
                .filter_map(|&k| plain[usize::from(k - b'a')].as_ref());
            rows.fold(None::<Vec<f32>>, |acc, row| {
                Some(match acc {
                    None => row.clone(),
                    Some(a) => a.iter().zip(row).map(|(x, y)| x.min(*y)).collect(),
                })
            })
        })
        .collect();
    let trail: Vec<&Vec<f32>> = TRAIL
        .iter()
        .filter_map(|&t| plain[usize::from(t - b'a')].as_ref())
        .collect();
    let skeleton = Skeleton {
        trail: &trail,
        h: plain[usize::from(b'h' - b'a')].as_ref(),
    };
    walk(lexicon.raw(), &costs, &track, Some(&skeleton))
}

/// `end_cost`, also trying up to `TRAILING` dropped letters drawn after the last one.
fn end_with_trail(cost: &[f32], bends: &[f32], trail: &[&Vec<f32>]) -> f32 {
    let n = bends.len();
    let mut best = end_cost(cost, n);
    let mut frontier = vec![cost.to_vec()];
    for _ in 0..TRAILING {
        let mut next = Vec::new();
        for c in &frontier {
            for row in trail {
                let e = extend(c, row, bends);
                best = best.min(end_cost(&e, n));
                next.push(e);
            }
        }
        frontier = next;
    }
    best
}

/// What a skeleton lexicon's walk adds: the cost rows of `TRAIL` and of `h`.
struct Skeleton<'a> {
    trail: &'a [&'a Vec<f32>],
    h: Option<&'a Vec<f32>>,
}

/// `skeleton`: Some for a skeleton lexicon.
fn walk<D: AsRef<[u8]>>(
    fst: &Fst<D>,
    costs: &[Option<Vec<f32>>],
    track: &Track<'_>,
    skeleton: Option<&Skeleton<'_>>,
) -> Vec<Finished> {
    let (bends, n) = (&track.bends[..], track.bends.len());
    let root = fst.root();
    let mut beam = vec![Hyp {
        addr: root.addr(),
        out: Output::zero(),
        key: Vec::new(),
        cost: Vec::new(),
        line: 0.0,
        last: None,
        best: 0.0,
    }];
    let mut finished: Vec<Finished> = Vec::new();
    for _ in 0..MAX_LETTERS {
        let mut next: Vec<Hyp> = Vec::new();
        for hyp in &beam {
            let node = fst.node(hyp.addr);
            for t in node.transitions() {
                if hyp.key.is_empty() && t.inp < FIRST_WORD_BYTE {
                    continue;
                }
                let (cost, centre) = match step_for(t.inp) {
                    Step::Skip => continue,
                    // A word can't start with a silent mark.
                    Step::Silent if hyp.cost.is_empty() => continue,
                    Step::Silent => (hyp.cost.clone(), None),
                    Step::Letter(l) => {
                        let Some(row) = &costs[l] else { continue };
                        let cost = extend(&hyp.cost, row, bends);
                        let cost = match skeleton.and_then(|s| s.h) {
                            // With or without a drawn h after it, whichever fits each point.
                            Some(h) if ASPIRATED.contains(&t.inp) => {
                                let with_h = extend(&cost, h, bends);
                                cost.iter().zip(&with_h).map(|(a, b)| a.min(*b)).collect()
                            }
                            _ => cost,
                        };
                        (cost, track.keys.centre(t.inp))
                    }
                };
                let line = match (hyp.last, centre) {
                    (Some(a), Some(b)) => hyp.line + dist2(a, b).sqrt(),
                    _ => hyp.line,
                };
                let best = rank(&cost, &track.arc, line);
                if !best.is_finite() {
                    continue;
                }
                let mut key = hyp.key.clone();
                key.push(t.inp);
                let out = hyp.out.cat(t.out);
                let child = fst.node(t.addr);
                if let Some(skeleton) = skeleton {
                    if child.find_input(SEPARATOR).is_some() {
                        let end = end_with_trail(&cost, bends, skeleton.trail);
                        if end.is_finite() {
                            finished.push(Finished {
                                key: key.clone(),
                                count: 0,
                                cost: end,
                            });
                        }
                    }
                } else if child.is_final() {
                    let end = end_cost(&cost, n);
                    if end.is_finite() {
                        let count = out.cat(child.final_output()).value();
                        finished.push(Finished {
                            key: key.clone(),
                            count,
                            cost: end,
                        });
                    }
                }
                next.push(Hyp {
                    addr: t.addr,
                    out,
                    key,
                    cost,
                    line,
                    last: centre.or(hyp.last),
                    best,
                });
            }
        }
        if next.len() > BEAM {
            next.select_nth_unstable_by(BEAM, |a, b| a.best.total_cmp(&b.best));
            next.truncate(BEAM);
        }
        if finished.len() > 4 * FINISHED {
            keep_best(&mut finished);
        }
        if next.is_empty() {
            break;
        }
        beam = next;
    }
    keep_best(&mut finished);
    finished
}

fn keep_best(finished: &mut Vec<Finished>) {
    finished.sort_by(|a, b| a.cost.total_cmp(&b.cost));
    finished.truncate(FINISHED);
}

/// The costs after one more letter whose cost per point is `row`: at each point, the cheaper
/// of the previous letter on the same point and the best earlier point plus the bends skipped
/// between them, then the letter there. The first letter starts the path, a few points of
/// slack allowed.
fn extend(prev: &[f32], row: &[f32], bends: &[f32]) -> Vec<f32> {
    if prev.is_empty() {
        return row
            .iter()
            .enumerate()
            .map(|(j, &c)| {
                if j <= END_SLACK {
                    c + END_SKIP * j as f32
                } else {
                    f32::INFINITY
                }
            })
            .collect();
    }
    // min over k < j of prev[k] - BEND·bends[k]; skipping from k to j costs the bends between.
    let mut running = f32::INFINITY;
    (0..prev.len())
        .map(|j| {
            let skip = if j > 0 {
                running + bends[j - 1]
            } else {
                f32::INFINITY
            };
            let here = prev[j].min(skip) + row[j];
            running = running.min(prev[j] - bends[j]);
            here
        })
        .collect()
}

/// The cost of a word whose last letter is placed: it must be near the end of the path.
fn end_cost(cost: &[f32], n: usize) -> f32 {
    (n.saturating_sub(END_SLACK + 1)..n)
        .map(|j| cost[j] + END_SKIP * (n - 1 - j) as f32)
        .fold(f32::INFINITY, f32::min)
}

/// How well `word` (as typed, any case) fits a resampled path, by the beam's own measure;
/// infinite when a letter is far from the whole path. For words outside the lexicons (learned
/// words), which the beam never walks.
pub fn fit(keys: &KeyMap, path: &[(f32, f32)], word: &str) -> f32 {
    let bends = bends(path);
    let mut cost: Vec<f32> = Vec::new();
    for b in word.bytes() {
        match step_for(b) {
            Step::Letter(_) => {
                let Some(centre) = keys.centre(b) else {
                    return f32::INFINITY;
                };
                let row: Vec<f32> = path.iter().map(|&p| dist2(p, centre)).collect();
                if row.iter().copied().fold(f32::INFINITY, f32::min) > FAR {
                    return f32::INFINITY;
                }
                let row: Vec<f32> = row.into_iter().map(|d| d / SPREAD).collect();
                cost = extend(&cost, &row, &bends);
            }
            Step::Silent if !cost.is_empty() => {}
            _ => return f32::INFINITY,
        }
    }
    if cost.is_empty() {
        f32::INFINITY
    } else {
        end_cost(&cost, path.len())
    }
}

/// Weights of a swiped word's score:
/// `-align·fit - shape·shape_cost - length·length_cost + ln p(word) + context`.
/// Tuned on the synthetic swipe eval (`tests/swipe_eval.rs`); the eval may set others
/// (`Engine::set_swipe_weights`), the keyboard never does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Weights {
    pub align: f32,
    pub shape: f32,
    pub length: f32,
    /// Per `ln(1 + uses)` of a word the user has used.
    pub uses: f32,
    /// A learned word's stand-in for `ln p(word)`.
    pub learned: f32,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            align: 1.0,
            shape: 4.0,
            length: 4.0,
            uses: 0.5,
            learned: -12.0,
        }
    }
}
/// Skeletons expanded to their Malayalam words, and words per skeleton.
pub const SKELETONS: usize = 16;
pub const WORDS_PER_SKELETON: usize = 3;
/// Candidates returned.
pub const CANDIDATES: usize = 20;

/// How far a word's drawing is from the path, both ways: the mean distance from each path
/// point to the line through the word's keys, plus the mean distance from each key to the
/// path. Catches what the beam ignores: a path that wanders off to keys the word doesn't have.
pub fn shape_cost(keys: &KeyMap, path: &[(f32, f32)], word: &[u8]) -> f32 {
    let mut line: Vec<(f32, f32)> = Vec::with_capacity(word.len());
    for &b in word {
        if let Some(c) = keys.centre(b)
            && line.last() != Some(&c)
        {
            line.push(c);
        }
    }
    if line.is_empty() || path.is_empty() {
        return f32::INFINITY;
    }
    let to_line: f32 = path
        .iter()
        .map(|&p| distance_to_line(p, &line))
        .sum::<f32>()
        / path.len() as f32;
    let to_path: f32 = line
        .iter()
        .map(|&c| {
            path.iter()
                .map(|&p| dist2(p, c))
                .fold(f32::INFINITY, f32::min)
                .sqrt()
        })
        .sum::<f32>()
        / line.len() as f32;
    to_line + to_path
}

/// How much longer or shorter the path is than the line through the word's keys, as
/// `|ln(path / line)|`: a short word can't stand in for a long swipe that passes its keys.
pub fn length_cost(keys: &KeyMap, path: &[(f32, f32)], word: &[u8]) -> f32 {
    let line: Vec<(f32, f32)> = word.iter().filter_map(|&b| keys.centre(b)).collect();
    let length = |p: &[(f32, f32)]| p.windows(2).map(|w| dist2(w[0], w[1]).sqrt()).sum::<f32>();
    // Half a key each, so a one-key word or a tap-like path compares sensibly.
    ((length(path) + 0.5) / (length(&line) + 0.5)).ln().abs()
}

fn distance_to_line(p: (f32, f32), line: &[(f32, f32)]) -> f32 {
    if line.len() == 1 {
        return dist2(p, line[0]).sqrt();
    }
    line.windows(2)
        .map(|w| {
            let (a, b) = (w[0], w[1]);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
            dist2(p, (a.0 + dx * t, a.1 + dy * t))
        })
        .fold(f32::INFINITY, f32::min)
        .sqrt()
}

/// The QWERTY letter keys, one key width wide and tall, rows offset as on the keyboard: for
/// tests and the eval, which draw paths without an app.
pub fn qwerty() -> Vec<Key> {
    const ROWS: [(&[u8], f32); 3] = [(b"qwertyuiop", 0.0), (b"asdfghjkl", 0.5), (b"zxcvbnm", 1.5)];
    ROWS.iter()
        .enumerate()
        .flat_map(|(row, (letters, offset))| {
            letters.iter().enumerate().map(move |(col, &letter)| Key {
                letter,
                x: col as f32 + offset,
                y: row as f32,
                width: 1.0,
                height: 1.0,
            })
        })
        .collect()
}

/// The path a finger would draw for `word` without any wobble: straight lines through its keys'
/// centres, `per_key` points per key width. For tests and the eval.
pub fn ideal_path(keys: &KeyMap, word: &str, per_key: f32) -> Vec<(f32, f32)> {
    let centres: Vec<(f32, f32)> = word.bytes().filter_map(|b| keys.centre(b)).collect();
    let mut path = Vec::new();
    for w in centres.windows(2) {
        let steps = (dist2(w[0], w[1]).sqrt() * per_key).ceil().max(1.0) as usize;
        for s in 0..steps {
            let t = s as f32 / steps as f32;
            path.push((
                w[0].0 + (w[1].0 - w[0].0) * t,
                w[0].1 + (w[1].1 - w[0].1) * t,
            ));
        }
    }
    if let Some(&last) = centres.last() {
        path.push(last);
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexicon;

    fn lex(words: &[(&str, u64)]) -> Lexicon {
        Lexicon::from_bytes(
            lexicon::build(words.iter().map(|(w, c)| (w.as_bytes().to_vec(), *c))).unwrap(),
        )
        .unwrap()
    }

    fn keymap() -> KeyMap {
        KeyMap::new(&qwerty()).unwrap()
    }

    fn top(lexicon: &Lexicon, word: &str) -> Vec<String> {
        let keys = keymap();
        let path = resample(&ideal_path(&keys, word, 4.0), STEP, MAX_POINTS);
        decode(lexicon, &keys, &path, false)
            .into_iter()
            .map(|f| String::from_utf8(f.key).unwrap())
            .collect()
    }

    #[test]
    fn resampling_spaces_points_evenly() {
        let out = resample(&[(0.0, 0.0), (4.0, 0.0)], 1.0, MAX_POINTS);
        assert_eq!(
            out,
            vec![(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (3.0, 0.0), (4.0, 0.0)]
        );
        assert_eq!(resample(&[(1.0, 1.0)], 1.0, MAX_POINTS), vec![(1.0, 1.0)]);
        let long = resample(&[(0.0, 0.0), (1000.0, 0.0)], 0.4, MAX_POINTS);
        assert_eq!(long.len(), MAX_POINTS);
        assert_eq!(*long.last().unwrap(), (1000.0, 0.0));
    }

    #[test]
    fn key_map_measures_in_key_widths() {
        let keys: Vec<Key> = qwerty()
            .into_iter()
            .map(|k| Key {
                x: k.x * 40.0,
                y: k.y * 50.0,
                width: 40.0,
                height: 50.0,
                ..k
            })
            .collect();
        let map = KeyMap::new(&keys).unwrap();
        assert_eq!(map.centre(b'q'), Some((0.5, 0.625)));
        assert_eq!(map.centre(b'Q'), map.centre(b'q'));
        assert_eq!(map.centre(b'1'), None);
        assert_eq!(map.scale(&[(80.0, 40.0)]), vec![(2.0, 1.0)]);
    }

    #[test]
    fn a_word_is_found_from_its_own_path() {
        let lexicon = lex(&[
            ("hello", 50),
            ("help", 40),
            ("jello", 5),
            ("the", 100),
            ("hi", 30),
        ]);
        assert_eq!(top(&lexicon, "hello")[0], "hello");
        assert_eq!(top(&lexicon, "help")[0], "help");
        assert_eq!(top(&lexicon, "the")[0], "the");
    }

    #[test]
    fn a_doubled_letter_costs_nothing() {
        let lexicon = lex(&[("helo", 1), ("hello", 1)]);
        let found = top(&lexicon, "hello");
        assert!(found.contains(&"hello".to_string()) && found.contains(&"helo".to_string()));
    }

    #[test]
    fn capitals_and_apostrophes_follow_their_letters() {
        let lexicon = lex(&[("London", 10), ("don't", 10)]);
        assert_eq!(top(&lexicon, "london")[0], "London");
        assert_eq!(top(&lexicon, "dont")[0], "don't");
    }

    #[test]
    fn a_word_whose_keys_are_off_the_path_is_not_found() {
        let lexicon = lex(&[("cat", 1), ("zip", 1)]);
        assert_eq!(top(&lexicon, "cat"), vec!["cat"]);
    }

    #[test]
    fn manglish_skeletons_end_at_the_separator() {
        let lexicon = lex(&[("sukamanu\0x", 1), ("suk\0y", 1)]);
        let keys = keymap();
        let path = resample(&ideal_path(&keys, "sukamanu", 4.0), STEP, MAX_POINTS);
        let found = decode(&lexicon, &keys, &path, true);
        assert_eq!(found[0].key, b"sukamanu");
        // A skeleton that ends early is found, but its last letter is far from the path's end.
        let early = found.iter().find(|f| f.key == b"suk").unwrap();
        assert!(early.cost > found[0].cost + 4.0);
    }

    #[test]
    fn shape_prefers_the_word_the_path_draws() {
        let keys = keymap();
        let path = resample(&ideal_path(&keys, "cat", 4.0), STEP, MAX_POINTS);
        assert!(shape_cost(&keys, &path, b"cat") < shape_cost(&keys, &path, b"cart"));
        assert!(shape_cost(&keys, &path, b"cat") < 0.05);
    }
}
