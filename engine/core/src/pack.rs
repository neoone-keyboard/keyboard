//! Language packs (M6 design): a folder `<id>.neopack/` holding `manifest.json` and the
//! language's data files. The manifest maps roles (`lexicon`, `romanisedLexicon`,
//! `romanisation`, `nextWords`, `translit`, `lm`, `suffixes`, `chatForms`, `layout`, `lang`) to file
//! names in the folder; `Pack::open` reads and checks it, and `Paths::with_packs` turns it into
//! the paths the engine opens.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::lang::LangId;
use crate::{PackPaths, Paths};

/// The pack format this engine reads.
pub const FORMAT: u32 = 1;
/// This engine's pack level: a pack whose `minEngine` is higher needs a newer app.
/// 1: phase 1 (Malayalam, its tables compiled in). 2: phase 2, which reads format 1 with the
/// `romanisation`, `suffixes`, `chatForms` and `layout` roles, any number of packs, and the
/// scripts `mlym`, `deva` and `beng` (Hindi and Assamese need it). 3: an `lm` in a
/// pack whose script isn't Malayalam's (Hindi's): the LM's native pieces are the pack's script
/// (`Lm::set_native`). Engine 2 would load such an LM but see none of its native pieces, and
/// rank the pack's words worse than with no LM. 4: the `lang` role, a `lang.json` that defines
/// the pack's script as data (`script::data`), so a pack may be in any script. Only a pack
/// whose script the engine has no copy of (`script::supported`) needs 4: a pack in mlym, deva
/// or beng may carry the file and keep a lower `minEngine`, as older engines ignore the role
/// and have those scripts' rules in code. 5: `lang.json`'s `notLetter`, `looseSigns`,
/// `finalPairs` and `suffixJoins`, with which every script's rules are data (Malayalam's too:
/// the engine's own copies of the mlym, deva and beng files replace the coded plug-ins). Only
/// a pack in another script that uses one of those fields needs 5; Malayalam's pack doesn't,
/// as engines before 5 have its rules in code and ignore the fields.
pub const PACK_ENGINE: u32 = 5;

/// `manifest.json`. Keys are camelCase in the file.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format: u32,
    pub id: String,
    pub version: String,
    pub name: String,
    pub native_name: String,
    pub script: String,
    #[serde(default)]
    pub romanised: Option<Romanised>,
    pub min_engine: u32,
    /// The `en.fst` the pack's LM and next words were built against (FNV-1a of the file, as
    /// `Lexicon::fingerprint`, 16 hex digits, little-endian bytes in order). Not checked when a
    /// pack opens: an LM is only loaded when its own tables fit the lexicons (`lm_fits`), and
    /// with another `en.fst` next words still work, only the English pairs are a little off.
    /// Phase 3's installer compares it with the base's to offer an update.
    #[serde(default)]
    pub english_fingerprint: Option<String>,
    /// Role → file name in the pack folder.
    pub files: BTreeMap<String, String>,
    #[serde(default)]
    pub sha256: BTreeMap<String, String>,
    #[serde(default)]
    pub credits: Vec<Credit>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Romanised {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Credit {
    pub name: String,
    pub licence: String,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug)]
pub enum PackError {
    /// No `manifest.json`, or it couldn't be read.
    Unreadable(String),
    /// Not valid JSON for a manifest.
    Malformed(String),
    /// A format this engine doesn't read.
    Format(u32),
    /// A script this engine has no rules for, or a bad `lang` file: why, in words.
    Script(String),
    /// The pack needs a newer engine.
    Engine(u32),
    /// The required `lexicon` file is missing.
    NoLexicon,
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable(e) => write!(f, "manifest unreadable: {e}"),
            Self::Malformed(e) => write!(f, "manifest malformed: {e}"),
            Self::Format(v) => write!(f, "pack format {v}, this engine reads {FORMAT}"),
            Self::Script(why) => write!(f, "script rules: {why}"),
            Self::Engine(v) => write!(f, "pack needs engine {v}, this is {PACK_ENGINE}"),
            Self::NoLexicon => write!(f, "pack has no lexicon file"),
        }
    }
}

