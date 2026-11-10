//! Malayalam (mlym). The bodies still live in `malayalam.rs`, `normalize.rs` and `suffix.rs`
//! (the joins); this file is the plug-in face the engine sees.

use std::ops::RangeInclusive;

use super::ScriptRules;

#[derive(Debug)]
pub struct Mlym;

impl ScriptRules for Mlym {
    fn code(&self) -> &'static str {
        "mlym"
    }
    fn block(&self) -> RangeInclusive<char> {
        '\u{0D00}'..='\u{0D7F}'
    }
    fn is_letter(&self, c: char) -> bool {
        ('\u{0D00}'..='\u{0D63}').contains(&c) || ('\u{0D7A}'..='\u{0D7F}').contains(&c)
    }
    fn normalize(&self, word: &str) -> String {
        crate::normalize::normalize(word)
    }
    fn mark_base(&self, context_before: &str) -> Option<char> {
        crate::malayalam::vowel_sign_base(context_before)
    }
    fn vowel_sign_drops(&self, word: &str) -> Vec<String> {
        crate::malayalam::vowel_sign_drops(word)
    }
    fn other_final_u(&self, word: &str) -> Option<String> {
        crate::malayalam::other_final_u(word)
    }
    /// The whole block, digits and signs too, as `text::words` has always split Malayalam.
    fn is_word_char(&self, c: char) -> bool {
        self.block().contains(&c)
    }
    fn join_suffix(&self, stem: &str, suffix: &str, how: crate::suffix::Join) -> Option<String> {
        crate::suffix::join(stem, suffix, how)
    }
    /// The chillus, the final-u spellings and the suffix joins are code.
    fn needs_code(&self) -> bool {
        true
    }
}
