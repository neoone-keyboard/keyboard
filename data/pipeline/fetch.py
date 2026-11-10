"""Downloads every file listed in data/manifests/*.json into data/raw/<id>/
and verifies its checksum. Files already present and correct are skipped.
Runs on the Mac only; the app never touches the network."""

import argparse
import hashlib
import json
import http.client
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

CHUNK = 1 << 20
TIMEOUT = 60  # seconds per socket read
RETRIES = 3
BACKOFF = 2.0  # seconds; doubles each retry


def digest(path: Path, algorithm: str) -> str:
    h = hashlib.new(algorithm)
    with open(path, "rb") as f:
        while chunk := f.read(CHUNK):
            h.update(chunk)
    return h.hexdigest()


HEX_LENGTH = {"sha256": 64, "sha1": 40}


def expected(entry: dict) -> tuple[str, str] | None:
    """The entry's (algorithm, hex digest), or None when it has none or only a placeholder."""
    for algorithm, length in HEX_LENGTH.items():
        value = str(entry.get(algorithm) or "").lower()
        if len(value) == length and all(c in "0123456789abcdef" for c in value):
            return algorithm, value
    return None


def verified(path: Path, entry: dict) -> bool:
    want = expected(entry)
    return want is not None and path.exists() and digest(path, want[0]) == want[1]


class DownloadError(Exception):
    pass


def _attempt(url: str, partial: Path, timeout: float) -> None:
    """One request. Resumes from `partial` with a Range request when it exists."""
    offset = partial.stat().st_size if partial.exists() else 0
    headers = {"User-Agent": "ios-keyboard-lexicon-pipeline/1.0"}
    if offset:
        headers["Range"] = f"bytes={offset}-"
    with urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=timeout) as response:
        resumed = offset > 0 and response.status == 206  # a 200 means the server ignored Range
        with open(partial, "ab" if resumed else "wb") as out:
            while chunk := response.read(CHUNK):
                out.write(chunk)


def download(
    url: str, partial: Path, entry: dict, *, timeout: float = TIMEOUT, retries: int = RETRIES, backoff: float = BACKOFF
) -> None:
    """Downloads into `partial`, retrying with backoff and resuming where it stopped."""
    partial.parent.mkdir(parents=True, exist_ok=True)
    problem = "no attempt made"
    for attempt in range(retries + 1):
        try:
            _attempt(url, partial, timeout)
            return
        except urllib.error.HTTPError as e:
            problem = f"HTTP {e.code} {e.reason}"
            if e.code == 416:  # nothing left to send: complete, or a stale partial
                if verified(partial, entry):
                    return
                partial.unlink(missing_ok=True)
            elif e.code < 500 and e.code not in (408, 429):
                raise DownloadError(problem) from None
        except (OSError, http.client.HTTPException) as e:  # URLError, timeouts, dropped connections
            problem = str(getattr(e, "reason", None) or e)
        if attempt < retries:
            time.sleep(backoff * 2**attempt)
    raise DownloadError(f"{problem} (gave up after {retries + 1} attempts)")


def candidate_urls(url: str, mirrors: list[str]) -> list[str]:
    """The primary URL, then the same path on each mirror base URL, in order."""
    path = urllib.parse.urlsplit(url).path.lstrip("/")
    return [url] + [mirror.rstrip("/") + "/" + path for mirror in mirrors]


def fetch(
    manifest: dict, raw: Path, *, timeout: float = TIMEOUT, retries: int = RETRIES, backoff: float = BACKOFF
) -> list[Path]:
    paths = []
    for entry in manifest["files"]:
        path = raw / manifest["id"] / entry["path"]
        if not verified(path, entry):
            partial = path.with_name(path.name + ".part")
            failures = []
            for url in candidate_urls(entry["url"], manifest.get("mirrors", [])):
                print(f"downloading {url}", file=sys.stderr)
                try:
                    download(url, partial, entry, timeout=timeout, retries=retries, backoff=backoff)
                    break
                except DownloadError as e:
                    failures.append(f"{url}: {e}")
            else:
                raise SystemExit(f"could not download {path.name}:\n  " + "\n  ".join(failures))
            got = digest(partial, "sha256")
            want = expected(entry)
            if want is None:
                # A new manifest: keep the download, so the next run uses it, and say what to write.
                raise SystemExit(
                    f"{manifest['id']}: {entry['path']} has no sha256 in its manifest. The download is\n"
                    f"  {partial}\n"
                    f'Check it is the file you meant, then put "sha256": "{got}" in its "files" entry and run again.'
                )
            if not verified(partial, entry):
                partial.unlink(missing_ok=True)
                raise SystemExit(
                    f"checksum mismatch for {path.name}: the manifest's {want[0]} is {want[1]}, the download's "
                    f"sha256 is {got}. The source changed (update the manifest), or the manifest's checksum is wrong."
                )
            partial.replace(path)
        paths.append(path)
    return paths


def only_ids(values: list[str] | None) -> set[str] | None:
    """`--only a,b --only c` as {a, b, c}; None (fetch everything) when not given."""
    if values is None:
        return None
    return {part.strip() for value in values for part in value.split(",") if part.strip()}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifests", type=Path, required=True)
    parser.add_argument("--raw", type=Path, required=True)
    parser.add_argument(
        "--only", action="append", help="manifest ids to fetch, comma-separated or repeated (default: all)"
    )
    args = parser.parse_args(argv)
    only = only_ids(args.only)
    manifests = [json.loads(p.read_text(encoding="utf-8")) for p in sorted(args.manifests.glob("*.json"))]
    if only is not None:
        unknown = sorted(only - {m["id"] for m in manifests})
        if unknown:
            raise SystemExit(f'no manifest with id: {", ".join(unknown)} (an id is the "id" in {args.manifests}/*.json)')
    failed = []
    for manifest in manifests:
        if only is not None and manifest["id"] not in only:
            continue
        try:
            for fetched in fetch(manifest, args.raw):
                print(f"ok {fetched}", file=sys.stderr)
        except SystemExit as e:  # one bad source doesn't stop the rest
            print(e.code, file=sys.stderr)
            failed.append(manifest["id"])
    if failed:
        raise SystemExit(f"failed: {', '.join(failed)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
