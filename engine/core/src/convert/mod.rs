//! Phrase conversion (engine 9): a language typed as a reading and converted as a whole phrase,
//! Japanese's romaji → kana → kanji. A pack with `"conversion": "phrase"` in its manifest
//! carries three files for it:
//!
//! - `kana` (`kana.json`): the romaji table, read by the composer (`romaji`);
//! - `conversionLexicon`: reading → (surface, left id, right id, cost) (`dict`);
//! - `connection`: the cost of one part of speech followed by another (`connection`).
//!
//! The app keeps the typed letters as composing text (`Composer`, or `Converter::compose`
//! from the letters typed so far), shows the kana, and asks `Converter::convert` for the
//! whole phrase's candidates and its first segment's. Nothing here touches the word engine.

pub mod connection;
pub mod dict;
pub mod lattice;
pub mod romaji;

use std::path::{Path, PathBuf};

pub use connection::Connection;
pub use dict::Dictionary;
pub use lattice::{Lattice, Span};
pub use romaji::{Composer, KanaTable, to_hiragana, to_katakana};

/// The longest kana converted in one lattice; longer text is converted a piece at a time.
pub const MAX_KANA: usize = 64;

/// How much dearer than the best path a candidate may be. Mozc's word costs run to about
/// 20,000; a path that much dearer is never what was meant.
const BEAM: i32 = 30_000;

/// A phrase-conversion pack's files, as the engine opens them (`PackPaths::conversion`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversionPaths {
    /// `kana.json`, the `kana` role.
    pub kana: PathBuf,
    /// The `conversionLexicon` role.
    pub dictionary: PathBuf,
    /// The `connection` role.
    pub connection: PathBuf,
}

/// One segment of a conversion: a word and the particles or endings after it (今日は).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub reading: String,
    pub surface: String,
}

/// What `Converter::convert` returns.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Conversion {
    /// The best conversion, segment by segment.
    pub segments: Vec<Segment>,
    /// Candidates for the whole text, best first: the first is `segments` joined, and the
    /// text in hiragana and katakana are always among them.
    pub candidates: Vec<String>,
    /// Candidates for the first segment (`segments[0]`'s reading), best first, with its
    /// hiragana and katakana.
    pub first: Vec<String>,
}

/// A pack's conversion data, opened.
#[derive(Debug)]
pub struct Converter {
    table: KanaTable,
    dict: Dictionary,
    conn: Connection,
}

impl Converter {
    /// The converter of the files at these paths.
    pub fn open(kana: &Path, dict: &Path, connection: &Path) -> Result<Self, String> {
        Self::new(
            KanaTable::load(kana)?,
            Dictionary::open(dict)?,
            Connection::open(connection)?,
        )
    }

    pub fn new(table: KanaTable, dict: Dictionary, conn: Connection) -> Result<Self, String> {
        if dict.ids() != u32::from(conn.ids()) {
            return Err(format!(
                "the dictionary has {} ids, the connection matrix {}",
                dict.ids(),
                conn.ids()
            ));
        }
        Ok(Self { table, dict, conn })
    }

    pub fn table(&self) -> &KanaTable {
        &self.table
    }

