"""Downloads every file listed in data/manifests/*.json into data/raw/<id>/
and verifies its checksum. Files already present and correct are skipped.
Runs on the Mac only; the app never touches the network."""

import argparse
import hashlib
import json
import sys
import urllib.request
from pathlib import Path

CHUNK = 1 << 20


def digest(path: Path, algorithm: str) -> str:
    h = hashlib.new(algorithm)
    with open(path, "rb") as f:
        while chunk := f.read(CHUNK):
            h.update(chunk)
    return h.hexdigest()


def expected(entry: dict) -> tuple[str, str]:
    for algorithm in ("sha256", "sha1"):
        if entry.get(algorithm):
            return algorithm, entry[algorithm].lower()
    raise ValueError(f"{entry['url']}: manifest gives no sha256 or sha1")


def verified(path: Path, entry: dict) -> bool:
    algorithm, want = expected(entry)
    return path.exists() and digest(path, algorithm) == want


def download(url: str, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    partial = path.with_suffix(path.suffix + ".part")
    request = urllib.request.Request(url, headers={"User-Agent": "ios-keyboard-lexicon-pipeline/1.0"})
    with urllib.request.urlopen(request) as response, open(partial, "wb") as out:
        while chunk := response.read(CHUNK):
            out.write(chunk)
    partial.replace(path)


def fetch(manifest: dict, raw: Path) -> list[Path]:
    paths = []
    for entry in manifest["files"]:
        path = raw / manifest["id"] / entry["path"]
        if not verified(path, entry):
            print(f"downloading {entry['url']}", file=sys.stderr)
            download(entry["url"], path)
            if not verified(path, entry):
                raise SystemExit(f"checksum mismatch for {path}: the source changed; update the manifest")
        paths.append(path)
    return paths


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifests", type=Path, required=True)
    parser.add_argument("--raw", type=Path, required=True)
    parser.add_argument("--only", action="append", help="manifest ids to fetch (default: all)")
    args = parser.parse_args(argv)
    for path in sorted(args.manifests.glob("*.json")):
        manifest = json.loads(path.read_text(encoding="utf-8"))
        if args.only and manifest["id"] not in args.only:
            continue
        for fetched in fetch(manifest, args.raw):
            print(f"ok {fetched}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
