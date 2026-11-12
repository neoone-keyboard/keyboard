//! The conversion lattice: every dictionary entry whose reading is found at each place in the
//! kana, plus one node per kana that no entry needs to cover (so there is always a path). The
//! cost of a path is its words' costs plus the connection cost of each pair of neighbours,
//! with the start and end of the text (id 0, Mozc's BOS/EOS) at both ends.
//!
//! `best` is a forward Viterbi pass (the cheapest cost from the start to each node) followed by
//! a backward A* search, whose estimate of the rest of a path is exact (the forward cost), so
//! paths come out cheapest first. Both work on a span of the text with given neighbours, so the
//! same code finds the whole input's candidates and one segment's.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};

use super::connection::Connection;
use super::dict::{Dictionary, Surface};

/// The word cost of a kana no entry covers: dearer than any dictionary word.
pub const UNKNOWN_COST: i32 = 20_000;

/// One word in the lattice: bytes `start..end` of the kana.
#[derive(Debug, Clone, Copy)]
pub struct Node {
    pub start: u32,
    pub end: u32,
    pub lid: u16,
    pub rid: u16,
    pub cost: i32,
    /// None for an uncovered kana (its surface is itself).
    pub surface: Option<Surface>,
}

/// Bytes `start..end` of the kana, between a word whose right id is `left` and one whose left
/// id is `right` (0, Mozc's BOS/EOS, for the start and end of the text).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub left: u16,
    pub right: u16,
}

impl Span {
    /// All of a text `len` bytes long.
    pub fn whole(len: usize) -> Self {
        Self {
            start: 0,
            end: len,
            left: 0,
            right: 0,
        }
    }
}

/// A path through a span, first word first.
#[derive(Debug, Clone)]
pub struct Path {
    pub nodes: Vec<u32>,
    pub cost: i32,
    pub surface: String,
}

pub struct Lattice<'d> {
    dict: &'d Dictionary,
    kana: String,
    nodes: Vec<Node>,
    /// Nodes starting and ending at each byte of the kana.
    starts: Vec<Vec<u32>>,
    ends: Vec<Vec<u32>>,
}

/// A reusable table by id, remembering which ids it set so it can be cleared cheaply.
struct ById {
    values: Vec<(i32, u32)>,
    touched: Vec<u16>,
}

impl ById {
    fn new(ids: usize) -> Self {
        Self {
            values: vec![(i32::MAX, u32::MAX); ids],
            touched: Vec::new(),
        }
    }

    fn offer(&mut self, id: u16, value: i32, from: u32) {
        let slot = &mut self.values[usize::from(id)];
        if slot.0 == i32::MAX {
            self.touched.push(id);
        }
        if value < slot.0 {
            *slot = (value, from);
        }
    }

    fn get(&self, id: u16) -> Option<(i32, u32)> {
        let v = self.values[usize::from(id)];
        (v.0 != i32::MAX).then_some(v)
    }

    fn clear(&mut self) {
        for id in self.touched.drain(..) {
            self.values[usize::from(id)] = (i32::MAX, u32::MAX);
        }
    }
}

impl<'d> Lattice<'d> {
    /// The lattice of `kana` over `dict`.
    pub fn new(dict: &'d Dictionary, kana: &str) -> Self {
        let len = kana.len();
        let mut lattice = Self {
            dict,
            kana: kana.to_string(),
            nodes: Vec::new(),
            starts: vec![Vec::new(); len + 1],
            ends: vec![Vec::new(); len + 1],
        };
        let unknown = dict.unknown_id();
        for (start, c) in kana.char_indices() {
            for (found, offset) in dict.prefixes(&kana[start..]) {
                let end = (start + found) as u32;
                dict.each_entry(offset, |e| {
                    lattice.push(Node {
                        start: start as u32,
                        end,
                        lid: e.lid,
                        rid: e.rid,
                        cost: i32::from(e.cost),
                        surface: Some(e.surface),
                    });
                });
            }
            lattice.push(Node {
                start: start as u32,
                end: (start + c.len_utf8()) as u32,
                lid: unknown,
                rid: unknown,
                cost: UNKNOWN_COST,
                surface: None,
            });
        }
        lattice
    }

    fn push(&mut self, node: Node) {
        let i = self.nodes.len() as u32;
        self.starts[node.start as usize].push(i);
        self.ends[node.end as usize].push(i);
        self.nodes.push(node);
    }

    pub fn kana(&self) -> &str {
        &self.kana
    }

