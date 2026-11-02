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
/// A substitution between neighbouring keys, a missed apostrophe, or a doubled letter typed once or three times.
pub const CHEAP_EDIT: u8 = 1;

const INF: u8 = u8::MAX;

#[derive(Debug, Clone)]
pub struct WeightedEdit {
    query: [u8; MAX_LEN],
    len: usize,
    max_cost: u8,
    neighbours: [u32; 26],
}

/// One DP row per byte read, plus the row before it for transpositions.
#[derive(Debug, Clone, Copy)]
pub struct EditState {
    prev: [u8; MAX_LEN + 1],
    cur: [u8; MAX_LEN + 1],
    last: Option<u8>,
}

impl EditState {
    /// The cost of matching the whole query against the key read so far.
    pub fn cost(&self, automaton: &WeightedEdit) -> u8 {
        self.cur[automaton.len]
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
        })
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
        if near { CHEAP_EDIT } else { EDIT }
    }
}

impl Automaton for WeightedEdit {
    type State = EditState;

    fn start(&self) -> EditState {
        let mut cur = [INF; MAX_LEN + 1];
        cur[0] = 0;
        for i in 1..=self.len {
            cur[i] = cur[i - 1].saturating_add(self.delete_cost(i));
        }
        EditState {
            prev: [INF; MAX_LEN + 1],
            cur,
            last: None,
        }
    }

    fn is_match(&self, state: &EditState) -> bool {
        state.cur[self.len] <= self.max_cost
    }

    fn can_match(&self, state: &EditState) -> bool {
        state.cur[..=self.len].iter().any(|&c| c <= self.max_cost)
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
                cost = cost.min(state.prev[i - 2].saturating_add(EDIT));
            }
            next[i] = cost;
        }
        EditState {
            prev: state.cur,
            cur: next,
            last: Some(b),
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
    fn transposition_is_one_edit() {
        assert_eq!(cost("teh", "the"), EDIT);
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
