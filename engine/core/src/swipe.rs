//! Swipe typing (docs/design/swipe.md): the finger's path across the QWERTY
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

// See `Fit`: a `Hyp` no larger than this is cut as it was before `Fit` had detours.
const _: () = assert!(std::mem::size_of::<Hyp>() <= 96);

struct Hyp {
    addr: usize,
    out: Output,
    key: Vec<u8>,
    fit: Fit,
    /// Length of the line through the word's keys so far, and its last key's centre.
    line: f32,
    last: Option<(f32, f32)>,
    /// The beam's ranking (`rank`).
    best: f32,
}

/// How a partial word fits the path. Per path point: the cheapest fit with the last letter
/// there and, for that fit, how much longer the dropped letters it draws inside the word
/// (`SkeletonRules::between`) make the line through the word's keys. The second is None while
/// no point's fit draws one, as it always is for a lexicon whose keys drop nothing.
///
/// The detours are boxed to keep a `Hyp` small: the beam is cut with an unstable selection,
/// and which of several equally ranked partial words it keeps follows from how std moves the
/// values, which it chooses by their size.
#[derive(Clone, Default)]
struct Fit {
    cost: Vec<f32>,
    #[allow(clippy::box_collection)]
    detour: Option<Box<Vec<f32>>>,
}

impl Fit {
    /// The fit after one more letter whose cost per point is `row` (`extend`), each point
    /// keeping the detours of the fit it goes on from.
    fn then(&self, row: &[f32], bends: &[f32]) -> Fit {
        let Some(detour) = &self.detour else {
            return Fit {
                cost: extend(&self.cost, row, bends),
                detour: None,
            };
        };
        let (cost, from) = extend_from(&self.cost, row, bends);
        Fit {
            cost,
            detour: Some(Box::new(from.iter().map(|&k| detour[k]).collect())),
        }
    }

    /// At each point, the cheaper of this fit and `other`.
    fn or(mut self, other: &Fit) -> Fit {
        if !self.cost.iter().zip(&other.cost).any(|(a, b)| b < a) {
            return self;
        }
        if self.detour.is_none() && other.detour.is_none() {
            return Fit {
                cost: self.cost.iter().zip(&other.cost).map(|(a, b)| a.min(*b)).collect(),
                detour: None,
            };
        }
        let n = self.cost.len();
        let detour = self.detour.get_or_insert_with(|| Box::new(vec![0.0; n]));
        for j in 0..n {
            if other.cost[j] < self.cost[j] {
                self.cost[j] = other.cost[j];
                detour[j] = other.detour.as_ref().map_or(0.0, |d| d[j]);
            }
        }
        self
    }

    /// The same fit of a drawing that went by way of a dropped letter, `extra` longer.
    fn by_way(mut self, extra: f32) -> Fit {
        let n = self.cost.len();
        let detour = self.detour.get_or_insert_with(|| Box::new(vec![0.0; n]));
        detour.iter_mut().for_each(|d| *d += extra);
        self
    }
}

/// Per key width between the line through a partial word's keys and the path up to its
/// best point (`rank`).
const PROGRESS: f32 = 1.0;

/// Ranks a partial word in the beam: its fit at the point where it fits best, plus how far the
/// line through its keys (and the dropped letters that fit draws) is from the path's length up
/// to that point. Without the second term, prefixes that jump ahead along the path look as
/// good as ones that follow it.
fn rank(fit: &Fit, arc: &[f32], line: f32) -> f32 {
    let Some(detour) = &fit.detour else {
        return fit
            .cost
            .iter()
            .zip(arc)
            .map(|(&c, &a)| c + PROGRESS * (a - line).abs())
            .fold(f32::INFINITY, f32::min);
    };
    fit.cost
        .iter()
        .zip(detour.iter())
        .zip(arc)
        .map(|((&c, &d), &a)| c + PROGRESS * (a - line - d).abs())
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

/// What a pack's skeleton means for a swipe, from its `romanisation.json`
/// (`Romanisation::swipe_rules`): a skeleton letter stands for every spelling the skeleton
/// folds into it, so the finger may have drawn any of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkeletonRules {
    /// Per skeleton letter, the keys it may be drawn through, its own first (Malayalam: `k` is
    /// also `c`, `q` and the `g` of `gh`; `t` is `d`; `i` is the `e` of `ee`).
    keys: [Vec<u8>; 26],
    /// Skeleton letters people write with an `h` the skeleton drops (`th`, `kh`, `sh` …): the
    /// path may pass through `h` after them, bending there, at no cost.
    aspirated: Vec<u8>,
    /// The letters a skeleton drops from a word's end (`veedu` → `veed`, `entey` → `ent`): the
    /// path may still draw up to `TRAILING` of them after the skeleton's last letter.
    trail: Vec<u8>,
    /// The letters a skeleton drops between two consonants (Hindi's `a`: `namaste` is `nmste`,
    /// `karne` is `krne`): the path may draw one between any two consonants of a skeleton.
    between: Vec<u8>,
    /// Letters people write for that dropped vowel beside one consonant (Hindi's `e` beside
    /// `h`: `kehte` for `khte`, `behen` for `bhn`), each with its consonants: the path may
    /// draw one where `may_stand_in` says.
    beside: Vec<(u8, Vec<u8>)>,
}

