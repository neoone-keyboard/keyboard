//! A weighted Damerau–Levenshtein automaton for walking the English FST.
//! Costs are in sixteenths of a half-edit (`CHEAP_EDIT` is one half-edit), so neighbouring-key
//! slips and doubled letters are cheaper than other typos, and a slip read from where a tap
//! landed (`TapCosts`) can cost a fraction of one. The state is fixed-size, so a walk
//! allocates nothing however many keys it visits.

use fst::Automaton;

use crate::keys::adjacency_table;

/// Longest typed word that is corrected; longer ones are left alone.
pub const MAX_LEN: usize = 24;

/// Cost of one ordinary edit. `MAX_COST = 2 * EDIT` means "within two edits".
pub const EDIT: u8 = 2 * CHEAP_EDIT;
/// A substitution between neighbouring keys, two letters swapped, a missed
/// apostrophe, a doubled letter typed once or three times, or (for long words,
/// `WeightedEdit::with_cheap_vowels`) one vowel for another. (Swaps cost a
/// full edit until Task 17: `pani` then ranked `land` above `pain`.) One half-edit: costs are
/// counted in sixteenths of it, so a tap's slip (`TapCosts`) can cost a fraction of a
/// half-edit. Every cost without taps is a whole number of half-edits, as it always was.
pub const CHEAP_EDIT: u8 = 16;

/// `cost` in half-edits (`CHEAP_EDIT`), for the penalties that are counted per half-edit.
pub fn half_edits(cost: u8) -> f32 {
    f32::from(cost) / f32::from(CHEAP_EDIT)
}

/// Most letters a key may have after the part that matches the query, in tail mode
/// (`WeightedEdit::with_tail`): `apostrap` reaches `apostrophe` as `apostrop` + `he`.
pub const MAX_TAIL: usize = 2;

const INF: u8 = u8::MAX;

/// Where the finger landed for one typed character, as the keyboard read it: the character
/// typed (the key under the finger) with its probability, and up to two other letter keys the
/// tap could have meant, with theirs (`k` 0.7 typed, `j` 0.3 other). The keyboard works them
/// out from the touch point (a Gaussian around each key's centre) and keeps them only in
/// memory, for the word being typed: they never reach a log or a file, and the engine keeps
/// none of them past the suggest it is given them for. A letter with no `others` was tapped near
/// its key's centre: no other key is likely. A character typed some other way (pasted, from a
/// hardware keyboard) has no `Tap` at all.
#[derive(Debug, Clone, PartialEq)]
pub struct Tap {
    pub letter: char,
    pub probability: f32,
    pub others: Vec<(char, f32)>,
}

/// Per letter of `TapCosts`: what each letter `a`–`z` costs in the typed letter's place.
pub type TapRow = [u8; 26];

/// The keyboard names another key for a tap only when it has at least this probability: the
/// apps' `TapReading.floor` (iOS) and `TapReading.FLOOR` (Android).
pub const TAP_FLOOR: f32 = 0.05;

/// What putting another letter in a typed letter's place costs, from where its tap landed
/// (`Tap`): `scale` × ln(p(typed) / p(other)) half-edits, at least one unit and at most `EDIT`.
/// A tap between two keys makes the other key nearly free; one in the middle of its key makes
/// the other dearer than the flat neighbouring-key cost. A neighbouring key not among the
/// tap's `others` had less than `TAP_FLOOR`, and costs as if it had that; any other letter
/// costs `EDIT`, as without taps. The typed letter itself costs nothing. `scale` is
/// `rank::TAP_SCALE`.
pub fn tap_row(typed: u8, probability: f32, others: &[(u8, f32)], scale: f32) -> TapRow {
    let cost = |p: f32| {
        let units = f32::from(CHEAP_EDIT) * scale * (probability / p).ln();
        units.round().clamp(1.0, f32::from(EDIT)) as u8
    };
    let mut row = [EDIT; 26];
    if probability <= 0.0 {
        return row;
    }
    let unlisted = cost(TAP_FLOOR.min(probability));
    for (i, slot) in row.iter_mut().enumerate() {
        if crate::keys::adjacent(typed, b'a' + i as u8) {
            *slot = unlisted;
        }
    }
    for &(letter, p) in others {
        if !letter.is_ascii_lowercase() || letter == typed || p <= 0.0 {
            continue;
        }
        let slot = &mut row[usize::from(letter - b'a')];
        *slot = (*slot).min(cost(p));
    }
    if typed.is_ascii_lowercase() {
        row[usize::from(typed - b'a')] = 0;
    }
    row
}

