//! The LM's view of the text before the cursor (design §8): a context cache kept in step with
//! the committed words, the next-token distribution after them, and the language posterior.
//! Everything here is optional work: `sync` stops when told to and resumes on the next call,
//! and until it has caught up the engine ignores the LM.

use crate::nn::lm::MAX_POSITION;
use crate::nn::tokenizer::{BOS, WORD_START, flag, lm_normalize};
use crate::nn::{Cache, Lm, LmScratch};
use crate::rank;

/// The committed words before the cursor, `lm_normalize`d, the last `rank::LM_WORDS` of them;
/// a leading "" stands for bos when they are the whole text.
pub fn lm_words(before: &str) -> Vec<String> {
    words_of(&lm_normalize(before))
}

/// `lm_words` for `lm`: normalised by its pack's rules (`Tokenizer::normal`).
pub fn lm_words_of(lm: &Lm, before: &str) -> Vec<String> {
    words_of(&lm.tokenizer().normal(before))
}

fn words_of(normal: &str) -> Vec<String> {
    let all: Vec<&str> = normal.split(' ').filter(|w| !w.is_empty()).collect();
    let mut out = Vec::with_capacity(all.len().min(rank::LM_WORDS) + 1);
    if all.len() < rank::LM_WORDS {
        out.push(String::new());
    }
    out.extend(
        all[all.len().saturating_sub(rank::LM_WORDS)..]
            .iter()
            .map(|w| w.to_string()),
    );
    if out.len() > rank::LM_WORDS {
        out.remove(0);
    }
    out
}

/// Log probabilities that the next word is English, the pack's language (Malayalam for the LM
/// of design §6) in any script, and in its native script.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Lang {
    pub log_en: f32,
    pub log_ml: f32,
    pub log_script: f32,
}

/// Fewest cached words that a text cut at the front must end with to keep them
/// (`LmContext::slid`).
const SLID_RUN: usize = 3;

/// Cached words `at..at + len` are the new words `from..from + len`.
#[derive(Debug, Clone, Copy)]
struct Run {
    at: usize,
    from: usize,
    len: usize,
}

/// The context cache and what is read from it once per context.
#[derive(Debug, Default)]
pub struct LmContext {
    cache: Option<Cache>,
    /// The cached words, with their token counts, oldest first ("" is bos).
    words: Vec<(String, usize)>,
    /// The word just before the cached ones, if the window dropped it or a fill began after it.
    dropped: Option<String>,
    next: Vec<f32>,
    lang: Lang,
    ready: bool,
    fed: usize,
    scratch: LmScratch,
}

/// The tokens a context word is fed as: bos for "", else the word's tokens, at most
/// `rank::LM_WINDOW` of them (a pasted run of letters isn't worth more).
fn word_tokens(lm: &Lm, word: &str) -> Vec<u32> {
    if word.is_empty() {
        return vec![BOS];
    }
    let mut tokens = lm.tokenizer().word(word);
    tokens.truncate(rank::LM_WINDOW);
    tokens
}

impl LmContext {
    /// Brings the cache in step with `words` (from `lm_words`), feeding at most until `more`
    /// says stop (asked before each word). True when the context is ready: every word fed and
    /// the next-token distribution read. False leaves the work done so far for the next call.
    pub fn sync(&mut self, lm: &Lm, words: &[String], more: &dyn Fn() -> bool) -> bool {
        let start = match self.reused(words) {
            Some(Run { at, from, len }) => {
                let kept = at + len;
                if kept < self.words.len() {
                    let rows = self.words[..kept].iter().map(|(_, n)| n).sum();
                    if let Some(cache) = &mut self.cache {
                        lm.truncate(cache, rows);
                    }
                    self.words.truncate(kept);
                    self.ready = false;
                }
                from + len
            }
            None => {
                self.cache = Some(lm.cache());
                self.words.clear();
                self.dropped = None;
                self.ready = false;
                // The longest tail of whole words that fits the window (at least one word).
                let mut tokens = 0;
                let mut j = words.len();
                while j > 0 {
                    let n = word_tokens(lm, &words[j - 1]).len();
                    if tokens + n > rank::LM_WINDOW && j < words.len() {
                        break;
                    }
                    tokens += n;
                    j -= 1;
                }
                self.dropped = j.checked_sub(1).map(|i| words[i].clone());
                j
            }
        };
        let cache = self.cache.get_or_insert_with(|| lm.cache());
        for word in &words[start..] {
            if !more() {
                return false;
            }
            let tokens = word_tokens(lm, word);
            lm.feed(cache, &tokens, &mut self.scratch);
            self.fed += tokens.len();
            self.words.push((word.clone(), tokens.len()));
            self.ready = false;
            // The oldest whole words go while the window is over; never the newest.
            while cache.rows() > rank::LM_WINDOW && self.words.len() > 1 {
                let (word, n) = self.words.remove(0);
                lm.evict_front(cache, n);
                self.dropped = Some(word);
            }
        }
        if !self.ready {
            self.next.clear();
            self.next
                .extend_from_slice(lm.next_log_probs(cache, &mut self.scratch));
            self.lang = lang(&self.next, lm);
            self.ready = true;
        }
        true
    }

