//! The LM's view of the text before the cursor (design §8): a context cache kept in step with
//! the committed words, the next-token distribution after them, and the language posterior.
//! Everything here is optional work: `sync` stops when told to and resumes on the next call,
//! and until it has caught up the engine ignores the LM.

use std::collections::HashMap;

use crate::nn::lm::MAX_POSITION;
use crate::nn::tokenizer::{BOS, flag, lm_normalize};
use crate::nn::{Cache, Lm, LmScratch};
use crate::rank;

/// The committed words before the cursor, `lm_normalize`d, the last `rank::LM_WORDS` of them;
/// a leading "" stands for bos when they are the whole text.
pub fn lm_words(before: &str) -> Vec<String> {
    let normal = lm_normalize(before);
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

/// Log probabilities that the next word is English, Malayalam in any script, and in
/// Malayalam script (design §6).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Lang {
    pub log_en: f32,
    pub log_ml: f32,
    pub log_script: f32,
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
    /// Tier-1 results for this context (Task 15).
    pub(crate) memo: HashMap<String, f32>,
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
        let start = self.matched(words);
        let start = match start {
            Some((j, k)) => {
                if k < self.words.len() {
                    let rows = self.words[..k].iter().map(|(_, n)| n).sum();
                    if let Some(cache) = &mut self.cache {
                        lm.truncate(cache, rows);
                    }
                    self.words.truncate(k);
                    self.ready = false;
                }
                j + k
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
            self.memo.clear();
            self.ready = true;
        }
        true
    }

    /// Where the cached words reappear in `words`: `(j, k)` when the first `k` cached words
    /// are `words[j..j + k]` (the longest such run; ties, the latest `j`). None when nothing
    /// matches, when the run can't be the cache's own text, or when positions ran out: the
    /// cache is then filled afresh.
    fn matched(&self, words: &[String]) -> Option<(usize, usize)> {
        let cache = self.cache.as_ref()?;
        if cache.next_pos() > MAX_POSITION {
            return None;
        }
        let mut best: Option<(usize, usize)> = None;
        for j in 0..words.len() {
            let k = self
                .words
                .iter()
                .zip(&words[j..])
                .take_while(|((cached, _), word)| cached == *word)
                .count();
            if k > 0 && best.is_none_or(|(_, b)| k >= b) {
                best = Some((j, k));
            }
        }
        let (j, k) = best?;
        // Past the start of the text, the run must follow the word the cache last dropped;
        // otherwise it is a repeated phrase elsewhere, and the cache would hold less context
        // than a fresh fill.
        if j > 0 && self.dropped.as_deref() != Some(words[j - 1].as_str()) {
            return None;
        }
        Some((j, k))
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
        if f & flag::MALAYALAM != 0 {
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

    fn lm() -> Lm {
        Lm::open(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lm-tiny.mlkbnn"),
        )
        .unwrap()
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
    fn language_posterior_adds_up() {
        let (lm, mut ctx) = (lm(), LmContext::default());
        sync(&mut ctx, &lm, "i am ");
        let l = ctx.lang();
        assert!(l.log_script <= l.log_ml + 1e-6);
        assert!((l.log_en.exp() + l.log_ml.exp()) <= 1.0 + 1e-4);
    }
}