    pub fn node(&self, i: u32) -> &Node {
        &self.nodes[i as usize]
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The reading of node `i`.
    pub fn reading(&self, i: u32) -> &str {
        let n = self.node(i);
        &self.kana[n.start as usize..n.end as usize]
    }

    /// The surface of node `i`.
    pub fn surface(&self, i: u32) -> String {
        let reading = self.reading(i);
        match self.node(i).surface {
            Some(s) => self.dict.surface(reading, s),
            None => reading.to_string(),
        }
    }

    fn inside(&self, i: u32, start: usize, end: usize) -> bool {
        let n = self.node(i);
        n.start as usize >= start && n.end as usize <= end
    }

    /// The cheapest cost from `start` (after a word whose right id is `left`) to the end of
    /// each node inside `start..end`, its own cost included; i32::MAX for the rest.
    fn forward(&self, conn: &Connection, start: usize, end: usize, left: u16) -> Vec<i32> {
        let ids = usize::from(conn.ids());
        let mut alpha = vec![i32::MAX; self.nodes.len()];
        let mut by_rid = ById::new(ids);
        let mut by_lid = ById::new(ids);
        for p in start..end {
            if !self.kana.is_char_boundary(p) || self.starts[p].is_empty() {
                continue;
            }
            if p == start {
                by_rid.offer(left, 0, u32::MAX);
            } else {
                for &m in &self.ends[p] {
                    if alpha[m as usize] != i32::MAX && self.inside(m, start, end) {
                        by_rid.offer(self.node(m).rid, alpha[m as usize], m);
                    }
                }
            }
            if by_rid.touched.is_empty() {
                continue;
            }
            for &n in &self.starts[p] {
                let node = self.node(n);
                if node.end as usize > end {
                    continue;
                }
                let into = match by_lid.get(node.lid) {
                    Some((best, _)) => best,
                    None => {
                        let best = by_rid
                            .touched
                            .iter()
                            .map(|&rid| {
                                by_rid.values[usize::from(rid)].0 + conn.cost(rid, node.lid)
                            })
                            .min()
                            .unwrap_or(i32::MAX);
                        by_lid.offer(node.lid, best, 0);
                        best
                    }
                };
                alpha[n as usize] = into.saturating_add(node.cost);
            }
            by_rid.clear();
            by_lid.clear();
        }
        alpha
    }

    /// Up to `n` paths over `span` with distinct surfaces, cheapest first. Paths dearer than
    /// the cheapest by more than `beam` are not looked for.
    pub fn best(&self, conn: &Connection, span: Span, n: usize, beam: i32) -> Vec<Path> {
        let Span {
            start,
            end,
            left,
            right,
        } = span;
        let mut found: Vec<Path> = Vec::new();
        if n == 0 || start >= end || end > self.kana.len() {
            return found;
        }
        let alpha = self.forward(conn, start, end, left);
        // A state: a node, the cost after it (to `end` and the right neighbour), the state
        // after it.
        let mut states: Vec<(u32, i32, u32)> = Vec::new();
        let mut heap: BinaryHeap<Reverse<(i32, u32)>> = BinaryHeap::new();
        for &m in &self.ends[end] {
            let a = alpha[m as usize];
            if a == i32::MAX || !self.inside(m, start, end) {
                continue;
            }
            let g = conn.cost(self.node(m).rid, right);
            states.push((m, g, u32::MAX));
            heap.push(Reverse((a.saturating_add(g), states.len() as u32 - 1)));
        }
        let Some(&Reverse((cheapest, _))) = heap.peek() else {
            return found;
        };
        let limit = cheapest.saturating_add(beam);
        let mut seen = HashSet::new();
        let mut pops = 0;
        while let Some(Reverse((f, s))) = heap.pop() {
            pops += 1;
            if pops > 50_000 || f > limit {
                break;
            }
            let (m, g, _) = states[s as usize];
            let node = *self.node(m);
            if node.start as usize == start {
                let mut nodes = Vec::new();
                let mut at = s;
                while at != u32::MAX {
                    nodes.push(states[at as usize].0);
                    at = states[at as usize].2;
                }
                let surface: String = nodes.iter().map(|&i| self.surface(i)).collect();
                if seen.insert(surface.clone()) {
                    found.push(Path {
                        nodes,
                        cost: f,
                        surface,
                    });
                    if found.len() >= n {
                        break;
                    }
                }
                continue;
            }
            for &p in &self.ends[node.start as usize] {
                let a = alpha[p as usize];
                if a == i32::MAX || !self.inside(p, start, end) {
                    continue;
                }
                let g2 = g
                    .saturating_add(node.cost)
                    .saturating_add(conn.cost(self.node(p).rid, node.lid));
                let f2 = g2.saturating_add(a);
                if f2 > limit {
                    continue;
                }
                states.push((p, g2, s));
                heap.push(Reverse((f2, states.len() as u32 - 1)));
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::super::{connection, dict};
    use super::*;

    /// Ids: 0 BOS/EOS, 1 noun, 2 particle, 3 verb, 4 unknown.
    fn setup() -> (Dictionary, Connection) {
        let row = |reading: &str, id: u16, cost: u16, surface: &str| dict::Row {
            reading: reading.into(),
            lid: id,
            rid: id,
            cost,
            surface: surface.into(),
        };
        let rows = [
            row("きょう", 1, 1000, "今日"),
            row("きょう", 1, 1500, "京"),
            row("きょう", 1, 3000, "強"),
            row("きょ", 1, 5000, "居"),
            row("う", 1, 5000, "鵜"),
            row("は", 2, 100, "は"),
            row("は", 1, 3000, "葉"),
            row("あめ", 1, 1000, "雨"),
            row("あめ", 1, 1200, "飴"),
            row("が", 2, 100, "が"),
            row("ふる", 3, 1000, "降る"),
            row("ふる", 3, 1100, "振る"),
        ];
        let d = Dictionary::from_bytes(dict::build(&rows, 5, 4, &[2]).unwrap()).unwrap();
        // Particle after noun is cheap, noun after noun dear, particle at the start dear.
        let mut costs = vec![500; 25];
        let set = |c: &mut Vec<i32>, rid: usize, lid: usize, v: i32| c[rid * 5 + lid] = v;
        set(&mut costs, 1, 2, 0);
        set(&mut costs, 1, 1, 3000);
        set(&mut costs, 0, 2, 5000);
        set(&mut costs, 2, 3, 0);
        set(&mut costs, 3, 0, 0);
        let c = Connection::from_bytes(connection::build(5, &costs).unwrap()).unwrap();
        (d, c)
    }

    #[test]
    fn the_cheapest_path_is_first() {
        let (d, c) = setup();
        let kana = "きょうはあめがふる";
        let l = Lattice::new(&d, kana);
        let paths = l.best(&c, Span::whole(kana.len()), 5, 100_000);
        assert_eq!(paths[0].surface, "今日は雨が降る");
        let surfaces: Vec<&str> = paths.iter().map(|p| p.surface.as_str()).collect();
        assert!(surfaces.contains(&"京は雨が降る"), "{surfaces:?}");
        assert!(surfaces.contains(&"今日は飴が降る"), "{surfaces:?}");
        for pair in paths.windows(2) {
            assert!(pair[0].cost <= pair[1].cost);
        }
        let mut unique: Vec<&str> = surfaces.clone();
        unique.dedup();
        assert_eq!(unique.len(), surfaces.len());
    }

    #[test]
    fn the_path_cost_is_its_words_and_connections() {
        let (d, c) = setup();
        let kana = "あめがふる";
        let l = Lattice::new(&d, kana);
        let best = &l.best(&c, Span::whole(kana.len()), 1, 0)[0];
        // BOS→雨 500, 雨 1000, 雨→が 0, が 100, が→降る 0, 降る 1000, 降る→EOS 0; costs are
        // rounded to the matrix's resolution (20 here, 5000 / 255 + 1).
        assert_eq!(best.surface, "雨が降る");
        assert!((best.cost - 2600).abs() <= 30, "{}", best.cost);
        let readings: Vec<&str> = best.nodes.iter().map(|&i| l.reading(i)).collect();
        assert_eq!(readings, ["あめ", "が", "ふる"]);
    }

    #[test]
    fn uncovered_kana_are_themselves() {
        let (d, c) = setup();
        let kana = "ぬあめ";
        let l = Lattice::new(&d, kana);
        let best = &l.best(&c, Span::whole(kana.len()), 1, 0)[0];
        assert_eq!(best.surface, "ぬ雨");
        assert!(best.cost >= UNKNOWN_COST);
    }

    #[test]
    fn a_span_with_its_neighbours() {
        let (d, c) = setup();
        let kana = "きょうはあめ";
        let l = Lattice::new(&d, kana);
        // The first segment, きょうは, before a noun.
        let end = "きょうは".len();
        let paths = l.best(
            &c,
            Span {
                start: 0,
                end,
                left: 0,
                right: 1,
            },
            10,
            100_000,
        );
        let surfaces: Vec<&str> = paths.iter().map(|p| p.surface.as_str()).collect();
        assert_eq!(surfaces[0], "今日は");
        assert!(surfaces.contains(&"強は"), "{surfaces:?}");
        assert!(surfaces.iter().all(|s| s.chars().count() <= 4));
        // A span in the middle, after a particle.
        let paths = l.best(
            &c,
            Span {
                start: end,
                end: kana.len(),
                left: 2,
                right: 0,
            },
            2,
            100_000,
        );
        assert_eq!(paths[0].surface, "雨");
        assert_eq!(paths[1].surface, "飴");
    }

    #[test]
    fn nothing_for_an_empty_span() {
        let (d, c) = setup();
        let l = Lattice::new(&d, "あめ");
        assert!(l.best(&c, Span::whole(0), 5, 0).is_empty());
        assert!(l.best(&c, Span::whole(6), 0, 0).is_empty());
        assert!(l.best(&c, Span::whole(99), 5, 0).is_empty());
    }
}