    /// The cached words kept for `words`: `matched`'s run, or `slid`'s when that is longer.
    /// None when the cache is filled afresh.
    fn reused(&self, words: &[String]) -> Option<Run> {
        if self.words.is_empty()
            || self
                .cache
                .as_ref()
                .is_none_or(|c| c.next_pos() > MAX_POSITION)
        {
            return None;
        }
        let matched = self.matched(words);
        match self.slid(words) {
            Some(slid) if matched.is_none_or(|m| slid.len > m.len) => Some(slid),
            _ => matched,
        }
    }

    /// Where the cached words reappear in `words`: the first `len` cached words are
    /// `words[from..from + len]` (the longest such run; ties, the latest `from`). None when
    /// nothing matches or when the run can't be the cache's own text.
    fn matched(&self, words: &[String]) -> Option<Run> {
        let mut best: Option<Run> = None;
        for from in 0..words.len() {
            let len = self.run(0, words, from);
            if len > 0 && best.is_none_or(|b| len >= b.len) {
                best = Some(Run { at: 0, from, len });
            }
        }
        let best = best?;
        // Past the start of the text, the run must follow the word the cache last dropped;
        // otherwise it is a repeated phrase elsewhere, and the cache would hold less context
        // than a fresh fill.
        if best.from > 0 && self.dropped.as_deref() != Some(words[best.from - 1].as_str()) {
            return None;
        }
        Some(best)
    }

    /// The keyboard sends only the end of the text before the cursor (its last 64
    /// characters), so once the text is longer its front is cut at every keystroke, often
    /// inside a word: `words` are then bos (`lm_words` can't tell a cut text from a whole
    /// one), perhaps the end of the cut word, and the words the cache holds. `matched` finds
    /// only bos there and every word was fed again at each keystroke: 0.9 of 1.6 ms a
    /// suggest on the Mac, and on the phone past the 3 ms prefill cutoff while typing, so the
    /// LM was never ready. This keeps the cached words that `words` end with, and the cached
    /// words before them, which are the text that was cut. At least `SLID_RUN` words must
    /// match, with at most one new word after them; anything else is another text.
    fn slid(&self, words: &[String]) -> Option<Run> {
        let lead = usize::from(words.first().is_some_and(String::is_empty));
        let mut best: Option<Run> = None;
        for at in 0..self.words.len() {
            // The word before cached word `at`: the cache's own, or the one it dropped last.
            let before = match at {
                0 => self.dropped.as_deref(),
                _ => Some(self.words[at - 1].0.as_str()),
            };
            // After bos, the text starts at word `at`, or with the end of the word before it.
            let cut = words.get(lead).is_some_and(|w| {
                before.is_some_and(|b| b.len() > w.len() && b.ends_with(w.as_str()))
            });
            let from = lead + usize::from(cut);
            if at == 0 && from == 0 {
                continue; // `matched` has it
            }
            let len = self.run(at, words, from);
            if len >= SLID_RUN && from + len + 1 >= words.len() && best.is_none_or(|b| len > b.len)
            {
                best = Some(Run { at, from, len });
            }
        }
        best
    }

    /// How many cached words from `at` on are `words` from `from` on.
    fn run(&self, at: usize, words: &[String], from: usize) -> usize {
        self.words[at..]
            .iter()
            .zip(&words[from..])
            .take_while(|((cached, _), word)| cached == *word)
            .count()
    }

    /// Whether the context is ready: `next` and `lang` hold for the current text.
    pub fn is_ready(&self) -> bool {
        self.ready
    }

    /// Log-probabilities of the next token after the context (empty before the first sync).
    pub fn next(&self) -> &[f32] {
        &self.next
    }

    /// The language posterior for the next word.
    pub fn lang(&self) -> Lang {
        self.lang
    }

