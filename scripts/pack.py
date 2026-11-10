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

Standard library only.
"""

import argparse
import hashlib
import json
import sys
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
        problems = lang_problems(args.pack, manifest)
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
