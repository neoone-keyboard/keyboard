//! The user's own words of a pack: native-script words picked from the bar that the pack's
//! lexicon doesn't hold (`UserStore::pick`), found from Latin letters the way the pack's
//! romanised lexicon finds its words (`manglish::lookup`).
//!
//! A word is indexed by the skeleton the pack builder would key it by
//! (`Romanisation::word_skeleton`, as in `manglish::key`), so every typed form that finds a
//! lexicon word finds an own word too, and by the skeleton of each run of letters the user
//! typed when picking it (`raubdar` for रौबदार, which the tables walk as `raubadara`), so loose
//! spellings of those letters find it as well. A typed skeleton that begins a key finds the
//! word as a completion (`raub`), like `Lexicon::top_completions` under a skeleton prefix.
//!
//! The index lives in memory only: the store's log is the record. `Engine` builds this from
//! the store, puts each newly picked word in (`put`), and rebuilds it when the picked words
//! change any other way (`UserStore::picked_generation`). It is bounded by the store
//! (`user::CAPACITY` words, `user::MAX_TYPED_FORMS` runs of letters each).

use crate::manglish::{self, Match};
use crate::rank;
use crate::romanisation::Romanisation;

/// Words kept before the finer re-ranking by edit distance, as in `manglish::lookup`.
const SHORTLIST: usize = 16;

/// One pack's own words.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct OwnWords {
    /// The words, sorted by bytes: the transliterator's beam asks whether a text is one or
    /// begins one (`contains`, `has_prefix`).
    words: Vec<String>,
    /// The Latin letters each word was picked for, by its place in `words`.
    typed: Vec<Vec<String>>,
    /// (skeleton, place in `words`), sorted: a word's own skeleton and its typed letters'.
    keys: Vec<(String, u32)>,
}

