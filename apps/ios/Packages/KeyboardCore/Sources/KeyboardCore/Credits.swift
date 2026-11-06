/// Where the keyboard's word lists and models come from, and the software inside it.
/// The app's About screen lists these: English's own sources here, each pack's from its
/// manifest. `CreditsTests` checks that every shipped manifest in data/manifests/ is
/// credited with the same licence (with the bundled packs), and that every crate in
/// engine/shipped-crates.txt (generated from `cargo tree`) is credited as software.
public struct Credit: Equatable, Sendable, Identifiable {
    /// The manifest id for data sources (data/manifests/<id>.json); the crate name for software.
    public let id: String
    public let name: String
    public let licence: String
    public let licenceURL: String
    public let attribution: String

    public init(id: String, name: String, licence: String, licenceURL: String, attribution: String) {
        self.id = id
        self.name = name
        self.licence = licence
        self.licenceURL = licenceURL
        self.attribution = attribution
    }
}

public enum Credits {
    /// The base's own sources: English's word lists and the emoji data. Each pack's are in its
    /// manifest (`PackDescriptor.credits`).
    public static let english: [Credit] = [
        Credit(
            id: "enwiki", name: "English Wikipedia", licence: "CC BY-SA 4.0",
            licenceURL: "https://creativecommons.org/licenses/by-sa/4.0/",
            attribution: "Text of English Wikipedia articles by Wikipedia contributors, used for the word lists and to train the language model."
        ),
        Credit(
            id: "scowl", name: "SCOWL (Spell Checker Oriented Word Lists) 2020.12.07", licence: "SCOWL licence (MIT-like)",
            licenceURL: "http://wordlist.aspell.net/scowl-readme/",
            attribution: "Copyright 2000-2018 by Kevin Atkinson. Permission to use, copy, modify, distribute and sell these word lists, the associated scripts, the output created from the scripts, and its documentation for any purpose is hereby granted without fee, provided that the above copyright notice appears in all copies and that both that copyright notice and this permission notice appear in supporting documentation. Kevin Atkinson makes no representations about the suitability of this array for any purpose. It is provided \"as is\" without express or implied warranty. Alan Beale also deserves special credit (12Dicts, ENABLE)."
        ),
        Credit(
            id: "unicode-emoji", name: "Unicode Emoji 18.0", licence: "Unicode License v3",
            licenceURL: "https://www.unicode.org/license.txt",
            attribution: "Copyright © 1991-2026 Unicode, Inc. The emoji panel's emoji, order, groups and skin tones, from the Unicode emoji data file emoji-test.txt. https://www.unicode.org/emoji/"
        ),
        Credit(
            id: "cldr-annotations", name: "Unicode CLDR 49 emoji annotations", licence: "Unicode License v3",
            licenceURL: "https://www.unicode.org/license.txt",
            attribution: "Copyright © 1991-2026 Unicode, Inc. English and Malayalam emoji keywords and names from the Unicode Common Locale Data Repository, used for emoji suggestions. https://cldr.unicode.org/"
        ),
    ]

    /// Every data source of the keyboard with `packs` loaded: English's own, then each pack's
    /// in manifest order, each source once (the first credit for an id wins).
    public static func dataSources(packs: [PackDescriptor]) -> [Credit] {
        var seen = Set<String>()
        return (english + packs.flatMap(\.credits)).filter { seen.insert($0.id).inserted }
    }

