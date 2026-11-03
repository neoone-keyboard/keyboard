//! Platform-free keyboard engine. Nothing in this crate may depend on iOS or Android APIs.

pub mod engine;
pub mod fuzzy;
pub mod keys;
pub mod lexicon;
pub mod malayalam;
pub mod manglish;
pub mod nn;
pub mod normalize;
pub mod rank;
pub mod romanize;
pub mod suffix;
pub mod text;
pub mod translit;
pub mod user;

pub use engine::{Candidate, Engine, Kind, Layout, Paths, Sources, Suggestions};