    /// Tokens fed since this context was made: for tests and diagnostics.
    pub fn fed_tokens(&self) -> usize {
        self.fed
    }

    /// Tokens in the cache.
    pub fn cached_tokens(&self) -> usize {
        self.cache.as_ref().map_or(0, Cache::rows)
    }

    /// The cached words, oldest first, without the bos "".
    pub fn cached_words(&self) -> Vec<String> {
        self.words
            .iter()
            .filter(|(w, _)| !w.is_empty())
            .map(|(w, _)| w.clone())
            .collect()
    }
}

/// A word-start piece the LM offers next, with what next words need (design §6).
#[derive(Debug, Clone, PartialEq)]
pub struct NextPiece {
    /// The piece without its word mark.
    pub text: String,
    /// A Latin piece (English candidates); otherwise the pack's native script.
    pub latin: bool,
    /// ln p(t | ctx) + lang(t).
    pub log_p: f32,
    /// `lex.first_en[t]` or `lex.first_ml[t]`.
    pub first_log_mass: f32,
}

/// What the engine asks of the context. The LM implements it (`LmView`); tests use fakes.
pub trait ContextScores {
    /// The Manglish bias for the word being typed, on the scale of `rank::CONTEXT_*`.
    fn manglish_bias(&self) -> f32;
    /// The weighted, clamped tier-0 correction for a candidate (0 when the LM says nothing).
    fn weighted(&self, word: &str) -> f32;
    /// The same for the English words that begin with the typed letters, as one candidate.
    fn prefix_delta(&self, typed: &str) -> f32;
    /// The `k` likeliest word-start pieces that are Latin or the native script, in the next
    /// word's language.
    fn next_pieces(&self, k: usize) -> Vec<NextPiece>;
    /// The first piece of `word` as `next_pieces` would give it, if it would.
    fn piece_of(&self, word: &str) -> Option<NextPiece>;
    /// The first piece of `word`, without its word mark, if it is a word-start piece.
    fn first_piece(&self, word: &str) -> Option<String>;
}

/// The tunable weights of design §6, defaulting to `rank`'s constants. The eval harness may
/// set others (`Engine::set_lm_weights`) for sweeps; the keyboard never does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LmWeights {
    pub en: f32,
    pub ml: f32,
    pub clip: f32,
    pub lang_weight: f32,
    pub lang_offset: f32,
    pub next_bigram: f32,
}

impl Default for LmWeights {
    fn default() -> Self {
        Self {
            en: rank::LM_WEIGHT_EN,
            ml: rank::LM_WEIGHT_ML,
            clip: rank::LM_CLIP,
            lang_weight: rank::LANG_WEIGHT,
            lang_offset: rank::LANG_OFFSET,
            next_bigram: rank::NEXT_BIGRAM_WEIGHT,
        }
    }
}

/// The LM's scores for a ready context.
pub struct LmView<'a> {
    pub lm: &'a Lm,
    pub ctx: &'a LmContext,
    pub weights: LmWeights,
}

/// Next words are in the sentence's language (design §1): the likelier of the two. Its split
/// per piece (`p_en`) is too rough to mix them: after `ente`, a share of `▁a`'s mass, which
/// Manglish words begin, would offer English "a".
fn next_in_english(lang: Lang) -> bool {
    lang.log_en > lang.log_ml
}

