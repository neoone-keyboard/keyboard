#!/usr/bin/env python3
"""Language pack checksums and archives (M6 design, "Pack format").

    scripts/pack.py packs/ml.neopack --write      fill in manifest.json's sha256 for every listed file
    scripts/pack.py packs/ml.neopack --check      fail if any checksum is missing or stale
    scripts/pack.py packs/ml.neopack --zip OUT    write OUT (a .neopack zip of the folder)

Script rules (the `lang` role, engine 4). The engine has coded rules for mlym, deva and beng
(`CODED`). A pack in any other script carries its script's definition as `lang.json`, which is
`data/langs/<id>/lang.json`, the file its lexicons were built with. `--write` copies that file
in, adds the role and raises `minEngine` to 4. `--check` fails when a pack in another script
lacks the file or `minEngine` 4, when the file doesn't define the manifest's script, or when it
differs from `data/langs/<id>/lang.json`. A pack in a coded script may carry the file too and
keep a lower `minEngine` (older engines ignore the role).

The layout (the `layout` role). `--check` also reads `layout.json` as the app does and fails on
what the app would refuse (not format 1, no pages, an empty key, a strip entry that isn't one
character), which otherwise shows only as a language that never appears on the phone; and on a
key, long-press option or strip sign that isn't in the pack's script (its block, from
`lang.json` or the coded scripts' blocks; ZWJ, ZWNJ and punctuation are fine), on a long-press
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
# Scripts with coded rules in the engine (engine/core/src/script, `script::supported`).
CODED = {"mlym", "deva", "beng"}
# The engine level that reads the `lang` role (engine/core/src/pack.rs PACK_ENGINE).
LANG_ENGINE = 4


def lang_source(manifest: dict) -> Path:
    return ROOT / "data" / "langs" / manifest["id"] / "lang.json"


def add_lang(pack: Path, manifest: dict) -> None:
    """For a pack in a script without coded rules: lang.json from data/langs, the role, and
    minEngine 4. Nothing for a coded script, unless the manifest already names the file."""
    source = lang_source(manifest)
    wanted = manifest["script"] not in CODED or "lang" in manifest["files"]
    if not wanted:
        return
    name = manifest["files"].setdefault("lang", "lang.json")
    if source.is_file():
        (pack / name).write_bytes(source.read_bytes())
    if manifest["script"] not in CODED and manifest.get("minEngine", 0) < LANG_ENGINE:
        print(f"  minEngine {manifest.get('minEngine')} -> {LANG_ENGINE}: script {manifest['script']} "
              f"has its rules in lang.json")
        manifest["minEngine"] = LANG_ENGINE


def lang_problems(pack: Path, manifest: dict) -> list[str]:
    """Why the pack's script rules wouldn't load, in words; empty when they would."""
    script = manifest["script"]
    name = manifest["files"].get("lang")
    if name is None:
        if script in CODED:
            return []
        return [f"script {script} has no coded rules: the pack needs its lang.json "
                f"(the lang role); run --write with {lang_source(manifest)}"]
    problems = []
    if script not in CODED and manifest.get("minEngine", 0) < LANG_ENGINE:
        problems.append(f"minEngine must be at least {LANG_ENGINE} for script {script}, whose rules are in {name}")
    path = pack / name
    if not path.is_file():
        return problems + [f"{name} (the lang role) is not in the pack"]
    try:
        lang = json.loads(path.read_text(encoding="utf-8"))
    except (ValueError, UnicodeDecodeError) as e:
        return problems + [f"{name}: not JSON: {e}"]
    if lang.get("script") != script:
        problems.append(f"{name} defines script {lang.get('script')!r}, the manifest says {script}")
    if not lang.get("block") and not lang.get("letters"):
        problems.append(f"{name} has no characters: give block or letters")
    source = lang_source(manifest)
    if source.is_file() and source.read_bytes() != path.read_bytes():
        problems.append(f"{name} differs from {source.relative_to(ROOT)}, which the lexicons were built "
                        f"with; run --write")
    return problems


# The coded scripts' blocks (engine/core/src/script: mlym.rs, deva.rs, beng.rs).
CODED_BLOCKS = {"mlym": (0x0D00, 0x0D7F), "deva": (0x0900, 0x097F), "beng": (0x0980, 0x09FF)}
JOINERS = "\u200c\u200d"


def script_ranges(pack: Path, manifest: dict) -> list[tuple[int, int]]:
    """The code point ranges of the pack's script: `block` and `letters` of its lang.json (the
    pack's copy, else data/langs/<id>/lang.json), else the coded script's block. Empty when
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
    block = CODED_BLOCKS.get(manifest["script"])
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
        problems = lang_problems(args.pack, manifest) + layout_problems(args.pack, manifest)
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