impl Default for SkeletonRules {
    /// Every letter is its own key, and nothing is dropped: a skeleton that folds nothing.
    fn default() -> Self {
        Self::new(&[], b"", b"")
    }
}

impl SkeletonRules {
    /// `also`: (skeleton letter, another key it may be drawn through), in the order they are
    /// tried when two keys are equally near the path. Anything but a–z is left out.
    pub fn new(also: &[(u8, u8)], aspirated: &[u8], trail: &[u8]) -> Self {
        let mut keys: [Vec<u8>; 26] = std::array::from_fn(|l| vec![LETTERS[l]]);
        for &(letter, key) in also {
            if letter.is_ascii_lowercase() && key.is_ascii_lowercase() {
                let row = &mut keys[usize::from(letter - b'a')];
                if !row.contains(&key) {
                    row.push(key);
                }
            }
        }
        Self {
            keys,
            aspirated: distinct_letters(aspirated),
            trail: distinct_letters(trail),
            between: Vec::new(),
            beside: Vec::new(),
        }
    }

    /// The same rules for a skeleton that drops `letters` between two consonants
    /// (`dropBetweenConsonants`).
    pub fn dropping_between(mut self, letters: &[u8]) -> Self {
        self.between = distinct_letters(letters);
        self
    }

    /// The same rules with `letters` (`alsoBesideConsonant`): each a letter written for the
    /// dropped vowel, and the consonants it is written beside.
    pub fn also_beside(mut self, letters: &[(u8, Vec<u8>)]) -> Self {
        self.beside = letters
            .iter()
            .filter(|(letter, _)| letter.is_ascii_lowercase())
            .map(|(letter, consonants)| (*letter, distinct_letters(consonants)))
            .collect();
        self
    }

    /// Whether `drawing` (`drawn`) has a letter written for a dropped one (`also_beside`) that
    /// `skeleton` does not: the path was read as going by way of it.
    pub fn stood_in(&self, skeleton: &[u8], drawing: &[u8]) -> bool {
        let count = |letters: &[u8], l: u8| letters.iter().filter(|&&b| b == l).count();
        self.beside
            .iter()
            .any(|(letter, _)| count(drawing, *letter) > count(skeleton, *letter))
    }

    /// The keys a skeleton letter (a–z) may be drawn through.
    fn keys(&self, letter: u8) -> &[u8] {
        &self.keys[usize::from(letter - b'a')]
    }
}

const LETTERS: [u8; 26] = *b"abcdefghijklmnopqrstuvwxyz";

/// The a–z of `s`, each once, in order.
fn distinct_letters(s: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    for &b in s {
        if b.is_ascii_lowercase() && !out.contains(&b) {
            out.push(b);
        }
    }
    out
}

/// Whether a letter written beside `consonants` for the dropped vowel may be drawn between
/// the skeleton's consonants `prev` and `next`: `next` is one of them (`k`, e, `h` of `khte`),
/// or `prev` is and the letter before it, `before`, is a consonant too, the vowel between
/// them dropped (`h`, e, `n` of `bhn`; not `h`, e, `r` of `hr`, nor of `mohr`).
fn may_stand_in(consonants: &[u8], before: Option<u8>, prev: u8, next: u8) -> bool {
    consonants.contains(&next) || (consonants.contains(&prev) && before.is_some_and(is_consonant))
}

fn is_consonant(b: u8) -> bool {
    b.is_ascii_lowercase() && !matches!(b, b'a' | b'e' | b'i' | b'o' | b'u')
}

