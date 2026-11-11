#!/usr/bin/env python3
"""Language pack checksums and archives (M6 design, "Pack format").

    scripts/pack.py packs/ml.neopack --write      fill in manifest.json's sha256 for every listed file
    scripts/pack.py packs/ml.neopack --check      fail if any checksum is missing or stale
    scripts/pack.py packs/ml.neopack --zip OUT    write OUT (a .neopack zip of the folder)

Script rules (the `lang` role, engine 4). Every script's rules are data: a pack carries its
script's definition as `lang.json`, which is `data/langs/<id>/lang.json`, the file its lexicons
were built with. `--write` copies that file in and adds the role, for every pack that has one.
`--check` fails when the pack lacks the file though `data/langs/<id>/lang.json` exists, when the
file doesn't define the manifest's script, or when it differs from `data/langs/<id>/lang.json`.

What the file does to `minEngine`. The engine holds its own copy of the rules for mlym, deva
and beng (`EMBEDDED`: Malayalam's, Hindi's and Assamese's files), and engines before 5 had
those three scripts' rules in code, so a pack in one of them keeps its `minEngine`: an older
engine ignores the role and reads the pack as before. A pack in any other script needs the
file and `minEngine` 4, or the level that reads the newest field its file uses (`FIELDS`: 5
for the suffix and loose-spelling fields, 6 for `joiners` and `zwjForms`). A Devanagari or
Bengali pack that uses a newer field needs that level too.
`--write` raises it; `--check` fails when it is too low.

The romanisation tables (the `romanisation` role). `--check` fails when the file has no
`looseSpelling` (which typed spellings spell a word out; `{}` for none): a file without it is
read as the packs published before the field were, with Malayalam's habits (d for t, ee for i,
oo for u), which is right only for those packs.

Emoji words (the `emojiWords` and `emojiSearch` roles, optional). The words of the pack's
language that suggest an emoji in the bar and find one in the emoji panel's search
(`emoji-words.txt`, `emoji-search.txt`, built by data/pipeline/emoji_data.py into
`data/langs/<id>/`). `--write` copies them in and adds the roles when `data/langs/<id>/` has
them; `--check` fails when the pack lacks them or its copy differs. The engine ignores the
roles, and an app from before them uses its own bundled words, so `minEngine` doesn't move.

The layout (the `layout` role). `--check` also reads `layout.json` as the app does and fails on
what the app would refuse (not format 1, no pages, an empty key, a strip entry that isn't one
character), which otherwise shows only as a language that never appears on the phone; and on a
key, long-press option or strip sign that isn't in the pack's script (its block, from
`lang.json` or `EMBEDDED_BLOCKS`; ZWJ, ZWNJ and punctuation are fine), on a long-press
entry for a key no page has, and on a missing `spaceBar`, `languageKey` or page label. `--write`
prints the same problems without failing.

Standard library only.
"""

import argparse
import hashlib
import json
import sys
import unicodedata
import zipfile
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
# Scripts the engine has its own copy of the rules for (engine/core/src/script/mod.rs
# `EMBEDDED`, `script::supported`): a pack in one may lack the `lang` file.
EMBEDDED = {"mlym", "deva", "beng"}
# The engine level that reads the `lang` role (engine/core/src/pack.rs PACK_ENGINE).
LANG_ENGINE = 4
# lang.json fields by the engine level that reads them (engine/core/src/script/data.rs).
# Malayalam's file uses them all, and older engines have what they say in code, so mlym is not
# held to them.
FIELDS = {
    5: {"notLetter", "looseSigns", "finalPairs", "suffixJoins"},
    6: {"joiners", "zwjForms"},
}


def fields_engine(lang: dict | None) -> int:
    """The highest engine level a field of `lang` needs; 0 when it uses none of `FIELDS`. A
    `suffixJoins.nasal` ending of several code points (Tamil's ம்) is read from engine 6."""
    lang = lang or {}
    level = max((level for level, names in FIELDS.items() if names & set(lang)), default=0)
    joins = lang.get("suffixJoins")
    nasal = joins.get("nasal") if isinstance(joins, dict) else None
    if isinstance(nasal, list) and nasal and isinstance(nasal[0], str) and len(nasal[0].split()) > 1:
        level = max(level, 6)
    return level


def lang_source(manifest: dict) -> Path:
    return ROOT / "data" / "langs" / manifest["id"] / "lang.json"