impl OwnWords {
    /// The index of `picked` (each word with the letters it was picked for) under `r`. A word
    /// the pack builder could not key (`manglish::key`: no skeleton, or a character outside
    /// the script) is left out, as it would be from the pack.
    pub fn build<'a>(
        r: &Romanisation,
        picked: impl Iterator<Item = (&'a str, &'a [String])>,
    ) -> Self {
        // Each word with its own skeletons, walked once.
        let mut all: Vec<(&str, &[String], Vec<String>)> = picked
            .filter_map(|(word, typed)| Some((word, typed, word_skeletons(r, word)?)))
            .collect();
        all.sort_by_key(|(word, _, _)| *word);
        all.dedup_by_key(|(word, _, _)| *word);
        let mut keys = Vec::with_capacity(all.len() * 2);
        let mut words = Vec::with_capacity(all.len());
        let mut letters = Vec::with_capacity(all.len());
        for (place, (word, typed, skeleton)) in all.into_iter().enumerate() {
            keys.extend(skeletons(r, skeleton, typed).map(|s| (s, place as u32)));
            words.push(word.to_string());
            letters.push(typed.to_vec());
        }
        keys.sort();
        keys.dedup();
        Self {
            words,
            typed: letters,
            keys,
        }
    }

    /// Puts `word` in with the letters it was picked for, in place of what it had: the index
    /// is then as `build` would make it with the word so. False, and nothing changes, for a
    /// word the pack builder could not key.
    pub fn put(&mut self, r: &Romanisation, word: &str, typed: &[String]) -> bool {
        let Some(skeleton) = word_skeletons(r, word) else {
            return false;
        };
        let place = match self.words.binary_search_by(|w| w.as_str().cmp(word)) {
            Ok(place) => {
                self.keys.retain(|(_, p)| *p as usize != place);
                self.typed[place] = typed.to_vec();
                place
            }
            Err(place) => {
                for (_, p) in &mut self.keys {
                    if *p as usize >= place {
                        *p += 1;
                    }
                }
                self.words.insert(place, word.to_string());
                self.typed.insert(place, typed.to_vec());
                place
            }
        };
        for key in skeletons(r, skeleton, typed).map(|s| (s, place as u32)) {
            if let Err(at) = self.keys.binary_search(&key) {
                self.keys.insert(at, key);
            }
        }
        true
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    pub fn len(&self) -> usize {
        self.words.len()
    }

    /// The words, sorted by bytes.
    pub fn words(&self) -> &[String] {
        &self.words
    }

    pub fn contains(&self, word: &str) -> bool {
        contains(&self.words, word)
    }

    /// The own words for a typed Latin word, as `manglish::lookup` gives a lexicon's: those
    /// with exactly its skeleton, then longer ones, each scored `rank::OWN_WORD_LOG_PROB`.
    /// `uses`: how often a word was used, which orders the words when more match than are
    /// read closely (`SHORTLIST`). The fit is the closest of the word's romanisation and the
    /// letters it was picked for.
    pub fn lookup(&self, r: &Romanisation, typed: &str, uses: &dyn Fn(&str) -> u32) -> Vec<Match> {
        if self.words.is_empty() {
            return Vec::new();
        }
        let skeleton = r.skeleton(typed);
        if skeleton.is_empty() {
            return Vec::new();
        }
        let start = self.keys.partition_point(|(k, _)| k.as_str() < skeleton.as_str());
        // (place, skeleton letters past the typed ones), the fewest for each word.
        let mut found: Vec<(u32, usize)> = self.keys[start..]
            .iter()
            .take_while(|(k, _)| k.starts_with(&skeleton))
            .map(|(k, place)| (*place, k.len() - skeleton.len()))
            .collect();
        found.sort();
        found.dedup_by_key(|(place, _)| *place);
        if found.len() > SHORTLIST {
            // Exact skeletons first, then the most used, then the nearest to finished.
            let mut ranked: Vec<(bool, u32, usize, u32)> = found
                .iter()
                .map(|&(place, missing)| {
                    let used = uses(&self.words[place as usize]);
                    (missing > 0, u32::MAX - used, missing, place)
                })
                .collect();
            ranked.sort();
            ranked.truncate(SHORTLIST);
            found = ranked.into_iter().map(|(_, _, missing, place)| (place, missing)).collect();
        }
        found
            .into_iter()
            .map(|(place, missing)| {
                let word = &self.words[place as usize];
                let prefix = missing > 0;
                let (mut spelled, mut distance) = manglish::fit(r, typed, word, prefix);
                for letters in &self.typed[place as usize] {
                    let (s, d) = manglish::fit_latin(r, typed, letters, prefix);
                    spelled |= s;
                    distance = distance.min(d);
                }
                Match {
                    word: word.clone(),
                    log_prob: rank::OWN_WORD_LOG_PROB,
                    missing,
                    distance: if spelled { 0 } else { distance },
                    spelled,
                    other_reading: false,
                }
            })
            .collect()
    }
}

/// The skeletons the pack builder keys `word` by (`manglish::keys`: one, or two for a word
/// with a shorter spelling); None for a word it could not key: one with no skeleton, or with
/// a character outside the script.
fn word_skeletons(r: &Romanisation, word: &str) -> Option<Vec<String>> {
    manglish::pack(word, manglish::packing(r)?)?;
    Some(r.word_skeletons(word)).filter(|skeletons| !skeletons.is_empty())
}

/// The skeletons a word is indexed by: its own, and those of the letters it was picked for.
fn skeletons<'a>(
    r: &'a Romanisation,
    own: Vec<String>,
    typed: &'a [String],
) -> impl Iterator<Item = String> + 'a {
    let typed = typed.iter().map(|letters| r.skeleton(letters));
    own.into_iter().chain(typed.filter(|skeleton| !skeleton.is_empty()))
}

/// Whether `word` is one of `words`, which are sorted by bytes.
pub fn contains(words: &[String], word: &str) -> bool {
    words.binary_search_by(|w| w.as_str().cmp(word)).is_ok()
}

