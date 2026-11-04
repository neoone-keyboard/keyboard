/// Where the keyboard's word lists and models come from, and the software inside it.
/// The app's About screen lists these. `CreditsTests` checks that every
/// manifest in data/manifests/ is credited here with the same licence, and that
/// every crate in engine/shipped-crates.txt (generated from `cargo tree`) is
/// credited as software.
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
    public static let dataSources: [Credit] = [
        Credit(
            id: "enwiki", name: "English Wikipedia", licence: "CC BY-SA 4.0",
            licenceURL: "https://creativecommons.org/licenses/by-sa/4.0/",
            attribution: "Text of English Wikipedia articles by Wikipedia contributors, used for the word lists and to train the language model."
        ),
        Credit(
            id: "mlwiki", name: "Malayalam Wikipedia", licence: "CC BY-SA 4.0",
            licenceURL: "https://creativecommons.org/licenses/by-sa/4.0/",
            attribution: "Text of Malayalam Wikipedia articles by Wikipedia contributors, used for the word lists and to train the language model, also romanised as Manglish."
        ),
        Credit(
            id: "mlwikisource", name: "Malayalam Wikisource", licence: "CC BY-SA 4.0",
            licenceURL: "https://creativecommons.org/licenses/by-sa/4.0/",
            attribution: "Texts on Malayalam Wikisource, transcribed by Wikisource contributors. Most works are in the public domain; the site's text is CC BY-SA 4.0. Used for the word lists and to train the language model."
        ),
        Credit(
            id: "scowl", name: "SCOWL (Spell Checker Oriented Word Lists) 2020.12.07", licence: "SCOWL licence (MIT-like)",
            licenceURL: "http://wordlist.aspell.net/scowl-readme/",
            attribution: "Copyright 2000-2018 by Kevin Atkinson. Permission to use, copy, modify, distribute and sell these word lists, the associated scripts, the output created from the scripts, and its documentation for any purpose is hereby granted without fee, provided that the above copyright notice appears in all copies and that both that copyright notice and this permission notice appear in supporting documentation. Kevin Atkinson makes no representations about the suitability of this array for any purpose. It is provided \"as is\" without express or implied warranty. Alan Beale also deserves special credit (12Dicts, ENABLE)."
        ),
        Credit(
            id: "smc-words", name: "SMC Malayalam word lists", licence: "CC BY-SA 3.0",
            licenceURL: "https://creativecommons.org/licenses/by-sa/3.0/",
            attribution: "Word lists from the Malayalam Corpus by Swathanthra Malayalam Computing (SMC), prepared for the mlmorph project. Its list of English borrowings marks loanwords in the language model's Manglish text."
        ),
        Credit(
            id: "dakshina", name: "Dakshina dataset (Google Research)", licence: "CC BY-SA 4.0",
            licenceURL: "https://creativecommons.org/licenses/by-sa/4.0/",
            attribution: "Malayalam romanisations from the Dakshina dataset by Google Research (Roark et al., 2020), used to train the transliterator and to romanise the language model's Manglish text. https://github.com/google-research-datasets/dakshina"
        ),
        Credit(
            id: "aksharantar", name: "Aksharantar (AI4Bharat)", licence: "CC BY and CC0 1.0",
            licenceURL: "https://creativecommons.org/publicdomain/zero/1.0/",
            attribution: "Malayalam transliteration pairs from Aksharantar by AI4Bharat (Madhani et al., 2022), including names from Wikidata (CC0), used to train the transliterator and to find English loanwords for the language model. https://huggingface.co/datasets/ai4bharat/Aksharantar"
        ),
        Credit(
            id: "simplewiki", name: "Simple English Wikipedia", licence: "CC BY-SA 4.0",
            licenceURL: "https://creativecommons.org/licenses/by-sa/4.0/",
            attribution: "Text of Simple English Wikipedia articles by Wikipedia contributors, used to train the language model. https://simple.wikipedia.org/"
        ),
        Credit(
            id: "enwikinews", name: "English Wikinews", licence: "CC BY 2.5 and CC BY 4.0",
            licenceURL: "https://creativecommons.org/licenses/by/4.0/",
            attribution: "News articles from English Wikinews, by Wikinews contributors. This work may be attributed to 'Wikinews'. Used to train the language model. https://en.wikinews.org/"
        ),
        Credit(
            id: "gutenberg", name: "Project Gutenberg", licence: "Public domain",
            licenceURL: "https://www.gutenberg.org/",
            attribution: "Public-domain texts from Project Gutenberg, used to train the language model. https://www.gutenberg.org/"
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