# Optional pack files that are copies of data/langs/<id>/<name>: role -> file name.
COPIED_ROLES = {"emojiWords": "emoji-words.txt", "emojiSearch": "emoji-search.txt"}


def copied_source(manifest: dict, name: str) -> Path:
    return ROOT / "data" / "langs" / manifest["id"] / name


def add_copied(pack: Path, manifest: dict) -> None:
    """Each `COPIED_ROLES` file that data/langs/<id>/ has, copied into the pack with its role."""
    for role, name in COPIED_ROLES.items():
        source = copied_source(manifest, name)
        if source.is_file():
            (pack / manifest["files"].setdefault(role, name)).write_bytes(source.read_bytes())


def copied_problems(pack: Path, manifest: dict) -> list[str]:
    """The `COPIED_ROLES` files the pack lacks or holds another copy of, in words."""
    problems = []
    for role, name in COPIED_ROLES.items():
        source = copied_source(manifest, name)
        listed_as = manifest["files"].get(role)
        if not source.is_file():
            continue
        if listed_as is None:
            problems.append(f"the pack doesn't carry {source.relative_to(ROOT)} (the {role} role); run --write")
        elif not (pack / listed_as).is_file() or (pack / listed_as).read_bytes() != source.read_bytes():
            problems.append(f"{listed_as} differs from {source.relative_to(ROOT)}; run --write")
    return problems


def needed_engine(script: str, lang: dict | None) -> int:
    """The lowest `minEngine` the pack's script rules allow: 0 for a script older engines have
    in code, else the level that reads the `lang` role and the fields `lang` uses."""
    if script in EMBEDDED and (script == "mlym" or not fields_engine(lang)):
        return 0
    return max(fields_engine(lang), LANG_ENGINE)


def read_lang(path: Path) -> dict | None:
    try:
        lang = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError, UnicodeDecodeError):
        return None
    return lang if isinstance(lang, dict) else None


def add_lang(pack: Path, manifest: dict) -> None:
    """lang.json from data/langs, the role, and the minEngine its script needs. Nothing for a
    pack in an embedded script that has no data/langs file, unless the manifest names one."""
    source = lang_source(manifest)
    script = manifest["script"]
    wanted = source.is_file() or script not in EMBEDDED or "lang" in manifest["files"]
    if not wanted:
        return
    name = manifest["files"].setdefault("lang", "lang.json")
    if source.is_file():
        (pack / name).write_bytes(source.read_bytes())
    needed = needed_engine(script, read_lang(pack / name))
    if manifest.get("minEngine", 0) < needed:
        print(f"  minEngine {manifest.get('minEngine')} -> {needed}: script {script} "
              f"has its rules in lang.json")
        manifest["minEngine"] = needed


def lang_problems(pack: Path, manifest: dict) -> list[str]:
    """Why the pack's script rules wouldn't load, in words; empty when they would."""
    script = manifest["script"]
    name = manifest["files"].get("lang")
    source = lang_source(manifest)
    if name is None:
        if source.is_file():
            return [f"the pack doesn't carry {source.relative_to(ROOT)} (the lang role); run --write"]
        if script in EMBEDDED:
            return []
        return [f"the engine has no rules for script {script}: the pack needs its lang.json "
                f"(the lang role); run --write with {source}"]
    path = pack / name
    if not path.is_file():
        problems = []
        if manifest.get("minEngine", 0) < needed_engine(script, None):
            problems.append(f"minEngine must be at least {LANG_ENGINE} for script {script}, "
                            f"whose rules are in {name}")
        return problems + [f"{name} (the lang role) is not in the pack"]
    try:
        lang = json.loads(path.read_text(encoding="utf-8"))
    except (ValueError, UnicodeDecodeError) as e:
        return [f"{name}: not JSON: {e}"]
    if not isinstance(lang, dict):
        return [f"{name}: not a JSON object"]
    problems = []
    needed = needed_engine(script, lang)
    if manifest.get("minEngine", 0) < needed:
        uses = sorted(FIELDS.get(needed, set()) & set(lang))
        why = f" and uses {', '.join(uses)}" if uses else ""
        problems.append(f"minEngine must be at least {needed} for script {script}, "
                        f"whose rules are in {name}{why}")
    if lang.get("script") != script:
        problems.append(f"{name} defines script {lang.get('script')!r}, the manifest says {script}")
    if not lang.get("block") and not lang.get("letters"):
        problems.append(f"{name} has no characters: give block or letters")
    if source.is_file() and source.read_bytes() != path.read_bytes():
        problems.append(f"{name} differs from {source.relative_to(ROOT)}, which the lexicons were built "
                        f"with; run --write")
    return problems