impl LmView<'_> {
    /// `(ln p(t | ctx) + lang(t), first_log_mass, latin)` for a word-start piece `t` in the
    /// next word's language (`english`) that begins some lexicon word; None for any other.
    fn next_piece(&self, t: u32, english: bool) -> Option<(f32, f32, bool)> {
        let (f, tab, ctx) = (self.lm.tokenizer().flags(t), self.lm.tables(), self.ctx);
        let (lp, lang, t) = (ctx.next()[t as usize], ctx.lang(), t as usize);
        if f & flag::WORD_START == 0 || english != (f & flag::LATIN != 0) {
            return None;
        }
        let (log_p, first, latin) = if f & flag::NATIVE != 0 {
            (lp + lang.log_ml - lang.log_script, tab.first_ml[t], false)
        } else if f & flag::LATIN != 0 {
            (lp + tab.p_en[t].ln(), tab.first_en[t], true)
        } else {
            return None;
        };
        (log_p.is_finite() && first.is_finite()).then_some((log_p, first, latin))
    }

    /// `next_piece` as a `NextPiece`.
    fn piece(&self, t: u32, english: bool) -> Option<NextPiece> {
        let (log_p, first_log_mass, latin) = self.next_piece(t, english)?;
        Some(NextPiece {
            text: self.lm.tokenizer().piece(t)[WORD_START.len_utf8()..].to_string(),
            latin,
            log_p,
            first_log_mass,
        })
    }

    /// The clamped tier-0 correction for `word` and whether it is Latin, or None when its
    /// first piece isn't a Latin or native-script word start, or the lexicon has no word starting
    /// with it (design §6).
    pub fn delta(&self, word: &str) -> Option<(f32, bool)> {
        let tok = self.lm.tokenizer();
        let &t = tok.word(word).first()?;
        let (f, t) = (tok.flags(t), t as usize);
        if f & flag::WORD_START == 0 {
            return None;
        }
        let (tab, next, lang) = (self.lm.tables(), self.ctx.next()[t], self.ctx.lang());
        let (raw, latin) = if f & flag::NATIVE != 0 {
            (next - lang.log_script - tab.first_ml[t], false)
        } else if f & flag::LATIN != 0 {
            // A piece never seen in English text has p_en = 0: the correction is then the
            // clip, not -inf.
            (
                next + tab.p_en[t].max(1e-6).ln() - lang.log_en - tab.first_en[t],
                true,
            )
        } else {
            return None;
        };
        raw.is_finite()
            .then(|| (raw.clamp(-self.weights.clip, self.weights.clip), latin))
    }
}

impl ContextScores for LmView<'_> {
    fn manglish_bias(&self) -> f32 {
        let l = self.ctx.lang();
        (self.weights.lang_weight * (l.log_ml - l.log_en) + self.weights.lang_offset)
            .clamp(rank::CONTEXT_AFTER_ENGLISH, rank::CONTEXT_AFTER_MALAYALAM)
    }

    fn weighted(&self, word: &str) -> f32 {
        match self.delta(word) {
            Some((d, true)) => self.weights.en * d,
            Some((d, false)) => self.weights.ml * d,
            None => 0.0,
        }
    }

    fn prefix_delta(&self, typed: &str) -> f32 {
        let (tok, tab) = (self.lm.tokenizer(), self.lm.tables());
        let (next, lang) = (self.ctx.next(), self.ctx.lang());
        let prefix = format!("{WORD_START}{}", tok.normal(typed));
        let (mut lm_mass, mut lex_mass) = (0f64, 0f64);
        for t in tok.starting_with(&prefix) {
            if tok.flags(t) & flag::LATIN == 0 {
                continue;
            }
            let t = t as usize;
            lm_mass += f64::from(next[t].exp()) * f64::from(tab.p_en[t]);
            lex_mass += f64::from(tab.first_en[t].exp());
        }
        if lm_mass <= 0.0 || lex_mass <= 0.0 {
            return 0.0;
        }
        let raw = lm_mass.ln() as f32 - lang.log_en - lex_mass.ln() as f32;
        self.weights.en * raw.clamp(-self.weights.clip, self.weights.clip)
    }

    fn next_pieces(&self, k: usize) -> Vec<NextPiece> {
        let english = next_in_english(self.ctx.lang());
        // Word starts in that language that begin some lexicon word: (id, log_p).
        let mut found: Vec<(u32, f32)> = (0..self.ctx.next().len() as u32)
            .filter_map(|t| Some((t, self.next_piece(t, english)?.0)))
            .collect();
        let by_score = |a: &(u32, f32), b: &(u32, f32)| b.1.total_cmp(&a.1);
        if found.len() > k {
            found.select_nth_unstable_by(k, by_score);
            found.truncate(k);
        }
        found.sort_by(by_score);
        found
            .into_iter()
            .filter_map(|(t, _)| self.piece(t, english))
            .collect()
    }

    fn piece_of(&self, word: &str) -> Option<NextPiece> {
        let &t = self.lm.tokenizer().word(word).first()?;
        self.piece(t, next_in_english(self.ctx.lang()))
    }

    fn first_piece(&self, word: &str) -> Option<String> {
        let tok = self.lm.tokenizer();
        let &t = tok.word(word).first()?;
        (tok.flags(t) & flag::WORD_START != 0)
            .then(|| tok.piece(t)[WORD_START.len_utf8()..].to_string())
    }
}