impl std::error::Error for PackError {}

/// An opened, checked pack.
#[derive(Debug, Clone)]
pub struct Pack {
    pub dir: PathBuf,
    pub manifest: Manifest,
}

impl Pack {
    pub fn open(dir: &Path) -> Result<Self, PackError> {
        let text = std::fs::read_to_string(dir.join("manifest.json"))
            .map_err(|e| PackError::Unreadable(e.to_string()))?;
        let manifest: Manifest =
            serde_json::from_str(&text).map_err(|e| PackError::Malformed(e.to_string()))?;
        if manifest.format != FORMAT {
            return Err(PackError::Format(manifest.format));
        }
        if manifest.min_engine > PACK_ENGINE {
            return Err(PackError::Engine(manifest.min_engine));
        }
        let pack = Self {
            dir: dir.to_path_buf(),
            manifest,
        };
        if let (Some(name), None) = (pack.manifest.files.get("lang"), pack.file("lang")) {
            return Err(PackError::Script(format!(
                "the manifest names a lang file, {name:?}, that isn't in the pack"
            )));
        }
        crate::script::for_pack(&pack.manifest.script, pack.file("lang").as_deref())
        .map_err(PackError::Script)?;
        if pack.file("lexicon").is_none() {
            return Err(PackError::NoLexicon);
        }
        Ok(pack)
    }

    /// The file for `role`, if the manifest names one and it is there.
    pub fn file(&self, role: &str) -> Option<PathBuf> {
        let name = self.manifest.files.get(role)?;
        // A file name, never a path out of the pack.
        if name.is_empty() || name.contains('/') || name.contains('\\') || name.starts_with('.') {
            return None;
        }
        let path = self.dir.join(name);
        path.is_file().then_some(path)
    }
}

impl Pack {
    /// The paths of this pack's files, as the engine opens them.
    pub fn paths(&self) -> PackPaths {
        PackPaths {
            id: LangId(self.manifest.id.clone()),
            script: self.manifest.script.clone(),
            dir: Some(self.dir.clone()),
            lexicon: self.file("lexicon"),
            romanised: self.file("romanisedLexicon"),
            romanisation: self.file("romanisation"),
            next_words: self.file("nextWords"),
            translit: self.file("translit"),
            lm: self.file("lm"),
            suffixes: self.file("suffixes"),
            chat_forms: self.file("chatForms"),
            layout: self.file("layout"),
            lang: self.file("lang"),
        }
    }
}

impl Paths {
    /// The paths for English at `english` plus `packs`' languages, in order; `user_dir` is kept.
    pub fn with_packs(english: Option<PathBuf>, packs: &[Pack], user_dir: Option<PathBuf>) -> Self {
        Self {
            english,
            english_next: None,
            packs: packs.iter().map(Pack::paths).collect(),
            user_dir,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const FULL: &str = r#"{
      "format": 1, "id": "ml", "version": "2026.11.06", "name": "Malayalam",
      "nativeName": "മലയാളം", "script": "mlym", "romanised": {"name": "Manglish"},
      "minEngine": 1,
      "files": {"lexicon": "ml.fst", "romanisedLexicon": "manglish.fst", "nextWords": "next.fst",
                "translit": "translit.mlkbnn", "lm": "lm.mlkbnn"},
      "sha256": {"ml.fst": "00"},
      "credits": [{"name": "Malayalam Wikipedia", "licence": "CC BY-SA 4.0"}]
    }"#;

    fn pack_dir(manifest: &str, files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("manifest.json"), manifest).unwrap();
        for f in files {
            fs::write(dir.path().join(f), b"x").unwrap();
        }
        dir
    }