/// Whether one of `words`, which are sorted by bytes, starts with `prefix`.
pub fn has_prefix(words: &[String], prefix: &str) -> bool {
    let at = words.partition_point(|w| w.as_str() < prefix);
    words.get(at).is_some_and(|w| w.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexicon::{Lexicon, build};
    use crate::romanisation::ml;

    fn own(words: &[(&str, &[&str])]) -> OwnWords {
        let owned: Vec<(String, Vec<String>)> = words
            .iter()
            .map(|(w, typed)| (w.to_string(), typed.iter().map(|t| t.to_string()).collect()))
            .collect();
        OwnWords::build(ml(), owned.iter().map(|(w, t)| (w.as_str(), t.as_slice())))
    }

    fn no_uses(_: &str) -> u32 {
        0
    }

    #[test]
    fn finds_a_word_from_its_romanisation_loose_spellings_and_a_prefix() {
        let index = own(&[("മലയാളം", &[])]);
        for typed in ["malayaalam", "malayalam", "malayalaam", "Malayalam"] {
            let found = index.lookup(ml(), typed, &no_uses);
            assert_eq!(found.len(), 1, "{typed}");
            assert_eq!((found[0].word.as_str(), found[0].missing), ("മലയാളം", 0), "{typed}");
            assert_eq!(found[0].log_prob, rank::OWN_WORD_LOG_PROB);
        }
        assert!(index.lookup(ml(), "malayalam", &no_uses)[0].spelled);
        let partial = index.lookup(ml(), "malay", &no_uses);
        assert_eq!(partial.len(), 1);
        assert!(partial[0].missing > 0 && !partial[0].spelled);
        assert_eq!(partial[0].distance, 0, "malay is a prefix of malayaalam");
        assert!(index.lookup(ml(), "kerala", &no_uses).is_empty());
        assert!(index.lookup(ml(), "", &no_uses).is_empty());
        assert!(index.lookup(ml(), "123", &no_uses).is_empty());
    }

    /// A word with a shorter spelling (Hindi's `optionalFinals`) is indexed by both skeletons,
    /// as the pack's lexicon keys it.
    #[test]
    fn finds_a_word_by_its_shorter_spelling() {
        use crate::romanisation::hi;
        let picked = [("रहूँगीं".to_string(), Vec::<String>::new())];
        let mut index = OwnWords::build(hi(), picked.iter().map(|(w, t)| (w.as_str(), t.as_slice())));
        for typed in ["rahungin", "rahungi"] {
            let found = index.lookup(hi(), typed, &no_uses);
            assert_eq!(found.len(), 1, "{typed}");
            assert_eq!((found[0].missing, found[0].spelled), (0, true), "{typed}");
        }
        assert!(index.put(hi(), "जाऊँ", &[]));
        let found = index.lookup(hi(), "jau", &no_uses);
        assert_eq!((found[0].word.as_str(), found[0].missing, found[0].spelled), ("जाऊँ", 0, true));
    }

    #[test]
    fn finds_a_word_as_the_same_word_in_a_lexicon_is_found() {
        let words = ["എന്റെ", "എന്ത്", "അവൻ", "മലയാളം", "നാളെ", "വീട്"];
        let vocab: Vec<(String, u64)> = words.iter().map(|w| (w.to_string(), 1)).collect();
        let lexicon = Lexicon::from_bytes(build(manglish::entries(ml(), &vocab)).unwrap()).unwrap();
        let listed: Vec<(&str, &[&str])> = words.iter().map(|w| (*w, &[][..])).collect();
        let index = own(&listed);
        for typed in ["ente", "ende", "entey", "enth", "avan", "mala", "malayalam", "nale", "veedu", "vi"] {
            let fits = |found: Vec<Match>| {
                let mut fits: Vec<(String, usize, usize, bool)> = found
                    .into_iter()
                    .map(|m| (m.word, m.missing, m.distance, m.spelled))
                    .collect();
                fits.sort();
                fits
            };
            assert_eq!(
                fits(index.lookup(ml(), typed, &no_uses)),
                fits(manglish::lookup(ml(), &lexicon, typed)),
                "{typed}"
            );
        }
    }

    #[test]
    fn the_letters_a_word_was_picked_for_find_it_too() {
        // The tables walk തൃശ്ശൂർ as thrushshoor; people type trichur.
        let plain = own(&[("തൃശ്ശൂർ", &[])]);
        assert!(plain.lookup(ml(), "trichur", &no_uses).is_empty());
        let index = own(&[("തൃശ്ശൂർ", &["trichur"])]);
        let exact = index.lookup(ml(), "trichur", &no_uses);
        assert_eq!(exact.len(), 1);
        assert!(exact[0].spelled && exact[0].missing == 0 && exact[0].distance == 0);
        let loose = index.lookup(ml(), "trichoor", &no_uses);
        assert_eq!(loose.len(), 1, "another spelling of the letters picked for");
        assert!(loose[0].spelled, "up to vowel length: {loose:?}");
        let looser = index.lookup(ml(), "trichhur", &no_uses);
        assert_eq!((looser[0].missing, looser[0].spelled, looser[0].distance), (0, false, 1));
        let partial = index.lookup(ml(), "trichu", &no_uses);
        assert_eq!(partial.len(), 1);
        assert!(partial[0].missing > 0);
        assert_eq!(partial[0].distance, 0, "trichu begins trichur");
    }

    #[test]
    fn words_the_pack_could_not_key_are_left_out() {
        let index = own(&[("abc", &["abc"]), ("\u{0D4D}", &[]), ("क्या", &["kya"]), ("അവൻ", &[])]);
        assert_eq!(index.words(), ["അവൻ"]);
        assert!(index.lookup(ml(), "kya", &no_uses).is_empty());
    }

    #[test]
    fn the_most_used_words_are_read_when_many_match() {
        // 200 words under the skeleton "ent…", as in `manglish`'s test.
        let many: Vec<String> = (0..200)
            .map(|i| {
                let tail: String = format!("{i:03}")
                    .chars()
                    .map(|d| char::from_u32(0x0D15 + d.to_digit(10).unwrap()).unwrap())
                    .collect();
                format!("എന്റ{tail}")
            })
            .collect();
        let mut listed: Vec<(&str, &[&str])> = many.iter().map(|w| (w.as_str(), &[][..])).collect();
        listed.push(("എന്റെ", &[]));
        let index = own(&listed);
        assert_eq!(index.len(), 201);
        let used = many[137].clone();
        let uses = |w: &str| u32::from(w == used) * 5;
        let found = index.lookup(ml(), "ente", &uses);
        assert_eq!(found.len(), SHORTLIST);
        assert_eq!(found[0].word, "എന്റെ", "the exact skeleton comes first");
        assert_eq!(found[1].word, used);
    }

    #[test]
    fn a_word_put_in_later_leaves_the_index_as_if_built_with_it() {
        let all: &[(&str, &[&str])] = &[
            ("എന്റെ", &["ente"]),
            ("മലയാളം", &[]),
            ("അവൻ", &["avan", "awan"]),
            ("തൃശ്ശൂർ", &["trichur"]),
        ];
        let forms = |typed: &[&str]| typed.iter().map(|t| t.to_string()).collect::<Vec<_>>();
        // Each word left out and put in again, first, last and between.
        for (word, typed) in all {
            let rest: Vec<(&str, &[&str])> = all.iter().filter(|(w, _)| w != word).copied().collect();
            let mut index = own(&rest);
            assert!(index.put(ml(), word, &forms(typed)));
            assert_eq!(index, own(all), "{word}");
        }
        // A word already there takes its new letters and loses the old.
        let mut index = own(all);
        assert!(index.put(ml(), "അവൻ", &forms(&["avn"])));
        let mut changed = all.to_vec();
        changed[2] = ("അവൻ", &["avn"]);
        assert_eq!(index, own(&changed));
        // One the pack could not key is refused, and nothing changes.
        assert!(!index.put(ml(), "क्या", &forms(&["kya"])));
        assert_eq!(index, own(&changed));
        // Into an empty index.
        let mut index = OwnWords::default();
        assert!(index.put(ml(), "അവൻ", &[]));
        assert_eq!(index, own(&[("അവൻ", &[])]));
    }

    #[test]
    fn sorted_words_answer_the_beam() {
        let index = own(&[("എന്റെ", &[]), ("മലയാളം", &[]), ("അവൻ", &[])]);
        assert!(index.contains("മലയാളം") && !index.contains("മല"));
        assert!(has_prefix(index.words(), "മല") && has_prefix(index.words(), "മലയാളം"));
        assert!(!has_prefix(index.words(), "മലയാളംഎ") && !has_prefix(index.words(), "ക"));
        assert!(has_prefix(index.words(), ""));
        assert!(!has_prefix(&[], "") && !contains(&[], "അവൻ"));
    }
}