/// The tap rows (`tap_row`) of a typed word, one per byte: `None` where the keyboard said
/// nothing of that letter (pasted text, a hardware keyboard, a letter before an edit), which
/// costs what it always did. Empty for a word typed without taps.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TapCosts {
    rows: Vec<Option<TapRow>>,
}

impl TapCosts {
    /// No taps: every cost as without them.
    pub fn none() -> Self {
        Self::default()
    }

    /// `typed`'s rows from `taps`, the last characters typed before the cursor, one per
    /// character, matched to `typed` from its end: each tap whose character is the word's
    /// there (ignoring ASCII case; any two characters that are not letters match) gives that
    /// byte its row, until one doesn't match or either runs out. A tap's letter and its others
    /// are read in lower case; a character that isn't an ASCII letter gets no row. `scale` as
    /// in `tap_row`.
    pub fn of(typed: &str, taps: &[Tap], scale: f32) -> Self {
        if taps.is_empty() || !typed.is_ascii() {
            return Self::none();
        }
        let bytes = typed.as_bytes();
        let mut rows = vec![None; bytes.len()];
        for (i, tap) in (0..bytes.len()).rev().zip(taps.iter().rev()) {
            let b = bytes[i];
            let letter = tap.letter;
            let same = if b.is_ascii_alphabetic() {
                letter.is_ascii() && (letter as u8).eq_ignore_ascii_case(&b)
            } else {
                !letter.is_alphabetic()
            };
            if !same {
                break;
            }
            if !b.is_ascii_alphabetic() {
                continue;
            }
            let others: Vec<(u8, f32)> = tap
                .others
                .iter()
                .filter(|(c, _)| c.is_ascii_alphabetic())
                .map(|&(c, p)| ((c as u8).to_ascii_lowercase(), p))
                .collect();
            rows[i] = Some(tap_row(b.to_ascii_lowercase(), tap.probability, &others, scale));
        }
        if rows.iter().all(Option::is_none) {
            return Self::none();
        }
        Self { rows }
    }

    /// Whether no letter has a row: the costs are those without taps.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Byte `i`'s row, if its tap gave one.
    pub fn row(&self, i: usize) -> Option<&TapRow> {
        self.rows.get(i).and_then(Option::as_ref)
    }