def romanisation_problems(pack: Path, manifest: dict) -> list[str]:
    """What the pack's romanisation.json leaves to the engine's old defaults, in words."""
    name = manifest["files"].get("romanisation")
    if name is None or not (pack / name).is_file():
        return []
    try:
        tables = json.loads((pack / name).read_text(encoding="utf-8"))
    except (ValueError, UnicodeDecodeError) as e:
        return [f"{name}: not JSON: {e}"]
    if not isinstance(tables, dict):
        return [f"{name}: not a JSON object"]
    problems = []
    if not isinstance(tables.get("looseSpelling"), dict):
        problems.append(f"{name} has no looseSpelling: say which typed spellings spell a word out "
                        f"({{}} for none), or the pack is read with Malayalam's (d for t, ee for i, oo for u)")
    return problems


# The embedded scripts' blocks (`block` of data/langs/ml, hi and as/lang.json), for a pack in
# one of them that has no lang.json anywhere.
EMBEDDED_BLOCKS = {"mlym": (0x0D00, 0x0D7F), "deva": (0x0900, 0x097F), "beng": (0x0980, 0x09FF)}
JOINERS = "\u200c\u200d"


def script_ranges(pack: Path, manifest: dict) -> list[tuple[int, int]]:
    """The code point ranges of the pack's script: `block` and `letters` of its lang.json (the
    pack's copy, else data/langs/<id>/lang.json), else the embedded script's block. Empty when
    nothing says."""
    name = manifest["files"].get("lang")
    for path in ([pack / name] if name else []) + [lang_source(manifest)]:
        if not path.is_file():
            continue
        try:
            lang = json.loads(path.read_text(encoding="utf-8"))
            pairs = ([lang["block"]] if lang.get("block") else []) + list(lang.get("letters", []))
            ranges = [(int(a, 16), int(b, 16)) for a, b in pairs]
        except (ValueError, TypeError, UnicodeDecodeError):
            continue  # lang_problems reports a bad file
        if ranges:
            return ranges
    block = EMBEDDED_BLOCKS.get(manifest["script"])
    return [block] if block else []


def layout_problems(pack: Path, manifest: dict) -> list[str]:
    """What is wrong with the pack's layout.json, in words; empty when it is fine or the pack
    has no layout."""
    name = manifest["files"].get("layout")
    if name is None:
        return []
    path = pack / name
    if not path.is_file():
        return []  # listed but not written yet: --write names absent files
    try:
        layout = json.loads(path.read_text(encoding="utf-8"))
    except (ValueError, UnicodeDecodeError) as e:
        return [f"{name}: not JSON: {e}"]
    if not isinstance(layout, dict):
        return [f"{name}: not a JSON object"]
    problems = []
    if layout.get("format") != 1:
        problems.append(f"{name}: format is {layout.get('format')!r}; the app reads 1")
    for field in ("spaceBar", "languageKey"):
        if not isinstance(layout.get(field), str) or not layout[field]:
            problems.append(f"{name}: {field} is missing or empty")
    pages = layout.get("pages")
    if not isinstance(pages, list) or not pages:
        return problems + [f"{name}: no pages"]

    ranges = script_ranges(pack, manifest)

    def foreign(text: str) -> list[str]:
        """The characters of `text` that are neither the script's, a joiner nor punctuation."""
        return [
            f"{c} (U+{ord(c):04X})"
            for c in text
            if ranges
            and not any(a <= ord(c) <= b for a, b in ranges)
            and c not in JOINERS
            and not unicodedata.category(c).startswith("P")
        ]

    keys = set()
    for i, page in enumerate(pages):
        where = f"{name}: page {page.get('id', i) if isinstance(page, dict) else i}"
        if not isinstance(page, dict) or not isinstance(page.get("rows"), list) or not page["rows"]:
            problems.append(f"{where} has no rows")
            continue
        if not isinstance(page.get("id"), str) or not page["id"]:
            problems.append(f"{where} has no id")
        if not isinstance(page.get("label"), str) or not page["label"]:
            problems.append(f"{where} has no label (the text of the key that shows the page)")
        for row in page["rows"]:
            if not isinstance(row, list) or not row:
                problems.append(f"{where} has an empty row")
                continue
            for key in row:
                if not isinstance(key, str) or not key:
                    problems.append(f"{where} has an empty key")
                    continue
                keys.add(key)
                if foreign(key):
                    problems.append(f"{where}: key {key} has {', '.join(foreign(key))}, not of script {manifest['script']}")
    long_press = layout.get("longPress", {})
    if not isinstance(long_press, dict):
        problems.append(f"{name}: longPress is not an object of key to options")
        long_press = {}
    for key, options in long_press.items():
        if key not in keys:
            problems.append(f"{name}: longPress has {key}, which is on no page")
        if not isinstance(options, list) or not options or not all(isinstance(o, str) and o for o in options):
            problems.append(f"{name}: longPress {key} needs a list of non-empty options")
            continue
        for option in options:
            if foreign(option):
                problems.append(f"{name}: longPress option {option} has {', '.join(foreign(option))}, "
                                f"not of script {manifest['script']}")
    strip = layout.get("vowelSignStrip", [])
    if not isinstance(strip, list):
        problems.append(f"{name}: vowelSignStrip is not a list")
        strip = []
    for sign in strip:
        if not isinstance(sign, str) or len(sign) != 1:
            problems.append(f"{name}: vowelSignStrip entry {sign!r} is not one character")
        elif foreign(sign):
            problems.append(f"{name}: vowelSignStrip entry {foreign(sign)[0]} is not of script {manifest['script']}")
    return problems


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def listed(pack: Path, manifest: dict) -> dict[str, str]:
    """File name -> current checksum, for every listed file that is present."""
    out = {}
    for name in sorted(set(manifest["files"].values())):
        path = pack / name
        if path.is_file():
            out[name] = sha256(path)
    return out