    /// Open-source Rust crates linked into the keyboard's engine. The link is each crate's
    /// repository. UniFFI is MPL-2.0, which asks for a source notice; the others are MIT,
    /// Apache-2.0 or similar permissive licences.
    public static let software: [Credit] = [
        Credit(id: "anyhow", name: "anyhow", licence: "MIT OR Apache-2.0",
               licenceURL: "https://github.com/dtolnay/anyhow",
               attribution: "David Tolnay"),
        Credit(id: "bitflags", name: "bitflags", licence: "MIT OR Apache-2.0",
               licenceURL: "https://github.com/bitflags/bitflags",
               attribution: "The Rust Project Developers"),
        Credit(id: "bytes", name: "bytes", licence: "MIT",
               licenceURL: "https://github.com/tokio-rs/bytes",
               attribution: "Carl Lerche, Sean McArthur"),
        Credit(id: "camino", name: "camino", licence: "MIT OR Apache-2.0",
               licenceURL: "https://github.com/camino-rs/camino",
               attribution: "Without Boats, Ashley Williams, Steve Klabnik and others"),
        Credit(id: "cargo-platform", name: "cargo-platform", licence: "MIT OR Apache-2.0",
               licenceURL: "https://github.com/rust-lang/cargo",
               attribution: "The cargo-platform developers"),
        Credit(id: "cargo_metadata", name: "cargo_metadata", licence: "MIT",
               licenceURL: "https://github.com/oli-obk/cargo_metadata",
               attribution: "Oliver Schneider"),
        Credit(id: "cfg-if", name: "cfg-if", licence: "MIT OR Apache-2.0",
               licenceURL: "https://github.com/rust-lang/cfg-if",
               attribution: "Alex Crichton"),
        Credit(id: "equivalent", name: "equivalent", licence: "Apache-2.0 OR MIT",
               licenceURL: "https://github.com/indexmap-rs/equivalent",
               attribution: "The equivalent developers"),
        Credit(id: "errno", name: "errno", licence: "MIT OR Apache-2.0",
               licenceURL: "https://github.com/lambda-fairy/rust-errno",
               attribution: "Chris Wong, Dan Gohman"),
        Credit(id: "fastrand", name: "fastrand", licence: "Apache-2.0 OR MIT",
               licenceURL: "https://github.com/smol-rs/fastrand",
               attribution: "Stjepan Glavina"),
        Credit(id: "fs-err", name: "fs-err", licence: "MIT/Apache-2.0",
               licenceURL: "https://github.com/andrewhickman/fs-err",
               attribution: "Andrew Hickman"),
        Credit(id: "fst", name: "fst", licence: "Unlicense/MIT",
               licenceURL: "https://github.com/BurntSushi/fst",
               attribution: "Andrew Gallant"),
        Credit(id: "getrandom", name: "getrandom", licence: "MIT OR Apache-2.0",
               licenceURL: "https://github.com/rust-random/getrandom",
               attribution: "The Rand Project Developers"),
        Credit(id: "hashbrown", name: "hashbrown", licence: "MIT OR Apache-2.0",
               licenceURL: "https://github.com/rust-lang/hashbrown",
               attribution: "The hashbrown developers"),
        Credit(id: "heck", name: "heck", licence: "MIT OR Apache-2.0",
               licenceURL: "https://github.com/withoutboats/heck",
               attribution: "The heck developers"),
        Credit(id: "indexmap", name: "indexmap", licence: "Apache-2.0 OR MIT",
               licenceURL: "https://github.com/indexmap-rs/indexmap",
               attribution: "The indexmap developers"),
        Credit(id: "itoa", name: "itoa", licence: "MIT OR Apache-2.0",
               licenceURL: "https://github.com/dtolnay/itoa",
               attribution: "David Tolnay"),
        Credit(id: "libc", name: "libc", licence: "MIT OR Apache-2.0",
               licenceURL: "https://github.com/rust-lang/libc",
               attribution: "The libc developers"),
        Credit(id: "memchr", name: "memchr", licence: "Unlicense OR MIT",
               licenceURL: "https://github.com/BurntSushi/memchr",
               attribution: "Andrew Gallant, bluss"),
        Credit(id: "memmap2", name: "memmap2", licence: "MIT OR Apache-2.0",
               licenceURL: "https://github.com/RazrFalcon/memmap2-rs",
               attribution: "Dan Burkert, Yevhenii Reizner, The Contributors"),
        Credit(id: "once_cell", name: "once_cell", licence: "MIT OR Apache-2.0",
               licenceURL: "https://github.com/matklad/once_cell",
               attribution: "Aleksey Kladov"),
        Credit(id: "proc-macro2", name: "proc-macro2", licence: "MIT OR Apache-2.0",
               licenceURL: "https://github.com/dtolnay/proc-macro2",
               attribution: "David Tolnay, Alex Crichton"),
        Credit(id: "quote", name: "quote", licence: "MIT OR Apache-2.0",
               licenceURL: "https://github.com/dtolnay/quote",
               attribution: "David Tolnay"),
        Credit(id: "rustix", name: "rustix", licence: "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT",
               licenceURL: "https://github.com/bytecodealliance/rustix",
               attribution: "Dan Gohman, Jakub Konka"),
        Credit(id: "semver", name: "semver", licence: "MIT OR Apache-2.0",
               licenceURL: "https://github.com/dtolnay/semver",
               attribution: "David Tolnay"),
        Credit(id: "serde", name: "serde", licence: "MIT OR Apache-2.0",
               licenceURL: "https://github.com/serde-rs/serde",
               attribution: "Erick Tryzelaar, David Tolnay"),
        Credit(id: "serde_core", name: "serde_core", licence: "MIT OR Apache-2.0",
               licenceURL: "https://github.com/serde-rs/serde",
               attribution: "Erick Tryzelaar, David Tolnay"),
        Credit(id: "serde_derive", name: "serde_derive", licence: "MIT OR Apache-2.0",
               licenceURL: "https://github.com/serde-rs/serde",
               attribution: "Erick Tryzelaar, David Tolnay"),
        Credit(id: "serde_json", name: "serde_json", licence: "MIT OR Apache-2.0",
               licenceURL: "https://github.com/serde-rs/json",
               attribution: "The serde_json developers"),
        Credit(id: "siphasher", name: "siphasher", licence: "MIT/Apache-2.0",
               licenceURL: "https://github.com/jedisct1/rust-siphash",
               attribution: "Frank Denis"),
        Credit(id: "static_assertions", name: "static_assertions", licence: "MIT OR Apache-2.0",
               licenceURL: "https://github.com/nvzqz/static-assertions-rs",
               attribution: "Nikolai Vazquez"),
        Credit(id: "syn", name: "syn", licence: "MIT OR Apache-2.0",
               licenceURL: "https://github.com/dtolnay/syn",
               attribution: "David Tolnay"),
        Credit(id: "tempfile", name: "tempfile", licence: "MIT OR Apache-2.0",
               licenceURL: "https://github.com/Stebalien/tempfile",
               attribution: "Steven Allen, The Rust Project Developers, Ashley Mannix and others"),
        Credit(id: "thiserror", name: "thiserror", licence: "MIT OR Apache-2.0",
               licenceURL: "https://github.com/dtolnay/thiserror",
               attribution: "David Tolnay"),
        Credit(id: "thiserror-impl", name: "thiserror-impl", licence: "MIT OR Apache-2.0",
               licenceURL: "https://github.com/dtolnay/thiserror",
               attribution: "David Tolnay"),
        Credit(id: "tinyvec", name: "tinyvec", licence: "Zlib OR Apache-2.0 OR MIT",
               licenceURL: "https://github.com/Lokathor/tinyvec",
               attribution: "Lokathor"),
        Credit(id: "toml", name: "toml", licence: "MIT/Apache-2.0",
               licenceURL: "https://github.com/toml-rs/toml",
               attribution: "Alex Crichton"),
        Credit(id: "unicode-ident", name: "unicode-ident", licence: "(MIT OR Apache-2.0) AND Unicode-3.0",
               licenceURL: "https://github.com/dtolnay/unicode-ident",
               attribution: "David Tolnay"),
        Credit(id: "unicode-normalization", name: "unicode-normalization", licence: "(MIT OR Apache-2.0) AND Unicode-3.0",
               licenceURL: "https://github.com/unicode-rs/unicode-normalization",
               attribution: "kwantam, Manish Goregaokar"),
        Credit(id: "unicode-segmentation", name: "unicode-segmentation", licence: "MIT OR Apache-2.0",
               licenceURL: "https://github.com/unicode-rs/unicode-segmentation",
               attribution: "kwantam, Manish Goregaokar"),
        Credit(id: "uniffi", name: "uniffi", licence: "MPL-2.0",
               licenceURL: "https://github.com/mozilla/uniffi-rs",
               attribution: "Mozilla. Source code: https://github.com/mozilla/uniffi-rs"),
        Credit(id: "uniffi_core", name: "uniffi_core", licence: "MPL-2.0",
               licenceURL: "https://github.com/mozilla/uniffi-rs",
               attribution: "Mozilla. Source code: https://github.com/mozilla/uniffi-rs"),
        Credit(id: "uniffi_internal_macros", name: "uniffi_internal_macros", licence: "MPL-2.0",
               licenceURL: "https://github.com/mozilla/uniffi-rs",
               attribution: "Mozilla. Source code: https://github.com/mozilla/uniffi-rs"),
        Credit(id: "uniffi_macros", name: "uniffi_macros", licence: "MPL-2.0",
               licenceURL: "https://github.com/mozilla/uniffi-rs",
               attribution: "Mozilla. Source code: https://github.com/mozilla/uniffi-rs"),
        Credit(id: "uniffi_meta", name: "uniffi_meta", licence: "MPL-2.0",
               licenceURL: "https://github.com/mozilla/uniffi-rs",
               attribution: "Mozilla. Source code: https://github.com/mozilla/uniffi-rs"),
        Credit(id: "uniffi_pipeline", name: "uniffi_pipeline", licence: "MPL-2.0",
               licenceURL: "https://github.com/mozilla/uniffi-rs",
               attribution: "Mozilla. Source code: https://github.com/mozilla/uniffi-rs"),
        Credit(id: "zmij", name: "zmij", licence: "MIT",
               licenceURL: "https://github.com/dtolnay/zmij",
               attribution: "David Tolnay"),
    ]
}