    /// The rows of the bytes `keep` keeps, in order: the letters of a word with its other
    /// characters left out, as `manglish::lookup_slips` reads it. Empty without taps.
    pub fn filtered(&self, typed: &[u8], keep: impl Fn(&u8) -> bool) -> Vec<Option<TapRow>> {
        if self.is_empty() {
            return Vec::new();
        }
        typed
            .iter()
            .enumerate()
            .filter(|(_, b)| keep(b))
            .map(|(i, _)| self.row(i).copied())
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct WeightedEdit {
    query: [u8; MAX_LEN],
    len: usize,
    max_cost: u8,
    neighbours: [u32; 26],
    cheap_vowels: bool,
    tail: bool,
    /// Each query letter's tap row (`TapCosts`), when the word was typed with taps.
    taps: TapCosts,
}

/// One DP row per byte read, plus the row before it for transpositions.
#[derive(Debug, Clone, Copy)]
pub struct EditState {
    prev: [u8; MAX_LEN + 1],
    cur: [u8; MAX_LEN + 1],
    last: Option<u8>,
    /// `ends[k]`: the cost of the whole query against the key read so far less its last
    /// `k` bytes (`ends[0]` is `cost`).
    ends: [u8; MAX_TAIL + 1],
}

impl EditState {
    /// The cost of matching the whole query against the key read so far.
    pub fn cost(&self, automaton: &WeightedEdit) -> u8 {
        self.cur[automaton.len]
    }

    /// In tail mode, the best reading of the key as a near match of the query followed by
    /// 1 to `MAX_TAIL` more letters: (cost of the matched part, letters after it). Fewer
    /// letters win a tie. None when no such reading is within the budget.
    pub fn tail(&self, automaton: &WeightedEdit) -> Option<(u8, usize)> {
        if !automaton.tail {
            return None;
        }
        (1..=MAX_TAIL)
            .map(|k| (self.ends[k], k))
            .filter(|&(cost, _)| cost <= automaton.max_cost)
            .min()
    }
}

impl WeightedEdit {
    /// None if `query` is empty, too long or not ASCII.
    pub fn new(query: &str, max_cost: u8) -> Option<Self> {
        let bytes = query.as_bytes();
        if bytes.is_empty() || bytes.len() > MAX_LEN || !query.is_ascii() {
            return None;
        }
        let mut q = [0u8; MAX_LEN];
        for (slot, b) in q.iter_mut().zip(bytes) {
            *slot = b.to_ascii_lowercase();
        }
        Some(Self {
            query: q,
            len: bytes.len(),
            max_cost,
            neighbours: adjacency_table(),
            cheap_vowels: false,
            tail: false,
            taps: TapCosts::none(),
        })
    }

    /// A substitution for a letter whose tap gave a row costs what the row says, not the flat
    /// neighbouring-key cost (`TapCosts`).
    pub fn with_taps(mut self, taps: TapCosts) -> Self {
        self.taps = taps;
        self
    }

    /// One vowel typed for another (`definately`, `prefarance`) costs `CHEAP_EDIT`.
    pub fn with_cheap_vowels(mut self) -> Self {
        self.cheap_vowels = true;
        self
    }

    /// Also matches keys that run up to `MAX_TAIL` letters past a near match of the query
    /// (`EditState::tail`).
    pub fn with_tail(mut self) -> Self {
        self.tail = true;
        self
    }

    fn q(&self, i: usize) -> u8 {
        self.query[i]
    }

    /// The key has a byte the typed word lacks. A missed apostrophe or a
    /// missed doubled letter (helo → hello) is cheap.
    fn insert_cost(byte: u8, previous: Option<u8>) -> u8 {
        if byte == b'\'' || previous == Some(byte) {
            CHEAP_EDIT
        } else {
            EDIT
        }
    }

    /// The typed word has an extra byte at position `i - 1`.
    fn delete_cost(&self, i: usize) -> u8 {
        if i >= 2 && self.q(i - 1) == self.q(i - 2) {
            CHEAP_EDIT
        } else {
            EDIT
        }
    }

    /// Key byte `key` in place of query letter `i` (`typed`).
    fn substitute_cost(&self, i: usize, typed: u8, key: u8) -> u8 {
        if typed == key {
            return 0;
        }
        let vowels = self.cheap_vowels && is_vowel(typed) && is_vowel(key);
        if let Some(row) = self.taps.row(i).filter(|_| key.is_ascii_lowercase()) {
            let tapped = row[usize::from(key - b'a')];
            return if vowels { tapped.min(CHEAP_EDIT) } else { tapped };
        }
        let near = typed.is_ascii_lowercase()
            && key.is_ascii_lowercase()
            && self.neighbours[usize::from(typed - b'a')] & (1 << (key - b'a')) != 0;
        if near || vowels { CHEAP_EDIT } else { EDIT }
    }
}

fn is_vowel(b: u8) -> bool {
    matches!(b, b'a' | b'e' | b'i' | b'o' | b'u')
}

/// Every spelling one slip from `word` (lower-case ASCII letters), each once with the least the
/// automaton above charges for that slip: a neighbouring key for a letter, two letters swapped,
/// a doubled letter typed once or a letter typed twice (`CHEAP_EDIT`); an extra letter or a
/// missed one (`EDIT`). Not another letter for one far from it: that is any word of the same
/// length. For the romanised lexicons, whose keys are skeletons, not letters, so the automaton
/// can't walk them (`manglish::lookup_slips`). Empty for a word with anything but `a`–`z`.
pub fn slips(word: &[u8]) -> Vec<(Vec<u8>, u8)> {
    slips_tapped(word, &[])
}

/// `slips`, with each letter's tap row (`TapCosts::filtered`: one per letter of `word`; empty,
/// or `None` for a letter, where there was no tap). A letter with a row is replaced by each of
/// its neighbours and each other letter its tap could have meant, at what the row says, as
/// the automaton charges it (`WeightedEdit::with_taps`).
pub fn slips_tapped(word: &[u8], taps: &[Option<TapRow>]) -> Vec<(Vec<u8>, u8)> {
    if word.is_empty() || !word.iter().all(u8::is_ascii_lowercase) {
        return Vec::new();
    }
    let neighbours = adjacency_table();
    let n = word.len();
    let mut found: std::collections::HashMap<Vec<u8>, u8> = std::collections::HashMap::new();
    let mut add = |spelling: Vec<u8>, cost: u8| {
        if spelling != word {
            let at = found.entry(spelling).or_insert(cost);
            *at = (*at).min(cost);
        }
    };
    for i in 0..n {
        let letter = usize::from(word[i] - b'a');
        let row = taps.get(i).and_then(Option::as_ref);
        for b in b'a'..=b'z' {
            let near = neighbours[letter] & (1 << (b - b'a')) != 0;
            let cost = match row {
                Some(row) if b != word[i] && (near || row[usize::from(b - b'a')] < EDIT) => {
                    row[usize::from(b - b'a')]
                }
                None if near => CHEAP_EDIT,
                _ => continue,
            };
            let mut s = word.to_vec();
            s[i] = b;
            add(s, cost);
        }
        if i + 1 < n && word[i] != word[i + 1] {
            let mut s = word.to_vec();
            s.swap(i, i + 1);
            add(s, CHEAP_EDIT);
        }
        let doubled = (i > 0 && word[i - 1] == word[i]) || (i + 1 < n && word[i + 1] == word[i]);
        let mut s = word.to_vec();
        s.remove(i);
        add(s, if doubled { CHEAP_EDIT } else { EDIT });
    }
    for i in 0..=n {
        for b in b'a'..=b'z' {
            let doubled = (i > 0 && word[i - 1] == b) || (i < n && word[i] == b);
            let mut s = word.to_vec();
            s.insert(i, b);
            add(s, if doubled { CHEAP_EDIT } else { EDIT });
        }
    }
    let mut out: Vec<(Vec<u8>, u8)> = found.into_iter().collect();
    out.sort();
    out
}

impl Automaton for WeightedEdit {
    type State = EditState;

    fn start(&self) -> EditState {
        let mut cur = [INF; MAX_LEN + 1];
        cur[0] = 0;
        for i in 1..=self.len {
            cur[i] = cur[i - 1].saturating_add(self.delete_cost(i));
        }
        let mut ends = [INF; MAX_TAIL + 1];
        ends[0] = cur[self.len];
        EditState {
            prev: [INF; MAX_LEN + 1],
            cur,
            last: None,
            ends,
        }
    }

    fn is_match(&self, state: &EditState) -> bool {
        state.cur[self.len] <= self.max_cost || state.tail(self).is_some()
    }

    fn can_match(&self, state: &EditState) -> bool {
        state.cur[..=self.len].iter().any(|&c| c <= self.max_cost)
            || (self.tail && state.ends[..MAX_TAIL].iter().any(|&c| c <= self.max_cost))
    }

    fn accept(&self, state: &EditState, byte: u8) -> EditState {
        let b = byte.to_ascii_lowercase();
        let mut next = [INF; MAX_LEN + 1];
        next[0] = state.cur[0].saturating_add(Self::insert_cost(b, state.last));
        for i in 1..=self.len {
            let q = self.q(i - 1);
            let mut cost = state.cur[i].saturating_add(Self::insert_cost(b, state.last));
            cost = cost.min(next[i - 1].saturating_add(self.delete_cost(i)));
            cost = cost.min(state.cur[i - 1].saturating_add(self.substitute_cost(i - 1, q, b)));
            if i >= 2 && state.last == Some(q) && self.q(i - 2) == b && q != b {
                cost = cost.min(state.prev[i - 2].saturating_add(CHEAP_EDIT));
            }
            next[i] = cost;
        }
        let mut ends = [INF; MAX_TAIL + 1];
        ends[0] = next[self.len];
        ends[1..].copy_from_slice(&state.ends[..MAX_TAIL]);
        EditState {
            prev: state.cur,
            cur: next,
            last: Some(b),
            ends,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slip_cost(word: &str, spelling: &str) -> Option<u8> {
        slips(word.as_bytes()).into_iter().find(|(s, _)| s == spelling.as_bytes()).map(|(_, c)| c)
    }

    #[test]
    fn slips_cost_what_the_automaton_charges() {
        // A neighbouring key, two letters swapped, a doubled letter typed once or twice.
        assert_eq!(slip_cost("nkan", "njan"), Some(CHEAP_EDIT));
        assert_eq!(slip_cost("kaisw", "kaise"), Some(CHEAP_EDIT));
        assert_eq!(slip_cost("veertil", "veettil"), Some(CHEAP_EDIT));
        assert_eq!(slip_cost("mrea", "mera"), Some(CHEAP_EDIT));
        assert_eq!(slip_cost("vetil", "vettil"), Some(CHEAP_EDIT));
        assert_eq!(slip_cost("veettil", "vettil"), Some(CHEAP_EDIT));
        // A letter missed or extra.
        assert_eq!(slip_cost("dhanyavasd", "dhanyavad"), Some(EDIT));
        assert_eq!(slip_cost("kase", "kaise"), Some(EDIT));
        // Not the word itself, nor a far key, nor two slips.
        assert_eq!(slip_cost("nkan", "nkan"), None);
        assert_eq!(slip_cost("nkan", "npan"), None);
        assert_eq!(slip_cost("nkan", "njaan"), None);
        assert!(slips(b"Nkan").is_empty());
        assert!(slips(b"").is_empty());
        for word in ["nkan", "kaisw", "veertil"] {
            for (spelling, c) in slips(word.as_bytes()) {
                let spelling = String::from_utf8(spelling).unwrap();
                assert!(cost(word, &spelling) <= c, "{word} → {spelling}");
            }
        }
    }

    fn tap(letter: char, probability: f32, others: &[(char, f32)]) -> Tap {
        Tap { letter, probability, others: others.to_vec() }
    }

    /// The automaton's cost with `taps` (scale 1 / 1.6, `rank::TAP_SCALE`).
    fn tapped_cost(query: &str, key: &str, taps: &[Tap]) -> u8 {
        let aut = WeightedEdit::new(query, u8::MAX - 1)
            .unwrap()
            .with_taps(TapCosts::of(query, taps, 1.0 / 1.6));
        let state = key.bytes().fold(aut.start(), |s, b| aut.accept(&s, b));
        state.cost(&aut)
    }

    #[test]
    fn a_tap_near_a_key_makes_that_key_cheap_and_one_in_the_middle_dear() {
        let plain = |c: char| tap(c, 1.0, &[]);
        // `tge` with the g tapped near h: the is nearly free, far cheaper than the flat cost.
        let near = [plain('t'), tap('g', 0.55, &[('h', 0.45)]), plain('e')];
        let between = tapped_cost("tge", "the", &near);
        assert!((1..CHEAP_EDIT / 4).contains(&between), "{between}");
        // A clear lean to g: still under the flat cost.
        let leaning = [plain('t'), tap('g', 0.7, &[('h', 0.3)]), plain('e')];
        let leant = tapped_cost("tge", "the", &leaning);
        assert!(between < leant && leant < CHEAP_EDIT, "{leant}");
        // In the middle of g (no other key likely): h costs more than without taps.
        let middle = [plain('t'), tap('g', 0.97, &[]), plain('e')];
        let centre = tapped_cost("tge", "the", &middle);
        assert!(CHEAP_EDIT < centre && centre <= EDIT, "{centre}");
        // Without taps, the flat neighbouring-key cost; letters without a row keep it.
        assert_eq!(tapped_cost("tge", "the", &[]), CHEAP_EDIT);
        assert_eq!(tapped_cost("hwllo", "hello", &[tap('o', 0.6, &[('i', 0.4)])]), CHEAP_EDIT);
        // The typed word itself costs nothing, whatever the taps.
        assert_eq!(tapped_cost("tge", "tge", &near), 0);
        // Another letter the tap could have meant, though not a neighbour, is costed by it.
        let row = tap_row(b'g', 0.6, &[(b'p', 0.4)], 1.0 / 1.6);
        assert!(row[usize::from(b'p' - b'a')] < CHEAP_EDIT);
        assert!(row[usize::from(b'h' - b'a')] > CHEAP_EDIT);
        assert_eq!(row[usize::from(b'z' - b'a')], EDIT);
        assert_eq!(row[usize::from(b'g' - b'a')], 0);
    }

    #[test]
    fn taps_are_matched_to_the_word_from_its_end() {
        let near = |c: char| tap(c, 0.6, &[('x', 0.4)]);
        // More taps than letters: the last ones are the word's.
        let costs = TapCosts::of("ab", &[near('z'), near('a'), near('b')], 1.0);
        assert!(costs.row(0).is_some() && costs.row(1).is_some());
        // Fewer: the first letters have none.
        let costs = TapCosts::of("abc", &[near('C')], 1.0);
        assert!(costs.row(0).is_none() && costs.row(1).is_none() && costs.row(2).is_some());
        // A tap that isn't the word's letter there ends the match: the letters before it were
        // typed some other way (an edit, a paste).
        let costs = TapCosts::of("abc", &[near('a'), near('q'), near('c')], 1.0);
        assert!(costs.row(0).is_none() && costs.row(1).is_none() && costs.row(2).is_some());
        // An apostrophe matches any tap that is no letter, and has no row.
        let costs = TapCosts::of("don't", &[near('n'), tap('’', 1.0, &[]), near('t')], 1.0);
        assert!(costs.row(2).is_some() && costs.row(3).is_none() && costs.row(4).is_some());
        assert_eq!(costs.filtered(b"don't", u8::is_ascii_alphabetic).len(), 4);
        // Nothing to say: no rows at all.
        assert!(TapCosts::of("abc", &[], 1.0).is_empty());
        // A letter tapped in the middle of its key has a row: its neighbours cost more.
        let centre = TapCosts::of("abc", &[tap('c', 1.0, &[])], 1.0);
        assert_eq!(centre.row(2), Some(&tap_row(b'c', 1.0, &[], 1.0)));
        assert!(TapCosts::of("a'", &[tap('\'', 1.0, &[])], 1.0).is_empty());
        assert!(TapCosts::of("എന്റെ", &[near('a')], 1.0).is_empty());
    }

    #[test]
    fn tapped_slips_cost_what_the_tapped_automaton_charges() {
        let rows = |word: &str, taps: &[Tap]| {
            TapCosts::of(word, taps, 1.0 / 1.6).filtered(word.as_bytes(), u8::is_ascii_alphabetic)
        };
        let plain = |c: char| tap(c, 1.0, &[]);
        let slip = |word: &str, taps: &[Tap], spelling: &str| {
            slips_tapped(word.as_bytes(), &rows(word, taps))
                .into_iter()
                .find(|(s, _)| s == spelling.as_bytes())
                .map(|(_, c)| c)
        };
        let between = [plain('n'), tap('k', 0.55, &[('j', 0.45)]), plain('a'), plain('n')];
        let centre = [plain('n'), tap('k', 0.97, &[]), plain('a'), plain('n')];
        let near = slip("nkan", &between, "njan").unwrap();
        assert!(near < CHEAP_EDIT / 4, "{near}");
        assert!(slip("nkan", &centre, "njan").unwrap() > CHEAP_EDIT);
        // Swaps cost as without taps; a letter tapped in the middle of its key is a full edit
        // from its neighbours, and one with no tap the flat neighbouring cost.
        assert_eq!(slip("nkan", &between, "nakn"), Some(CHEAP_EDIT));
        assert!(slip("nkan", &between, "nkam").unwrap() > CHEAP_EDIT);
        assert_eq!(slip("nkan", &between[1..], "mkan"), Some(CHEAP_EDIT));
        assert_eq!(slips_tapped(b"nkan", &[]), slips(b"nkan"));
        for (word, taps) in [("nkan", &between), ("nkan", &centre)] {
            for (spelling, c) in slips_tapped(word.as_bytes(), &rows(word, taps)) {
                let spelling = String::from_utf8(spelling).unwrap();
                assert!(tapped_cost(word, &spelling, taps) <= c, "{word} → {spelling}");
            }
        }
    }

    /// Runs the automaton over a whole key and returns the final cost.
    fn cost(query: &str, key: &str) -> u8 {
        let aut = WeightedEdit::new(query, u8::MAX - 1).unwrap();
        let state = key.bytes().fold(aut.start(), |s, b| aut.accept(&s, b));
        state.cost(&aut)
    }

    #[test]
    fn exact_and_case_insensitive_matches_cost_nothing() {
        assert_eq!(cost("hello", "hello"), 0);
        assert_eq!(cost("Hello", "hello"), 0);
        assert_eq!(cost("london", "London"), 0);
    }

    #[test]
    fn transposition_is_half_an_edit() {
        assert_eq!(cost("teh", "the"), CHEAP_EDIT);
    }

    #[test]
    fn neighbouring_key_is_cheaper() {
        assert_eq!(cost("hwllo", "hello"), CHEAP_EDIT); // w is next to e
        assert_eq!(cost("hpllo", "hello"), EDIT); // p is not
    }

    #[test]
    fn missing_apostrophe_and_doubled_letters_are_cheap() {
        assert_eq!(cost("dont", "don't"), CHEAP_EDIT);
        assert_eq!(cost("helllo", "hello"), CHEAP_EDIT);
        assert_eq!(cost("helo", "hello"), CHEAP_EDIT);
        assert_eq!(cost("hllo", "hello"), EDIT);
    }

    #[test]
    fn vowel_for_vowel_is_cheap_only_when_asked() {
        let walk = |aut: &WeightedEdit, key: &str| {
            key.bytes()
                .fold(aut.start(), |s, b| aut.accept(&s, b))
                .cost(aut)
        };
        let plain = WeightedEdit::new("definately", 4 * EDIT).unwrap();
        assert_eq!(walk(&plain, "definitely"), EDIT);
        assert_eq!(
            walk(&plain.clone().with_cheap_vowels(), "definitely"),
            CHEAP_EDIT
        );
        // Only vowel for vowel: a consonant for a vowel is still a full edit.
        assert_eq!(walk(&plain.with_cheap_vowels(), "definitrly"), EDIT);
    }

    #[test]
    fn tail_mode_reads_a_near_prefix_plus_letters() {
        let aut = WeightedEdit::new("apostrap", 2 * EDIT)
            .unwrap()
            .with_cheap_vowels()
            .with_tail();
        let state = b"apostrophe"
            .iter()
            .fold(aut.start(), |s, &b| aut.accept(&s, b));
        assert!(aut.is_match(&state));
        assert_eq!(state.tail(&aut), Some((CHEAP_EDIT, 2)));
        // Three letters past the match is too many.
        let exact = WeightedEdit::new("apostrap", CHEAP_EDIT).unwrap().with_tail();
        let state = b"apostrapxyz"
            .iter()
            .fold(exact.start(), |s, &b| exact.accept(&s, b));
        assert_eq!(state.tail(&exact), None);
        let state = b"apostrapxy"
            .iter()
            .fold(exact.start(), |s, &b| exact.accept(&s, b));
        assert_eq!(state.tail(&exact), Some((0, 2)));
        // Without tail mode the key is just too far away.
        let plain = WeightedEdit::new("apostrap", 2 * EDIT)
            .unwrap()
            .with_cheap_vowels();
        let state = b"apostrophe"
            .iter()
            .fold(plain.start(), |s, &b| plain.accept(&s, b));
        assert!(!plain.is_match(&state));
    }

    #[test]
    fn rejects_unusable_queries() {
        assert!(WeightedEdit::new("", 4).is_none());
        assert!(WeightedEdit::new("എന്റെ", 4).is_none());
        assert!(WeightedEdit::new(&"a".repeat(MAX_LEN + 1), 4).is_none());
    }

    #[test]
    fn prunes_hopeless_branches() {
        let aut = WeightedEdit::new("cat", EDIT).unwrap();
        let s = b"xyz".iter().fold(aut.start(), |s, &b| aut.accept(&s, b));
        assert!(!aut.can_match(&s));
    }

    proptest::proptest! {
        #[test]
        fn a_word_matches_itself_for_free(a in "[a-z]{1,8}", b in "[a-z]{1,8}") {
            // Any pair of words can be walked without panicking.
            let _ = cost(&a, &b);
            proptest::prop_assert_eq!(cost(&a, &a), 0);
        }
    }
}