    pub fn dictionary(&self) -> &Dictionary {
        &self.dict
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// The composing text of `typed` (the letters typed so far): its kana and the letters
    /// still waiting. `finish` writes what waits as well (before converting).
    pub fn compose(&self, typed: &str, finish: bool) -> (String, String) {
        self.table.compose(typed, finish)
    }

    /// Converts `kana`, giving up to `n` candidates for the whole text and for its first
    /// segment. Empty for empty text.
    pub fn convert(&self, kana: &str, n: usize) -> Conversion {
        if kana.is_empty() || n == 0 {
            return Conversion::default();
        }
        let pieces = pieces(kana);
        if pieces.len() > 1 {
            return self.convert_long(&pieces, n);
        }
        let lattice = Lattice::new(&self.dict, kana);
        let paths = lattice.best(&self.conn, Span::whole(kana.len()), n, BEAM);
        let Some(best) = paths.first() else {
            return Conversion::default();
        };
        let segments = self.segments(&lattice, &best.nodes);
        let mut candidates: Vec<String> = paths.iter().map(|p| p.surface.clone()).collect();
        add_kana(&mut candidates, kana, n);
        let first = self.first_candidates(&lattice, &best.nodes, &segments, n);
        Conversion {
            segments,
            candidates,
            first,
        }
    }

    /// Converts `kana` with its first segment fixed to its first `first_end` bytes, as when
    /// the user moves the segment boundary (a shorter or longer reading for the first
    /// segment): the first segment's candidates are for that reading, before the best
    /// conversion of the rest, which starts a segment of its own. None when `first_end` isn't
    /// a character boundary inside the text, or the text is longer than `MAX_KANA`.
    pub fn convert_with_first(&self, kana: &str, first_end: usize, n: usize) -> Option<Conversion> {
        let inside = first_end > 0 && first_end <= kana.len() && kana.is_char_boundary(first_end);
        if n == 0 || !inside || kana.chars().count() > MAX_KANA {
            return None;
        }
        let lattice = Lattice::new(&self.dict, kana);
        // The first segment's best alone gives the rest its left neighbour; the rest's best
        // gives the first segment its right one.
        let span = |start, end, left, right| Span {
            start,
            end,
            left,
            right,
        };
        let head = lattice.best(&self.conn, span(0, first_end, 0, 0), 1, BEAM);
        let left = head
            .first()
            .and_then(|p| p.nodes.last())
            .map_or(0, |&i| lattice.node(i).rid);
        let rest = lattice.best(&self.conn, span(first_end, kana.len(), left, 0), 1, BEAM);
        let right = rest
            .first()
            .and_then(|p| p.nodes.first())
            .map_or(0, |&i| lattice.node(i).lid);
        let mut first: Vec<String> = lattice
            .best(&self.conn, span(0, first_end, 0, right), n, BEAM)
            .into_iter()
            .map(|p| p.surface)
            .collect();
        add_kana(&mut first, &kana[..first_end], n);
        let mut segments = vec![Segment {
            reading: kana[..first_end].to_string(),
            surface: first[0].clone(),
        }];
        if let Some(rest) = rest.first() {
            let mut more = self.segments(&lattice, &rest.nodes);
            segments.append(&mut more);
        }
        let mut candidates = vec![
            segments
                .iter()
                .map(|s| s.surface.as_str())
                .collect::<String>(),
        ];
        add_kana(&mut candidates, kana, n);
        Some(Conversion {
            segments,
            candidates,
            first,
        })
    }

    /// Text over `MAX_KANA`: the best of each piece, one after the other.
    fn convert_long(&self, pieces: &[&str], n: usize) -> Conversion {
        let mut whole = Conversion::default();
        let mut best = String::new();
        for (i, piece) in pieces.iter().enumerate() {
            let c = self.convert(piece, n);
            best.push_str(c.candidates.first().map_or(*piece, String::as_str));
            if i == 0 {
                whole.first = c.first;
            }
            whole.segments.extend(c.segments);
        }
        whole.candidates.push(best);
        let kana: String = pieces.concat();
        add_kana(&mut whole.candidates, &kana, n.max(3));
        whole
    }

    /// The best path's nodes grouped into segments: a node whose part of speech joins the
    /// segment before it (a particle, an auxiliary, a suffix) goes with the one before.
    fn segments(&self, lattice: &Lattice, nodes: &[u32]) -> Vec<Segment> {
        let mut segments: Vec<Segment> = Vec::new();
        for &i in nodes {
            let joins = self.dict.joins_before(lattice.node(i).lid) && !segments.is_empty();
            let (reading, surface) = (lattice.reading(i), lattice.surface(i));
            match segments.last_mut() {
                Some(last) if joins => {
                    last.reading.push_str(reading);
                    last.surface.push_str(&surface);
                }
                _ => segments.push(Segment {
                    reading: reading.into(),
                    surface,
                }),
            }
        }
        segments
    }

    /// Candidates for the first segment, in its place: after the start of the text and
    /// before the best path's next word.
    fn first_candidates(
        &self,
        lattice: &Lattice,
        nodes: &[u32],
        segments: &[Segment],
        n: usize,
    ) -> Vec<String> {
        let Some(first) = segments.first() else {
            return Vec::new();
        };
        let end = first.reading.len();
        let right = nodes
            .iter()
            .map(|&i| lattice.node(i))
            .find(|node| node.start as usize == end)
            .map_or(0, |node| node.lid);
        let mut out: Vec<String> = lattice
            .best(
                &self.conn,
                Span {
                    start: 0,
                    end,
                    left: 0,
                    right,
                },
                n,
                BEAM,
            )
            .into_iter()
            .map(|p| p.surface)
            .collect();
        add_kana(&mut out, &first.reading, n);
        out
    }
}

/// `kana` cut into pieces of at most `MAX_KANA` characters, after a punctuation mark where
/// there is one.
fn pieces(kana: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = kana;
    while rest.chars().count() > MAX_KANA {
        let cut = rest
            .char_indices()
            .nth(MAX_KANA)
            .map_or(rest.len(), |(i, _)| i);
        let at = rest[..cut]
            .char_indices()
            .filter(|(_, c)| matches!(c, '、' | '。' | '！' | '？'))
            .map(|(i, c)| i + c.len_utf8())
            .next_back()
            .unwrap_or(cut);
        out.push(&rest[..at]);
        rest = &rest[at..];
    }
    out.push(rest);
    out
}

/// Adds `kana` in hiragana and katakana to `candidates` when missing, keeping at most `n`
/// others before them.
fn add_kana(candidates: &mut Vec<String>, kana: &str, n: usize) {
    let forms = [to_hiragana(kana), to_katakana(kana)];
    let missing: Vec<&String> = forms.iter().filter(|f| !candidates.contains(f)).collect();
    candidates.truncate(n.saturating_sub(missing.len()).max(1));
    for f in forms {
        if !candidates.contains(&f) {
            candidates.push(f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn converter() -> Converter {
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
            row("は", 2, 100, "は"),
            row("あめ", 1, 1000, "雨"),
            row("あめ", 1, 1200, "飴"),
            row("が", 2, 100, "が"),
            row("ふる", 3, 1000, "降る"),
        ];
        let d = Dictionary::from_bytes(dict::build(&rows, 5, 4, &[2]).unwrap()).unwrap();
        let mut costs = vec![500; 25];
        costs[5 + 2] = 0;
        costs[5 + 1] = 3000;
        costs[2] = 5000;
        costs[2 * 5 + 3] = 0;
        let c = Connection::from_bytes(connection::build(5, &costs).unwrap()).unwrap();
        let t = KanaTable::parse(include_str!("../../../../data/langs/ja/kana.json")).unwrap();
        Converter::new(t, d, c).unwrap()
    }

    #[test]
    fn converts_typed_romaji() {
        let c = converter();
        let (kana, pending) = c.compose("kyouhaamegafuru", true);
        assert_eq!(pending, "");
        let conv = c.convert(&kana, 5);
        assert_eq!(conv.candidates[0], "今日は雨が降る");
        let segments: Vec<(&str, &str)> = conv
            .segments
            .iter()
            .map(|s| (s.reading.as_str(), s.surface.as_str()))
            .collect();
        assert_eq!(
            segments,
            [("きょうは", "今日は"), ("あめが", "雨が"), ("ふる", "降る")]
        );
        assert_eq!(conv.first[0], "今日は");
        assert!(conv.first.contains(&"京は".to_string()), "{:?}", conv.first);
        assert!(conv.first.contains(&"きょうは".to_string()));
        assert!(conv.first.contains(&"キョウハ".to_string()));
        assert!(conv.candidates.contains(&"きょうはあめがふる".to_string()));
        assert!(conv.candidates.contains(&"キョウハアメガフル".to_string()));
        assert!(conv.candidates.len() <= 5, "{:?}", conv.candidates);
    }

    #[test]
    fn empty_text_has_nothing() {
        assert_eq!(converter().convert("", 5), Conversion::default());
        assert_eq!(converter().convert("あめ", 0), Conversion::default());
    }

    #[test]
    fn long_text_is_converted_in_pieces() {
        let c = converter();
        let long = "あめが、".repeat(30);
        let p = pieces(&long);
        assert!(p.len() > 1);
        assert!(
            p.iter()
                .all(|p| p.chars().count() <= MAX_KANA && p.ends_with('、'))
        );
        assert_eq!(p.concat(), long);
        let conv = c.convert(&long, 3);
        assert_eq!(conv.candidates[0], "雨が、".repeat(30));
        assert_eq!(conv.first[0], "雨が");
        let plain = "あ".repeat(150);
        assert_eq!(
            pieces(&plain)
                .iter()
                .map(|p| p.chars().count())
                .collect::<Vec<_>>(),
            [64, 64, 22]
        );
    }

    #[test]
    fn the_first_segment_can_be_shortened_or_lengthened() {
        let c = converter();
        let kana = "きょうはあめがふる";
        assert_eq!(c.convert(kana, 5).segments[0].reading, "きょうは");
        // Shorter: きょう alone, then the rest.
        let short = c.convert_with_first(kana, "きょう".len(), 5).unwrap();
        assert_eq!(short.segments[0].reading, "きょう");
        assert_eq!(short.first[0], "今日");
        assert!(short.first.contains(&"京".to_string()), "{:?}", short.first);
        assert_eq!(short.candidates[0], "今日は雨が降る");
        let readings: String = short.segments.iter().map(|s| s.reading.as_str()).collect();
        assert_eq!(readings, kana);
        // Longer: きょうはあめ as one segment.
        let long = c.convert_with_first(kana, "きょうはあめ".len(), 5).unwrap();
        assert_eq!(long.segments[0].reading, "きょうはあめ");
        assert_eq!(long.first[0], "今日は雨");
        // The whole text as the first segment.
        let all = c.convert_with_first(kana, kana.len(), 5).unwrap();
        assert_eq!(all.segments.len(), 1);
        assert_eq!(all.first[0], "今日は雨が降る");
        // Not a boundary inside the text.
        assert_eq!(c.convert_with_first(kana, 1, 5), None);
        assert_eq!(c.convert_with_first(kana, 0, 5), None);
        assert_eq!(c.convert_with_first(kana, kana.len() + 3, 5), None);
    }

    #[test]
    fn mismatched_files_are_refused() {
        let d = Dictionary::from_bytes(dict::build(&[], 5, 4, &[]).unwrap()).unwrap();
        let c = Connection::from_bytes(connection::build(4, &[0; 16]).unwrap()).unwrap();
        let t = KanaTable::parse(r#"{"format": 1, "rules": []}"#).unwrap();
        assert!(Converter::new(t, d, c).is_err());
    }
}
