//! Platform-free keyboard engine. Nothing in this crate may depend on iOS or Android APIs.

pub mod chat_forms;
pub mod engine;
pub mod fuzzy;
pub mod keys;
pub mod lang;
pub mod lexicon;
pub mod lm_context;
pub mod malayalam;
pub mod manglish;
pub mod nn;
pub mod normalize;
pub mod pack;
pub mod pack_layout;
pub mod personal;
pub mod rank;
pub mod romanisation;
pub mod script;
pub mod suffix;
pub mod swipe;
pub mod text;
pub mod translit;
pub mod user;

pub use engine::{
    Candidate, DEFAULT_SUGGESTIONS, Engine, Kind, Layout, MAX_SUGGESTIONS, PackPaths, PackSources,
    Paths, Sources, Suggestions, Timing,
};
