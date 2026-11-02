//! Platform-free keyboard engine. Nothing in this crate may depend on iOS or Android APIs.

pub mod text;

/// The prediction engine. M0 echoes the current word; real candidates arrive in M2.
#[derive(Debug, Default)]
pub struct Engine;

impl Engine {
    pub fn new() -> Self {
        Self
    }

    pub fn suggest(&self, context_before: &str) -> Vec<String> {
        match text::current_word(context_before) {
            "" => Vec::new(),
            word => vec![word.to_string()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Engine;

    #[test]
    fn suggests_current_word() {
        assert_eq!(Engine::new().suggest("hello wor"), vec!["wor".to_string()]);
    }

    #[test]
    fn no_suggestions_after_a_space() {
        assert!(Engine::new().suggest("hello ").is_empty());
    }
}
