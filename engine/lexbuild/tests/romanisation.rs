//! lexbuild's romaniser agrees with `training/mlkb/data/table_romanise.py` on every row of the
//! fixture it wrote (`python -m mlkb.data.romanisation_fixture`), for every pack with one.

use std::fs;
use std::path::Path;

use lexbuild::romanisation::Table;

#[test]
fn matches_the_python_romaniser() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for lang in ["hi"] {
        let table =
            Table::load(&root.join(format!("../../packs/{lang}.neopack/romanisation.json")))
                .unwrap();
        let fixture =
            fs::read_to_string(root.join(format!("tests/fixtures/{lang}-romanisation.tsv")))
                .unwrap();
        let mut rows = 0;
        for line in fixture.lines() {
            let cols: Vec<&str> = line.split('\t').collect();
            let [native, canonical, skeleton, typed, typed_skeleton] = cols[..] else {
                panic!("bad row {line:?}");
            };
            assert_eq!(table.canonical(native), canonical, "{native}");
            assert_eq!(table.word_skeleton(native), skeleton, "{native}");
            assert_eq!(table.skeleton(typed), typed_skeleton, "{typed}");
            rows += 1;
        }
        assert!(rows > 100, "{lang}: {rows} rows");
    }
}
