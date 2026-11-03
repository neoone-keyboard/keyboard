//! A weighted Damerau–Levenshtein automaton for walking the English FST.
//! Costs are in half-edits, so neighbouring-key slips and doubled letters are
//! cheaper than other typos. The state is fixed-size, so a walk allocates
//! nothing however many keys it visits.

use fst::Automaton;

use crate::keys::adjacency_table;

/// Longest typed word that is corrected; longer ones are left alone.
pub const MAX_LEN: usize = 24;

/// Cost of one ordinary edit. `MAX_COST = 2 * EDIT` means "within two edits".
pub const EDIT: u8 = 2;
/// A substitution between neighbouring keys, two letters swapped, a missed
/// apostrophe, a doubled letter typed once or three times, or (for long words,
/// `WeightedEdit::with_cheap_vowels`) one vowel for another. (Swaps cost a
/// full edit until Task 17: `pani` then ranked `land` above `pain`.)
pub const CHEAP_EDIT: u8 = 1;

/// Most letters a key may have after the part that matches the query, in tail mode
/// (`WeightedEdit::with_tail`): `apostrap` reaches `apostrophe` as `apostrop` + `he`.
pub const MAX_TAIL: usize = 2;

const INF: u8 = u8::MAX;

#[derive(Debug, Clone)]
pub struct WeightedEdit {
    query: [u8; MAX_LEN],
    len: usize,
    max_cost: u8,
    neighbours: [u32; 26],
    cheap_vowels: bool,
    tail: bool,
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
        })
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

    fn substitute_cost(&self, typed: u8, key: u8) -> u8 {
        if typed == key {
            return 0;
        }
        let near = typed.is_ascii_lowercase()
            && key.is_ascii_lowercase()
            && self.neighbours[usize::from(typed - b'a')] & (1 << (key - b'a')) != 0;
        let vowels = self.cheap_vowels && is_vowel(typed) && is_vowel(key);
        if near || vowels { CHEAP_EDIT } else { EDIT }
    }
}

fn is_vowel(b: u8) -> bool {
    matches!(b, b'a' | b'e' | b'i' | b'o' | b'u')
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
            cost = cost.min(state.cur[i - 1].saturating_add(self.substitute_cost(q, b)));
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
        let plain = WeightedEdit::new("definately", 8).unwrap();
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
        let aut = WeightedEdit::new("apostrap", 4)
            .unwrap()
            .with_cheap_vowels()
            .with_tail();
        let state = b"apostrophe"
            .iter()
            .fold(aut.start(), |s, &b| aut.accept(&s, b));
        assert!(aut.is_match(&state));
        assert_eq!(state.tail(&aut), Some((CHEAP_EDIT, 2)));
        // Three letters past the match is too many.
        let exact = WeightedEdit::new("apostrap", 1).unwrap().with_tail();
        let state = b"apostrapxyz"
            .iter()
            .fold(exact.start(), |s, &b| exact.accept(&s, b));
        assert_eq!(state.tail(&exact), None);
        let state = b"apostrapxy"
            .iter()
            .fold(exact.start(), |s, &b| exact.accept(&s, b));
        assert_eq!(state.tail(&exact), Some((0, 2)));
        // Without tail mode the key is just too far away.
        let plain = WeightedEdit::new("apostrap", 4)
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
        let aut = WeightedEdit::new("cat", 2).unwrap();
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