/// The language posterior over word-start pieces (design §6).
fn lang(next: &[f32], lm: &Lm) -> Lang {
    let (tok, p_en) = (lm.tokenizer(), &lm.tables().p_en);
    let (mut en, mut ml, mut script) = (0f64, 0f64, 0f64);
    for (t, &lp) in next.iter().enumerate() {
        let f = tok.flags(t as u32);
        if f & flag::WORD_START == 0 {
            continue;
        }
        let p = f64::from(lp.exp());
        if f & flag::NATIVE != 0 {
            ml += p;
            script += p;
        } else if f & flag::LATIN != 0 {
            let e = f64::from(p_en[t]);
            en += p * e;
            ml += p * (1.0 - e);
        }
    }
    let ln = |x: f64| x.max(1e-30).ln() as f32;
    Lang {
        log_en: ln(en),
        log_ml: ln(ml),
        log_script: ln(script),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nn::Lm;

    /// The tiny fixture LM, its native pieces Malayalam's as the engine sets them for the pack.
    fn lm() -> Lm {
        let mut lm = Lm::open(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lm-tiny.mlkbnn"),
        )
        .unwrap();
        lm.set_native('\u{0D00}'..='\u{0D7F}');
        lm
    }

    fn sync(ctx: &mut LmContext, lm: &Lm, text: &str) -> bool {
        ctx.sync(lm, &lm_words(text), &|| true)
    }

    #[test]
    fn words_are_normalised_and_start_with_bos_when_they_are_the_whole_text() {
        assert_eq!(lm_words("Hi  THERE "), ["", "hi", "there"]);
        let long = "w ".repeat(rank::LM_WORDS + 5);
        assert!(!lm_words(&long)[0].is_empty());
        assert_eq!(lm_words(&long).len(), rank::LM_WORDS);
    }

    #[test]
    fn typing_on_feeds_only_the_new_word() {
        let (lm, mut ctx) = (lm(), LmContext::default());
        assert!(sync(&mut ctx, &lm, "i am "));
        let fed = ctx.fed_tokens();
        assert!(sync(&mut ctx, &lm, "i am here "));
        assert_eq!(ctx.fed_tokens() - fed, lm.tokenizer().word("here").len());
    }

    #[test]
    fn editing_the_last_word_truncates_and_refeeds_it() {
        let (lm, mut ctx) = (lm(), LmContext::default());
        sync(&mut ctx, &lm, "i am here ");
        let fed = ctx.fed_tokens();
        sync(&mut ctx, &lm, "i am there ");
        assert_eq!(ctx.fed_tokens() - fed, lm.tokenizer().word("there").len());
        let mut fresh = LmContext::default();
        sync(&mut fresh, &lm, "i am there ");
        assert_eq!(ctx.next(), fresh.next());
    }

    #[test]
    fn the_window_keeps_whole_words_within_its_tokens() {
        let (lm, mut ctx) = (lm(), LmContext::default());
        let text: String = (0..30).map(|i| format!("word{i} ")).collect();
        sync(&mut ctx, &lm, &text);
        assert!(ctx.cached_tokens() <= rank::LM_WINDOW);
        assert_eq!(
            ctx.cached_words().last().map(String::as_str),
            Some("word29")
        );
        // Typing on slides the window: only the new word is fed.
        let fed = ctx.fed_tokens();
        sync(&mut ctx, &lm, &format!("{text}more "));
        assert_eq!(ctx.fed_tokens() - fed, lm.tokenizer().word("more").len());
        assert!(ctx.cached_tokens() <= rank::LM_WINDOW);
    }

    /// The last `n` characters of `text`, as the keyboard sends them.
    fn window(text: &str, n: usize) -> &str {
        let skip = text.chars().count().saturating_sub(n);
        &text[text.char_indices().nth(skip).map_or(text.len(), |(i, _)| i)..]
    }

    #[test]
    fn a_text_cut_at_the_front_keeps_the_cache() {
        let (lm, mut ctx) = (lm(), LmContext::default());
        let text = "we could meet at the cafe near the station tomorrow ";
        sync(&mut ctx, &lm, text);
        let fed = ctx.fed_tokens();
        // Cut inside a word ("uld meet …") and at a word boundary ("meet …"): nothing to feed.
        for cut in [window(text, 46), window(text, 43)] {
            assert!(sync(&mut ctx, &lm, cut));
            assert_eq!(ctx.fed_tokens(), fed, "{cut:?}");
        }
        // A new word after the cut text: only it is fed, and the context is the one the
        // uncut text gives, cut words and all.
        let more = format!("{text}then ");
        assert!(sync(&mut ctx, &lm, window(&more, 46)));
        assert_eq!(ctx.fed_tokens() - fed, lm.tokenizer().word("then").len());
        let mut whole = LmContext::default();
        sync(&mut whole, &lm, text);
        sync(&mut whole, &lm, &more);
        assert_eq!(ctx.cached_words(), whole.cached_words());
        assert_eq!(ctx.next(), whole.next());
    }

    #[test]
    fn a_text_ending_in_cached_words_is_another_text_without_a_cut() {
        let (lm, mut ctx) = (lm(), LmContext::default());
        sync(&mut ctx, &lm, "we could meet at the cafe near the station ");
        // Three cached words, but after a word the cache doesn't have before them.
        for other in [
            "so the cafe near the station ",
            "near the station and then home ",
        ] {
            sync(&mut ctx, &lm, other);
            let mut fresh = LmContext::default();
            sync(&mut fresh, &lm, other);
            assert_eq!(ctx.cached_words(), fresh.cached_words(), "{other:?}");
            assert_eq!(ctx.next(), fresh.next());
            sync(&mut ctx, &lm, "we could meet at the cafe near the station ");
        }
    }

    #[test]
    fn a_repeated_word_elsewhere_is_no_match() {
        // "the" begins the cache's words, but a new text that only shares "the" with it is
        // filled afresh, from bos.
        let (lm, mut ctx) = (lm(), LmContext::default());
        let text: String = (0..30).map(|i| format!("word{i} ")).collect();
        sync(&mut ctx, &lm, &format!("{text}the end "));
        let first = ctx.cached_words()[0].clone();
        sync(&mut ctx, &lm, &format!("so {first} "));
        let mut fresh = LmContext::default();
        sync(&mut fresh, &lm, &format!("so {first} "));
        assert_eq!(ctx.cached_words(), fresh.cached_words());
        assert_eq!(ctx.next(), fresh.next());
    }

    #[test]
    fn running_out_of_time_resumes_on_the_next_call() {
        let (lm, mut ctx) = (lm(), LmContext::default());
        let words = lm_words("one two three four ");
        let calls = std::cell::Cell::new(0);
        let two_words = || {
            calls.set(calls.get() + 1);
            calls.get() <= 2
        };
        assert!(!ctx.sync(&lm, &words, &two_words));
        assert!(!ctx.is_ready());
        assert!(ctx.sync(&lm, &words, &|| true));
        let mut fresh = LmContext::default();
        fresh.sync(&lm, &words, &|| true);
        assert_eq!(ctx.next(), fresh.next());
    }

    #[test]
    fn delta_is_zero_for_a_piece_the_context_says_nothing_about() {
        // With next[t] equal to the lexicon's first-token share (scaled to the language),
        // the correction is 0.
        let (lm, mut ctx) = (lm(), LmContext::default());
        sync(&mut ctx, &lm, "i am ");
        let view = LmView {
            lm: &lm,
            ctx: &ctx,
            weights: LmWeights::default(),
        };
        let t = lm.tokenizer().word("the")[0] as usize;
        let want = ctx.next()[t] + lm.tables().p_en[t].max(1e-6).ln()
            - ctx.lang().log_en
            - lm.tables().first_en[t];
        let (got, latin) = view.delta("the").unwrap();
        assert!(latin);
        assert!((got - want.clamp(-rank::LM_CLIP, rank::LM_CLIP)).abs() < 1e-6);
        assert_eq!(view.weighted("the"), rank::LM_WEIGHT_EN * got);
        assert_eq!(view.delta("..."), None, "punctuation says nothing about a word");
    }

    #[test]
    fn next_pieces_are_word_starts_in_order() {
        let (lm, mut ctx) = (lm(), LmContext::default());
        sync(&mut ctx, &lm, "i am ");
        let view = LmView {
            lm: &lm,
            ctx: &ctx,
            weights: LmWeights::default(),
        };
        let pieces = view.next_pieces(5);
        assert!(!pieces.is_empty() && pieces.len() <= 5);
        assert!(pieces.windows(2).all(|w| w[0].log_p >= w[1].log_p));
        assert!(pieces.iter().all(|p| p.first_log_mass.is_finite()));
        let the = view.first_piece("the").unwrap();
        assert!("the".starts_with(&the));
    }

    #[test]
    fn language_posterior_adds_up() {
        let (lm, mut ctx) = (lm(), LmContext::default());
        sync(&mut ctx, &lm, "i am ");
        let l = ctx.lang();
        assert!(l.log_script <= l.log_ml + 1e-6);
        assert!((l.log_en.exp() + l.log_ml.exp()) <= 1.0 + 1e-4);
    }
}
