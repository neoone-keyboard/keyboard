//! The engine level a pack needs, and why: `make pack-needs PACK_LANG=xx`, or
//! `cargo run -q -p mlengine-core --example pack_needs -- <pack folder>...`.
//! Prints, for each pack, its `minEngine`, the level its contents need (`Pack::needs`) and
//! every reason above 1; exits 1 when a pack's `minEngine` is too low.

use std::path::Path;
use std::process::ExitCode;

use mlengine_core::pack::Pack;

fn main() -> ExitCode {
    let mut ok = true;
    for arg in std::env::args().skip(1) {
        let pack = match Pack::open(Path::new(&arg)) {
            Ok(pack) => pack,
            Err(e) => {
                eprintln!("{arg}: {e}");
                ok = false;
                continue;
            }
        };
        let id = &pack.manifest.id;
        let declared = pack.manifest.min_engine;
        match pack.needs() {
            Ok(needs) => {
                let level = needs.first().map_or(1, |n| n.level);
                let verdict = if declared >= level { "ok" } else { "TOO LOW" };
                println!("{id}: minEngine {declared}, needs {level}: {verdict}");
                for need in needs {
                    println!("  {need}");
                }
                ok &= declared >= level;
            }
            Err(e) => {
                eprintln!("{id}: {e}");
                ok = false;
            }
        }
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
