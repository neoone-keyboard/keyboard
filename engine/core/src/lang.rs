//! Languages as values (M6 design, decision 4). English is built in; every other language is a
//! loaded pack. Up to `MAX_ACTIVE` of them are active at once, English counted, and English is
//! as optional as any other.

use std::fmt;

/// How many languages can be active at once, English included. The only place the number is
/// written: raising it needs memory and latency measured on the phone, not new code.
pub const MAX_ACTIVE: usize = 2;

/// A language's id: "en", or a pack's manifest `id` ("ml", "hi", "as").
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LangId(pub String);

impl LangId {
    pub fn english() -> Self {
        Self("en".into())
    }
}

impl fmt::Display for LangId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A language's place in the engine's table: 0 is English, 1.. the loaded packs in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Lang(pub u8);

/// English's place.
pub const EN: Lang = Lang(0);

#[derive(Debug, PartialEq, Eq)]
pub enum ActiveError {
    Empty,
    TooMany(usize),
    Repeated(Lang),
}

impl fmt::Display for ActiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "at least one language must be active"),
            Self::TooMany(n) => write!(f, "{n} languages, at most {MAX_ACTIVE} can be active"),
            Self::Repeated(l) => write!(f, "language {} is listed twice", l.0),
        }
    }
}

impl std::error::Error for ActiveError {}

/// The active languages in the user's order: never empty, at most `MAX_ACTIVE`, no repeats.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Active {
    order: Vec<Lang>,
}

impl Active {
    pub fn new(order: Vec<Lang>) -> Result<Self, ActiveError> {
        if order.is_empty() {
            return Err(ActiveError::Empty);
        }
        if order.len() > MAX_ACTIVE {
            return Err(ActiveError::TooMany(order.len()));
        }
        for (i, l) in order.iter().enumerate() {
            if order[..i].contains(l) {
                return Err(ActiveError::Repeated(*l));
            }
        }
        Ok(Self { order })
    }

    pub fn contains(&self, lang: Lang) -> bool {
        self.order.contains(&lang)
    }

    pub fn english(&self) -> bool {
        self.contains(EN)
    }

    pub fn iter(&self) -> impl Iterator<Item = Lang> + '_ {
        self.order.iter().copied()
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    /// Never true; here for clippy's `len_without_is_empty`.
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_order() {
        let a = Active::new(vec![Lang(1), EN]).unwrap();
        assert_eq!(a.iter().collect::<Vec<_>>(), vec![Lang(1), EN]);
        assert!(a.english());
        assert!(!a.contains(Lang(2)));
    }

    #[test]
    fn english_is_optional() {
        let a = Active::new(vec![Lang(1)]).unwrap();
        assert!(!a.english());
    }

    #[test]
    fn refuses_empty_too_many_and_repeats() {
        assert_eq!(Active::new(vec![]), Err(ActiveError::Empty));
        let many: Vec<Lang> = (0..=MAX_ACTIVE as u8).map(Lang).collect();
        assert_eq!(Active::new(many), Err(ActiveError::TooMany(MAX_ACTIVE + 1)));
        assert_eq!(Active::new(vec![EN, EN]), Err(ActiveError::Repeated(EN)));
    }
}