/// The key of a skeleton letter (a–z) nearest the path: one of `SkeletonRules::keys`.
fn nearest_key(keys: &KeyMap, path: &[(f32, f32)], letter: u8, rules: &SkeletonRules) -> u8 {
    let near = |k: u8| {
        keys.centre(k).map_or(f32::INFINITY, |c| {
            path.iter()
                .map(|&p| dist2(p, c))
                .fold(f32::INFINITY, f32::min)
        })
    };
    rules
        .keys(letter)
        .iter()
        .copied()
        .min_by(|&a, &b| near(a).total_cmp(&near(b)))
        .unwrap_or(letter)
}

/// A step of a skeleton's drawing (`drawn`): the keys it may be drawn through, whether the
/// path may leave it out, and whether it counts as part of the drawing.
struct Stroke<'a> {
    keys: std::borrow::Cow<'a, [u8]>,
    optional: bool,
    shown: bool,
}

/// The keys a skeleton was most likely drawn through, for `shape_cost` and `length_cost`, which
/// compare a drawing with the path: the skeleton's cheapest drawing by the beam's own measure,
/// with the letters it drops between consonants (`SkeletonRules::between`) where the path went
/// to one. Each letter is drawn through the key of its own (`SkeletonRules::keys`) nearest the
/// path. An `h` after a letter is fitted, as the beam fits it, but is not part of the drawing.
pub fn drawn(keys: &KeyMap, path: &[(f32, f32)], skeleton: &[u8], rules: &SkeletonRules) -> Vec<u8> {
    let plain = |l: u8| {
        if l.is_ascii_lowercase() {
            nearest_key(keys, path, l, rules)
        } else {
            l
        }
    };
    let between = &rules.between;
    let mut strokes: Vec<Stroke<'_>> = Vec::new();
    for (i, l) in skeleton.iter().enumerate() {
        if i > 0 && !between.is_empty() && is_consonant(skeleton[i - 1]) && is_consonant(*l) {
            let before = i.checked_sub(2).map(|k| skeleton[k]);
            let mut keys = std::borrow::Cow::Borrowed(&between[..]);
            for (letter, consonants) in &rules.beside {
                if may_stand_in(consonants, before, skeleton[i - 1], *l) && !keys.contains(letter) {
                    keys.to_mut().push(*letter);
                }
            }
            strokes.push(Stroke {
                keys,
                optional: true,
                shown: true,
            });
        }
        strokes.push(Stroke {
            keys: std::slice::from_ref(l).into(),
            optional: false,
            shown: true,
        });
        if rules.aspirated.contains(l) {
            strokes.push(Stroke {
                keys: b"h"[..].into(),
                optional: true,
                shown: false,
            });
        }
    }
    let straight = || skeleton.iter().map(|&l| plain(l)).collect::<Vec<u8>>();
    if between.is_empty() || path.is_empty() {
        return straight();
    }
    // The beam's walk for this one skeleton, remembering for each stroke and point which key
    // was drawn there (None: the stroke was left out) and the point the fit goes on from.
    let bends = bends(path);
    let n = path.len();
    let mut cost: Vec<f32> = Vec::new();
    let mut steps: Vec<Vec<(Option<u8>, usize)>> = Vec::with_capacity(strokes.len());
    for stroke in &strokes {
        let mut row = vec![f32::INFINITY; n];
        let mut key = vec![0u8; n];
        for &k in stroke.keys.iter() {
            // A skeleton letter is fitted through any of its keys, as the beam fits it.
            let through: &[u8] = if stroke.shown && k.is_ascii_lowercase() {
                rules.keys(k)
            } else {
                std::slice::from_ref(&k)
            };
            for centre in through.iter().filter_map(|&t| keys.centre(t)) {
                for (j, &p) in path.iter().enumerate() {
                    let c = dist2(p, centre) / SPREAD;
                    if c < row[j] {
                        (row[j], key[j]) = (c, k);
                    }
                }
            }
        }
        let (mut next, from) = extend_from(&cost, &row, &bends);
        let mut step: Vec<(Option<u8>, usize)> =
            key.iter().zip(from).map(|(&k, from)| (Some(k), from)).collect();
        if stroke.optional {
            for j in 0..n {
                if cost[j] <= next[j] {
                    (next[j], step[j]) = (cost[j], (None, j));
                }
            }
        }
        cost = next;
        steps.push(step);
    }
    // The word ends where it fits best, later points first when the fit is as good.
    let Some(mut at) = (0..n)
        .rev()
        .min_by(|&a, &b| {
            let end = |j: usize| cost[j] + END_SKIP * (n - 1 - j) as f32;
            end(a).total_cmp(&end(b))
        })
        .filter(|&j| cost[j].is_finite())
    else {
        return straight();
    };
    let mut out: Vec<u8> = Vec::with_capacity(strokes.len());
    for (stroke, step) in strokes.iter().zip(&steps).rev() {
        let (key, from) = step[at];
        if let Some(key) = key.filter(|_| stroke.shown) {
            out.push(plain(key));
        }
        at = from;
    }
    out.reverse();
    out
}