    #[test]
    fn opens_a_full_pack() {
        let dir = pack_dir(FULL, &["ml.fst", "manglish.fst", "next.fst", "translit.mlkbnn", "lm.mlkbnn"]);
        let pack = Pack::open(dir.path()).unwrap();
        assert_eq!(pack.manifest.id, "ml");
        assert_eq!(pack.manifest.native_name, "മലയാളം");
        assert_eq!(pack.manifest.romanised.as_ref().unwrap().name, "Manglish");
        assert_eq!(pack.manifest.credits.len(), 1);
        assert_eq!(pack.file("lm"), Some(dir.path().join("lm.mlkbnn")));
    }

    #[test]
    fn a_missing_file_or_role_is_none() {
        let dir = pack_dir(FULL, &["ml.fst"]);
        let pack = Pack::open(dir.path()).unwrap();
        assert_eq!(pack.file("translit"), None);
        assert_eq!(pack.file("emojiWords"), None);
    }

    #[test]
    fn file_names_cannot_leave_the_pack() {
        let manifest = FULL.replace("\"lm.mlkbnn\"", "\"../lm.mlkbnn\"");
        let dir = pack_dir(&manifest, &["ml.fst"]);
        let pack = Pack::open(dir.path()).unwrap();
        assert_eq!(pack.file("lm"), None);
    }

    #[test]
    fn refuses_what_it_cannot_read() {
        let cases = [
            (FULL.replace("\"format\": 1", "\"format\": 2"), "Format"),
            (FULL.replace("\"mlym\"", "\"taml\""), "Script"),
            (FULL.replace("\"minEngine\": 1", "\"minEngine\": 6"), "Engine"),
        ];
        for (manifest, want) in cases {
            let dir = pack_dir(&manifest, &["ml.fst"]);
            let err = Pack::open(dir.path()).unwrap_err();
            assert!(format!("{err:?}").starts_with(want), "{err:?}");
        }
        let dir = pack_dir(FULL, &[]);
        assert!(matches!(Pack::open(dir.path()), Err(PackError::NoLexicon)));
        let dir = pack_dir("{", &[]);
        assert!(matches!(Pack::open(dir.path()), Err(PackError::Malformed(_))));
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(Pack::open(dir.path()), Err(PackError::Unreadable(_))));
    }

    #[test]
    fn the_committed_packs_open() {
        let packs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs");
        for (id, script, min_engine) in [("ml", "mlym", 1), ("hi", "deva", 3), ("as", "beng", 2)] {
            let pack = Pack::open(&packs.join(format!("{id}.neopack"))).unwrap();
            assert_eq!(pack.manifest.id, id);
            assert_eq!(pack.manifest.script, script);
            assert_eq!(pack.manifest.min_engine, min_engine);
            assert!(pack.file("romanisation").is_some(), "{id}");
        }
    }

    #[test]
    fn paths_come_from_the_pack() {
        let dir = pack_dir(FULL, &["ml.fst", "manglish.fst", "next.fst", "translit.mlkbnn", "lm.mlkbnn"]);
        let pack = Pack::open(dir.path()).unwrap();
        let en = PathBuf::from("/x/en.fst");
        let user = PathBuf::from("/u");
        let paths = Paths::with_packs(Some(en.clone()), &[pack], Some(user.clone()));
        assert_eq!(paths.english, Some(en));
        assert_eq!(paths.user_dir, Some(user));
        let [ml] = &paths.packs[..] else { panic!("{paths:?}") };
        assert_eq!(ml.id.0, "ml");
        assert_eq!(ml.script, "mlym");
        assert_eq!(ml.lexicon, Some(dir.path().join("ml.fst")));
        assert_eq!(ml.romanised, Some(dir.path().join("manglish.fst")));
        assert_eq!(ml.romanisation, None);
        assert_eq!(ml.next_words, Some(dir.path().join("next.fst")));
        assert_eq!(ml.translit, Some(dir.path().join("translit.mlkbnn")));
        assert_eq!(ml.lm, Some(dir.path().join("lm.mlkbnn")));
    }
}
