#!/usr/bin/env python3
"""Language pack checksums and archives (M6 design, "Pack format").

    scripts/pack.py packs/ml.neopack --write      fill in manifest.json's sha256 for every listed file
    scripts/pack.py packs/ml.neopack --check      fail if any checksum is missing or stale
    scripts/pack.py packs/ml.neopack --zip OUT    write OUT (a .neopack zip of the folder)

Standard library only.
"""

import argparse
import hashlib
import json
import sys
import zipfile
from pathlib import Path


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
        return 0

    if manifest.get("sha256") != current:
        print(f"{manifest_path}: checksums stale; run --write first", file=sys.stderr)
        return 1
    write_zip(args.pack, list(current), args.zip)
    print(f"{args.zip}: {args.zip.stat().st_size} bytes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