/// Most dropped letters (`SkeletonRules::trail`) the path may draw after a skeleton's last.
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

/// The finished words of `lexicon` for a resampled `path`, best first. With `skeletons` (the
/// pack's rules), keys are `skeleton\0…` and a word ends where the separator follows; otherwise
/// where a key ends.
pub fn decode(
    lexicon: &Lexicon,
    keys: &KeyMap,
    path: &[(f32, f32)],
    skeletons: Option<&SkeletonRules>,
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
    let Some(rules) = skeletons else {
        return walk(lexicon.raw(), &plain, &track, None);
    };
    let costs: Vec<Option<Vec<f32>>> = LETTERS
        .iter()
        .map(|&l| {
            let rows = rules
                .keys(l)
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
    let trail: Vec<&Vec<f32>> = rules
        .trail
        .iter()
        .filter_map(|&t| plain[usize::from(t - b'a')].as_ref())
        .collect();
    // A dropped letter is drawn through the key of its own that is nearest the path.
    let between: Vec<Between<'_>> = rules
        .between
        .iter()
        .filter_map(|&b| {
            Some(Between {
                row: costs[usize::from(b - b'a')].as_ref()?,
                centre: keys.centre(nearest_key(keys, path, b, rules))?,
            })
        })
        .collect();
    let beside: Vec<(Between<'_>, &[u8])> = rules
        .beside
        .iter()
        .filter(|_| !between.is_empty())
        .filter_map(|(b, consonants)| {
            Some((
                Between {
                    row: costs[usize::from(b - b'a')].as_ref()?,
                    centre: keys.centre(nearest_key(keys, path, *b, rules))?,
                },
                &consonants[..],
            ))
        })
        .collect();
    let skeleton = Skeleton {
        trail: &trail,
        h: plain[usize::from(b'h' - b'a')].as_ref(),
        aspirated: &rules.aspirated,
        between: &between,
        beside: &beside,
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

/// What a skeleton lexicon's walk adds: the cost rows of the trailing letters and of `h`, the
/// letters an `h` may follow, and the letters dropped between consonants.
struct Skeleton<'a> {
    trail: &'a [&'a Vec<f32>],
    h: Option<&'a Vec<f32>>,
    aspirated: &'a [u8],
    between: &'a [Between<'a>],
    /// The letters written for a dropped one beside the consonants named (`may_stand_in`).
    beside: &'a [(Between<'a>, &'a [u8])],
}

/// A letter the skeleton drops between two consonants: its cost per path point and the centre
/// of the key it is drawn through.
struct Between<'a> {
    row: &'a Vec<f32>,
    centre: (f32, f32),
}

/// The length of the line from one key's centre to the next; nothing before the first key or
/// for a mark that has none.
fn reach(from: Option<(f32, f32)>, to: Option<(f32, f32)>) -> f32 {
    match (from, to) {
        (Some(a), Some(b)) => dist2(a, b).sqrt(),
        _ => 0.0,
    }
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
        fit: Fit::default(),
        line: 0.0,
        last: None,
        best: 0.0,
    }];
    let mut finished: Vec<Finished> = Vec::new();
    for _ in 0..MAX_LETTERS {
        let mut next: Vec<Hyp> = Vec::new();
        for hyp in &beam {
            let node = fst.node(hyp.addr);
            // After a consonant, the letters the skeleton drops before another one: the costs
            // with each drawn after the word so far, worked out when a consonant follows.
            let dropped = skeleton
                .map(|s| s.between)
                .filter(|_| hyp.key.last().is_some_and(|&b| is_consonant(b)))
                .unwrap_or_default();
            let mut through: Option<Vec<Fit>> = None;
            // The same for a letter written for a dropped one beside some consonants.
            let beside = skeleton
                .map(|s| s.beside)
                .filter(|_| hyp.key.last().is_some_and(|&b| is_consonant(b)))
                .unwrap_or_default();
            let mut through_beside: Vec<Option<Fit>> = vec![None; beside.len()];
            for t in node.transitions() {
                if hyp.key.is_empty() && t.inp < FIRST_WORD_BYTE {
                    continue;
                }
                let (fit, centre) = match step_for(t.inp) {
                    Step::Skip => continue,
                    // A word can't start with a silent mark.
                    Step::Silent if hyp.fit.cost.is_empty() => continue,
                    Step::Silent => (hyp.fit.clone(), None),
                    Step::Letter(l) => {
                        let Some(row) = &costs[l] else { continue };
                        let h = skeleton
                            .and_then(|s| Some((s.h?, s.aspirated)))
                            .and_then(|(h, aspirated)| aspirated.contains(&t.inp).then_some(h));
                        let place = |from: &Fit| {
                            let fit = from.then(row, bends);
                            match h {
                                // With or without a drawn h after it, whichever fits each point.
                                Some(h) => {
                                    let with_h = fit.then(h, bends);
                                    fit.or(&with_h)
                                }
                                None => fit,
                            }
                        };
                        let centre = track.keys.centre(t.inp);
                        let mut fit = place(&hyp.fit);
                        // Straight to this consonant or by way of a dropped letter, whichever
                        // fits each point.
                        if !dropped.is_empty() && is_consonant(t.inp) {
                            let through = through.get_or_insert_with(|| {
                                dropped.iter().map(|d| hyp.fit.then(d.row, bends)).collect()
                            });
                            for (d, via) in dropped.iter().zip(through.iter()) {
                                let extra = reach(hyp.last, Some(d.centre))
                                    + reach(Some(d.centre), centre)
                                    - reach(hyp.last, centre);
                                fit = fit.or(&place(via).by_way(extra));
                            }
                        }
                        if !beside.is_empty() && is_consonant(t.inp) {
                            let (prev, before) = match hyp.key[..] {
                                [.., before, prev] => (prev, Some(before)),
                                [prev] => (prev, None),
                                [] => unreachable!("beside is empty before the first letter"),
                            };
                            for ((d, consonants), via) in beside.iter().zip(&mut through_beside) {
                                if !may_stand_in(consonants, before, prev, t.inp) {
                                    continue;
                                }
                                let via = via.get_or_insert_with(|| hyp.fit.then(d.row, bends));
                                let extra = reach(hyp.last, Some(d.centre))
                                    + reach(Some(d.centre), centre)
                                    - reach(hyp.last, centre);
                                fit = fit.or(&place(via).by_way(extra));
                            }
                        }
                        (fit, centre)
                    }
                };
                let line = hyp.line + reach(hyp.last, centre);
                let best = rank(&fit, &track.arc, line);
                if !best.is_finite() {
                    continue;
                }
                let mut key = hyp.key.clone();
                key.push(t.inp);
                let out = hyp.out.cat(t.out);
                let child = fst.node(t.addr);
                if let Some(skeleton) = skeleton {
                    if child.find_input(SEPARATOR).is_some() {
                        let end = end_with_trail(&fit.cost, bends, skeleton.trail);
                        if end.is_finite() {
                            finished.push(Finished {
                                key: key.clone(),
                                count: 0,
                                cost: end,
                            });
                        }
                    }
                } else if child.is_final() {
                    let end = end_cost(&fit.cost, n);
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
                    fit,
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

/// `extend`, and for each point the earlier point (or itself) its fit goes on from.
fn extend_from(prev: &[f32], row: &[f32], bends: &[f32]) -> (Vec<f32>, Vec<usize>) {
    if prev.is_empty() {
        return (extend(prev, row, bends), (0..row.len()).collect());
    }
    let (mut running, mut running_at) = (f32::INFINITY, 0);
    (0..prev.len())
        .map(|j| {
            let skip = if j > 0 {
                running + bends[j - 1]
            } else {
                f32::INFINITY
            };
            let from = if skip < prev[j] { running_at } else { j };
            let here = prev[j].min(skip) + row[j];
            if prev[j] - bends[j] < running {
                (running, running_at) = (prev[j] - bends[j], j);
            }
            (here, from)
        })
        .unzip()
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
    /// Taken from a word found by its second skeleton (`Romanisation::word_skeletons`: नहीं
    /// drawn as `nahi`): it is drawn that way only some of the time, and without this the
    /// second keys cost Hindi's English swipes 8 firsts in 1,789 (5 with it).
    pub second: f32,
    /// Taken from a skeleton the path fits by way of a letter written for a dropped one
    /// (`SkeletonRules::stood_in`: Hindi's `khte` drawn k-e-h-t-e): the letter is as often
    /// itself. Over 200 wobbly swipes each of Hindi words without a chat-forms row, at 0, 2
    /// and 4: `lehar` gave लहर 189, 152 and 29 times and `behas` बहस 170, 115 and 38, while
    /// `behtar` gave बेहतर 30, 137 and 159 times and `sehat` सेहत 9, 63 and 79 (59 and 56
    /// before the rule). `chehra` stays behind कहा (चेहरा 13, 19 and 64; 73 before).
    pub stand_in: f32,
    /// An English word at least this likely (`ln p(word)` in the English lexicon: `hello`
    /// −12.0, `tomorrow` −11.6) is a common one, which a pack's word may outrank on a swipe
    /// only by fitting the path better (`english_margin`). Without it a word drawn loosely
    /// through the path (টিউমাৰটো, `tiumorotu`, for `tomorrow`) won on its pack's context
    /// and frequency. At −12, −13 and −14, common English words swiped after an Assamese
    /// word (`swipe_mix`) came first 912, 929 and 931 times in 1,100 (682 without the rule),
    /// and Assamese held-out words in the lexicon 0.3034, 0.3022 and 0.2980 (0.3022 without).
    pub english_common: f32,
    /// How much lower a pack's word's cost on the path (the beam's `Finished::cost`) must be
    /// than a common English word's for the pack's word to be ranked above it. At 0, 1 and 2
    /// (with `english_common` −12): English after an Assamese word first 829, 904 and 951
    /// times in 1,100, Assamese held-out words in the lexicon 0.3022, 0.3022 and 0.2945.
    pub english_margin: f32,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            align: 1.0,
            shape: 4.0,
            length: 4.0,
            uses: 0.5,
            learned: -12.0,
            second: 1.0,
            stand_in: 2.0,
            english_common: -13.0,
            english_margin: 1.0,
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
        decode(lexicon, &keys, &path, None)
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
        let rules = crate::romanisation::ml().swipe_rules();
        let found = decode(&lexicon, &keys, &path, Some(rules));
        assert_eq!(found[0].key, b"sukamanu");
        // A skeleton that ends early is found, but its last letter is far from the path's end.
        let early = found.iter().find(|f| f.key == b"suk").unwrap();
        assert!(early.cost > found[0].cost + 4.0);
    }

    /// What `skeleton_keys`, `ASPIRATED` and `TRAIL` were when they were written by hand for
    /// Malayalam: its tables give the same.
    #[test]
    fn malayalams_tables_give_the_rules_that_were_code() {
        let rules = crate::romanisation::ml().swipe_rules();
        for letter in LETTERS {
            let was: &[u8] = match letter {
                b'k' => b"kcqg",
                b't' => b"td",
                b'p' => b"pf",
                b'v' => b"vw",
                b'i' => b"ie",
                b'u' => b"uo",
                _ => std::slice::from_ref(&LETTERS[usize::from(letter - b'a')]),
            };
            assert_eq!(rules.keys(letter), was, "{}", letter as char);
        }
        let sorted = |s: &[u8]| {
            let mut v = s.to_vec();
            v.sort_unstable();
            v
        };
        assert_eq!(sorted(&rules.aspirated), sorted(b"ktpbcjsz"));
        assert_eq!(sorted(&rules.trail), sorted(b"euyh"));
    }

    #[test]
    fn another_packs_skeleton_is_drawn_through_its_own_keys() {
        let pack = |id: &str| {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../packs/{id}.neopack/romanisation.json"));
            crate::romanisation::Romanisation::load(&path).unwrap()
        };
        // Hindi keeps d and t apart, and says its own aspirates and trailing letters.
        let hi = pack("hi");
        let rules = hi.swipe_rules();
        assert_eq!(rules.keys(b't'), b"t");
        assert_eq!(rules.keys(b'd'), b"d");
        assert_eq!(rules.keys(b'j'), b"jz");
        assert_eq!(rules.aspirated, b"ktpbcjsdgz");
        assert_eq!(rules.trail, b"h");
        // Tamil folds every voiced stop: `k` is drawn through g too, `s` through j and c.
        let ta = pack("ta");
        let rules = ta.swipe_rules();
        assert_eq!(rules.keys(b'k'), b"kgq");
        assert_eq!(rules.keys(b's'), b"scj");
        assert!(rules.trail.is_empty(), "Tamil's skeleton drops nothing from the end");
        // Assamese drops a final a, and its o is an a: either may be drawn after the end.
        let as_ = pack("as");
        assert_eq!(as_.swipe_rules().trail, b"ao");
        assert_eq!(as_.swipe_rules().keys(b'a'), b"ao");
        // So a swipe of `bhalo` ends on o at no cost to the skeleton `bal`; with only the a
        // allowed after the end, the stretch to o is a word that stops short.
        let lexicon = lex(&[("bal\0x", 1)]);
        let path = resample(&ideal_path(&keymap(), "bhalo", 4.0), STEP, MAX_POINTS);
        let cost = |rules: &SkeletonRules| decode(&lexicon, &keymap(), &path, Some(rules))[0].cost;
        let only_a = SkeletonRules::new(&[(b'a', b'o')], b"b", b"a");
        assert!(cost(&only_a) > cost(as_.swipe_rules()) + 2.0);
        // Only Hindi drops a letter inside a word.
        assert_eq!(hi.swipe_rules().between, b"a");
        assert_eq!(hi.swipe_rules().beside, [(b'e', b"h".to_vec())]);
        for other in [ta.swipe_rules(), as_.swipe_rules(), crate::romanisation::ml().swipe_rules()] {
            assert!(other.between.is_empty());
        }
        // A path through d-a-l is Hindi's `dal`, and its `tal` only at the cost of the t key's
        // distance; under Malayalam's rules the same path draws either as well.
        let lexicon = lex(&[("dal\0x", 1), ("tal\0y", 1)]);
        let keys = keymap();
        let path = resample(&ideal_path(&keys, "dal", 4.0), STEP, MAX_POINTS);
        let cost = |rules: &SkeletonRules, key: &[u8]| {
            let found = decode(&lexicon, &keys, &path, Some(rules));
            found.iter().find(|f| f.key == key).map_or(f32::INFINITY, |f| f.cost)
        };
        let (hi, ml) = (hi.swipe_rules(), crate::romanisation::ml().swipe_rules());
        assert!(cost(hi, b"tal") > cost(hi, b"dal") + 4.0);
        assert_eq!(cost(ml, b"tal"), cost(ml, b"dal"));
    }

    /// A skeleton that drops `a` between consonants (Hindi's): the path still goes to the `a`
    /// key there, and the skeleton fits it as well as one that keeps the letter.
    #[test]
    fn a_letter_dropped_between_consonants_may_be_drawn() {
        let lexicon = lex(&[
            ("pyr\0x", 1),
            ("nmste\0x", 1),
            ("krne\0x", 1),
            ("srkr\0x", 1),
            ("pyar\0x", 1),
            ("namaste\0x", 1),
            ("karne\0x", 1),
            ("sarkar\0x", 1),
        ]);
        let keys = keymap();
        let dropping = SkeletonRules::default().dropping_between(b"a");
        let find = |rules: &SkeletonRules, drawn: &str, skeleton: &[u8]| {
            let path = resample(&ideal_path(&keys, drawn, 4.0), STEP, MAX_POINTS);
            let found = decode(&lexicon, &keys, &path, Some(rules));
            found.into_iter().find(|f| f.key == skeleton)
        };
        for (spelt, skeleton) in [
            ("pyar", &b"pyr"[..]),
            ("namaste", b"nmste"),
            ("karne", b"krne"),
            ("sarkar", b"srkr"),
        ] {
            let with = find(&dropping, spelt, skeleton).unwrap_or_else(|| panic!("{spelt}: not found"));
            let kept = find(&dropping, spelt, spelt.as_bytes()).unwrap();
            assert!(with.cost <= kept.cost + 0.01, "{spelt}: {} and {}", with.cost, kept.cost);
            // Without the rule the detour is a bend the skeleton leaves unmatched.
            let without = find(&SkeletonRules::default(), spelt, skeleton)
                .map_or(f32::INFINITY, |f| f.cost);
            assert!(without > with.cost + 2.0, "{spelt}: {without} without the rule");
        }
    }

    /// The drawing compared with the path has the dropped letters the path went to, and only
    /// those.
    #[test]
    fn the_drawing_of_a_skeleton_has_the_dropped_letters_drawn() {
        let keys = keymap();
        let dropping = SkeletonRules::default().dropping_between(b"a");
        let path = |spelt: &str| resample(&ideal_path(&keys, spelt, 4.0), STEP, MAX_POINTS);
        for (spelt, skeleton) in [
            ("pyar", &b"pyr"[..]),
            ("namaste", b"nmste"),
            ("karne", b"krne"),
            ("sarkar", b"srkr"),
            ("dhanyavad", b"dnyvd"),
            // No letter dropped, or none drawn: the drawing is the skeleton.
            ("hindi", b"hindi"),
            ("krne", b"krne"),
            ("dost", b"dost"),
        ] {
            let drawing = drawn(&keys, &path(spelt), skeleton, &dropping);
            let spelt_without_h: Vec<u8> = spelt.bytes().filter(|&b| b != b'h' || skeleton.contains(&b)).collect();
            assert_eq!(
                String::from_utf8(drawing).unwrap(),
                String::from_utf8(spelt_without_h).unwrap(),
                "{spelt}"
            );
        }
        // A skeleton letter is drawn through the key of its own nearest the path, as before.
        let folding = SkeletonRules::new(&[(b'k', b'c')], b"", b"");
        assert_eq!(drawn(&keys, &path("carne"), b"krne", &folding), b"crne");
        assert_eq!(drawn(&keys, &path("carne"), b"krne", &folding.dropping_between(b"a")), b"carne");
        // Without the rule nothing is added.
        assert_eq!(drawn(&keys, &path("karne"), b"krne", &SkeletonRules::default()), b"krne");
        assert_eq!(drawn(&keys, &[], b"krne", &dropping), b"krne");
    }

    /// A letter written for the dropped vowel beside one consonant (Hindi's `e` beside `h`)
    /// may be drawn there, and nowhere else.
    #[test]
    fn a_letter_written_beside_a_consonant_for_the_dropped_one_may_be_drawn() {
        let lexicon = lex(&[
            ("khte\0x", 1),
            ("bhn\0x", 1),
            ("hr\0x", 1),
            ("mohn\0x", 1),
            ("krne\0x", 1),
            ("kehte\0x", 1),
            ("behen\0x", 1),
            ("her\0x", 1),
            ("mohen\0x", 1),
            ("kerne\0x", 1),
        ]);
        let keys = keymap();
        let dropping = SkeletonRules::default().dropping_between(b"a");
        let beside = dropping.clone().also_beside(&[(b'e', b"h".to_vec())]);
        let cost = |rules: &SkeletonRules, drawn: &str, skeleton: &[u8]| {
            let path = resample(&ideal_path(&keys, drawn, 4.0), STEP, MAX_POINTS);
            let found = decode(&lexicon, &keys, &path, Some(rules));
            found.into_iter().find(|f| f.key == skeleton).map_or(f32::INFINITY, |f| f.cost)
        };
        let path = |spelt: &str| resample(&ideal_path(&keys, spelt, 4.0), STEP, MAX_POINTS);
        // Before the h, and after it when the vowel before it is dropped too.
        for (spelt, skeleton) in [("kehte", &b"khte"[..]), ("behen", b"bhn"), ("bahen", b"bhn")] {
            let with = cost(&beside, spelt, skeleton);
            assert!(with <= cost(&beside, spelt, spelt.as_bytes()) + 0.01, "{spelt}: {with}");
            assert!(cost(&dropping, spelt, skeleton) > with + 2.0, "{spelt}: {with}");
            assert_eq!(drawn(&keys, &path(spelt), skeleton, &beside), spelt.as_bytes(), "{spelt}");
            assert!(beside.stood_in(skeleton, spelt.as_bytes()));
        }
        // Not after an h that begins the word or follows a vowel, nor away from an h: the
        // skeleton fits the path no better than without the rule, and its drawing is itself.
        for (spelt, skeleton) in [("her", &b"hr"[..]), ("mohen", b"mohn"), ("kerne", b"krne")] {
            assert_eq!(cost(&beside, spelt, skeleton), cost(&dropping, spelt, skeleton), "{spelt}");
            let drawing = drawn(&keys, &path(spelt), skeleton, &beside);
            assert_eq!(drawing, skeleton, "{spelt}");
            assert!(!beside.stood_in(skeleton, &drawing));
        }
        // The dropped letter itself is drawn as before, and is no stand-in.
        assert_eq!(drawn(&keys, &path("kahte"), b"khte", &beside), b"kahte");
        assert!(!beside.stood_in(b"khte", b"kahte"));
        assert!(may_stand_in(b"h", None, b'k', b'h'));
        assert!(may_stand_in(b"h", Some(b'b'), b'h', b'n'));
        assert!(!may_stand_in(b"h", None, b'h', b'r'));
        assert!(!may_stand_in(b"h", Some(b'o'), b'h', b'n'));
        assert!(!may_stand_in(b"h", Some(b'k'), b'r', b'n'));
    }

    #[test]
    fn shape_prefers_the_word_the_path_draws() {
        let keys = keymap();
        let path = resample(&ideal_path(&keys, "cat", 4.0), STEP, MAX_POINTS);
        assert!(shape_cost(&keys, &path, b"cat") < shape_cost(&keys, &path, b"cart"));
        assert!(shape_cost(&keys, &path, b"cat") < 0.05);
    }
}