def write_zip(pack: Path, names: list[str], out: Path) -> None:
    """OUT as a zip of the pack folder: manifest.json, then `names`, under `<pack>.neopack/`.

    The same files always give the same bytes (fixed dates and modes, sorted names), so a pack's
    sha256 in the published index (scripts/publish-packs.py) doesn't depend on file times.
    """
    out.parent.mkdir(parents=True, exist_ok=True)
    # Stored, not deflated: the files are already dense, and the keyboard never reads the zip.
    with zipfile.ZipFile(out, "w", compression=zipfile.ZIP_STORED) as z:
        for name in ["manifest.json", *names]:
            info = zipfile.ZipInfo(f"{pack.name}/{name}", date_time=(1980, 1, 1, 0, 0, 0))
            info.external_attr = 0o644 << 16
            info.compress_type = zipfile.ZIP_STORED
            with (pack / name).open("rb") as src, z.open(info, "w") as dst:
                for block in iter(lambda: src.read(1 << 20), b""):
                    dst.write(block)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("pack", type=Path)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--write", action="store_true")
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--zip", type=Path, metavar="OUT")
    args = parser.parse_args()

    manifest_path = args.pack / "manifest.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if args.write:
        add_lang(args.pack, manifest)
        add_copied(args.pack, manifest)
    current = listed(args.pack, manifest)

    if args.write:
        manifest["sha256"] = current
        manifest_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print(f"{manifest_path}: {len(current)} checksums")
        for problem in layout_problems(args.pack, manifest):
            print(f"  {problem}", file=sys.stderr)  # --check fails on these
        absent = sorted(set(manifest["files"].values()) - set(current))
        if absent:
            # Fine while a pack is being built (the transliterator comes last); a published pack
            # with an absent file loses that feature without a message.
            print(f"  listed but not in the folder yet: {', '.join(absent)}", file=sys.stderr)
        return 0

    if args.check:
        if manifest.get("sha256") != current:
            stale = sorted(n for n in set(current) | set(manifest.get("sha256", {}))
                           if manifest.get("sha256", {}).get(n) != current.get(n))
            print(f"{manifest_path}: checksums stale for {', '.join(stale)}; "
                  f"run scripts/pack.py {args.pack} --write", file=sys.stderr)
            return 1
        problems = (lang_problems(args.pack, manifest) + romanisation_problems(args.pack, manifest)
                    + copied_problems(args.pack, manifest) + layout_problems(args.pack, manifest))
        for problem in problems:
            print(f"{manifest_path}: {problem}", file=sys.stderr)
        return 1 if problems else 0

    if manifest.get("sha256") != current:
        print(f"{manifest_path}: checksums stale; run --write first", file=sys.stderr)
        return 1
    write_zip(args.pack, list(current), args.zip)
    print(f"{args.zip}: {args.zip.stat().st_size} bytes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
