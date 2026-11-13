# NeoOne Keyboard

NeoOne Keyboard is an offline phonetic keyboard for iOS. Type any language the way it
sounds, in English letters, and get it in its own script: `mera` gives मेरा in Hindi, `ente`
gives എന്റെ in Malayalam. Hindi, Malayalam, Tamil and Assamese are examples; a language is a
data pack, and anyone can build one.

Everything runs on the phone. The keyboard does not ask for Full Access, so it has no
network access, and nothing typed leaves the phone.

## Features

- Phonetic typing: type in English letters and pick the native-script word from the
  suggestion bar.
- Suggestions, autocorrect and next-word prediction in English and in each added language.
- A native layout for each language, for typing the script directly.
- Up to four languages on at once, English included; the suggestion bar works across them.
- Swipe typing.
- Emoji, with search, and emoji suggestions while typing.
- Learns the words you use. They stay on the phone and can be viewed and forgotten.
- Language packs: download them in the app, or build your own and import it from Files.

## Repository layout

| Path | Contents |
|---|---|
| `engine/` | The Rust engine: suggestions, correction, transliteration, swipe, the user's learned words. `engine/core` is the library, `engine/ffi` its Swift bindings, `engine/lexbuild` the lexicon builder. |
| `apps/ios/` | The iOS app and keyboard extension (Swift), and the `KeyboardCore` and `MLEngine` Swift packages. |
| `packs/` | Language packs, one `<id>.neopack` folder per language: manifest, lexicons, models, layout. |
| `training/` | Python code that trains a pack's transliteration and language models. |
| `data/` | Per-language rules (`data/langs/`), word lists, evaluation sets, recorded suggestion bars (`data/golden/`) and the scripts that fetch and build text. Downloaded text goes to `data/raw/`, which is not tracked. |
| `site-src/` | Source of the website (https://neoone.jishnu.me); `site/` is the site as published. |
| `hosting/` | The Cloudflare Worker that serves the site and the packs. |
| `docs/` | The guide to making a language (`docs/make-a-language.md`) and its known limits (`docs/known-limits.md`), the design of the pack and romanisation formats (`docs/design/`), and the maintainer's App Store notes (`docs/maintainers/`). |
| `scripts/` | Build and check scripts: the xcframework build, pack checks, pack publishing. |

## Build and test

Requirements:

- macOS with Xcode (the app targets iOS 26).
- Rust through [rustup](https://rustup.rs). The toolchain version and targets are pinned in
  `engine/rust-toolchain.toml` and installed by rustup on first use.
- [xcodegen](https://github.com/yonaskolb/XcodeGen): `brew install xcodegen`.
- For the data and training checks: Python 3 and [uv](https://docs.astral.sh/uv/).

Build:

```sh
make engine     # build the Rust engine as an xcframework, with its Swift bindings
make project    # generate apps/ios/MLKeyboard.xcodeproj with xcodegen
make build      # build the app for the iOS Simulator (SIM="iPhone 17" by default)
```

`make build` does the first two steps as well, and builds unsigned for the Simulator.

To run on a device, sign with your own Apple developer team and identifiers. The defaults in
`apps/ios/Signing.xcconfig` are the published app's (`me.jishnu.keyboard`, App Group
`group.me.jishnu.keyboard`). Don't edit that file: create `apps/ios/Signing.local.xcconfig`
beside it (git-ignored) with your own values:

```
DEVELOPMENT_TEAM = ABCDE12345
BUNDLE_ID_PREFIX = com.example.neoone
```

Every identifier follows from these two: the app is `BUNDLE_ID_PREFIX`, the keyboard
`BUNDLE_ID_PREFIX.extension`, the App Group `group.BUNDLE_ID_PREFIX` (set `APP_GROUP` to use
another), and the `.neopack` file type `BUNDLE_ID_PREFIX.neopack`. The project, the app and
the keyboard, and the Makefile all read them from there, so nothing else needs editing. The
Info.plist and `.entitlements` files are written by `make project` and aren't tracked.

Then name the device, on the command line or in an untracked `local.mk`
(`DEVICE := <your-device-id>`):

```sh
xcrun devicectl list devices
make device DEVICE=<your-device-id>
```

Test:

```sh
make test-engine         # the engine's tests, and clippy
make test-swift          # the KeyboardCore and MLEngine Swift package tests
make golden-check        # every suggestion over the evaluation inputs is as recorded
make golden-check-packs  # the same for each language pack
make test                # the engine, data, training and Swift tests, the evaluation, and golden-check-packs
```

`make golden-check` prints `golden: identical` and `golden ml-only: identical` when no
suggestion changed. After a change that is meant to change suggestions, record them again
with `make golden-save` (and `make golden-save-packs`).

## Make a language pack

[docs/make-a-language.md](docs/make-a-language.md) walks through building a pack from openly
licensed text with the tools in this repository, importing it on a phone, and contributing
it. No step needs a change to the engine or the app. `make test-pack PACK_LANG=<id>` runs
every check of one pack.

## Licence

The source code is licensed under the Apache License, Version 2.0; see [LICENSE](LICENSE).

Data written for this project (the script rules and hand-written romanisation tables in
`data/langs/`, the packs' layouts, chat forms and suffixes, `data/indian-english.txt` and the
hand-written eval sets) is licensed under the Apache License 2.0 as well. Where such a file
ships inside a language pack, it is also offered under that pack's CC BY-SA 4.0.

Other data is not covered by the Apache licence. The language packs, the word lists and the
trained models are built from third-party sources and keep those sources' licences: each pack
as a whole is CC BY-SA 4.0, except the Japanese pack's dictionary files (see
[packs/README.md](packs/README.md)). See [NOTICE](NOTICE) for the list of third-party data in
this repository, the `credits` in each pack's `manifest.json`, and
https://neoone.jishnu.me/credits.

## Issues and support

Report bugs and ask for features in the GitHub issues of
[neoone-keyboard/keyboard](https://github.com/neoone-keyboard/keyboard/issues). Help with
using the app is at https://neoone.jishnu.me/support.
