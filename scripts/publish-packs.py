#!/usr/bin/env python3
"""The download site and its pack index (M6 design, "Where packs live", hosting).

    python3 -I scripts/publish-packs.py           build the packs, write site/ and the app's index URL
    python3 -I scripts/publish-packs.py --check   fail if site/ or the xcconfig is out of date
    python3 -I scripts/publish-packs.py --deploy  upload missing pack files to R2, deploy the site

The site is a Cloudflare Worker (hosting/wrangler.jsonc, hosting/worker.js): static assets from
site/, and the pack files from the R2 bucket, which the Worker serves at /packs/<file>. Its
address, the R2 bucket, where the pack files are downloaded from and the GitHub repository (for
issues) are in one place, site-src/site.json. From it and the pack folders in packs/ this writes:

- build/site-packs/<id>-<version>.neopack: each pack's zip (scripts/pack.py, byte-for-byte
  reproducible), and english-base-<sha256 prefix>.zip, the keyboard's built-in English word
  lists. They are R2 objects packs/<file>, not static assets (too large: 25 MiB at most).
- site/: the Worker's static assets. index.html, privacy.html, support.html and credits.html
  from site-src/ (credits from the pack manifests and data/manifests), style.css, _headers
  (packs/index.json cached for 5 minutes), and packs/index.json, the pack index the app
  downloads from (KeyboardCore.PackIndex, format 1), whose entries point at the pack files.
- apps/ios/PackIndex.xcconfig: ML_PACK_INDEX_URL, the app's Info.plist NeoPackIndexURL.

Then it prints the `wrangler r2 object put` commands that upload the pack files. It runs them
only with --deploy (`make site-deploy`), which uploads the files the site doesn't serve yet and
then runs `wrangler deploy`. A file's name carries its version, so a published file never
changes: a pack whose version is already in site/packs/index.json with other bytes is refused,
raise the manifest's `version` first.

Standard library only.
"""

import argparse
import hashlib
import html
import importlib.util
import json
import shlex
import subprocess
import sys
import urllib.error
import urllib.request
import tempfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SITE_SRC = ROOT / "site-src"
SITE = ROOT / "site"
PACKS = ROOT / "packs"
BUILD = ROOT / "build" / "site-packs"
XCCONFIG = ROOT / "apps" / "ios" / "PackIndex.xcconfig"
WRANGLER_CONFIG = ROOT / "hosting" / "wrangler.jsonc"
WRANGLER = ["npx", "-y", "wrangler@4"]
R2_PREFIX = "packs/"  # hosting/worker.js serves R2 key packs/<file> at /packs/<file>
INDEX_CACHE = "public, max-age=300"
LEXICON = ROOT / "apps" / "ios" / "KeyboardExtension" / "Lexicon"
DATA_MANIFESTS = ROOT / "data" / "manifests"

INDEX_FORMAT = 1  # KeyboardCore PackIndex.format
# English's own sources, as KeyboardCore's Credits.english lists them (the app's About screen).
ENGLISH_CREDIT_IDS = ["enwiki", "scowl", "unicode-emoji", "cldr-annotations"]
ENGLISH_FILES = ["en.fst", "en-next.fst"]
PRIVACY_UPDATED = "8 November 2026"

BY_SA_4 = "https://creativecommons.org/licenses/by-sa/4.0/"
LICENCE_LINKS = {
    "CC BY-SA 4.0": [BY_SA_4],
    "CC BY-SA 3.0": ["https://creativecommons.org/licenses/by-sa/3.0/"],
    "CC BY and CC0 1.0": ["https://creativecommons.org/licenses/by/4.0/",
                          "https://creativecommons.org/publicdomain/zero/1.0/"],
    "CC BY 2.5 and CC BY 4.0": ["https://creativecommons.org/licenses/by/2.5/",
                                "https://creativecommons.org/licenses/by/4.0/"],
    "Unicode License v3": ["https://www.unicode.org/license.txt"],
    "Public domain": [],
}

PAGES = {
    "index.html": ("NeoOne Keyboard", "An offline iPhone keyboard: type your language the way it sounds and get its own script."),
    "privacy.html": ("Privacy", "NeoOne Keyboard collects no data. Nothing you type leaves your iPhone."),
    "support.html": ("Support", "Help with NeoOne Keyboard, through the project's GitHub issues."),
    "credits.html": ("Credits and licences", "Where NeoOne Keyboard's word lists and models come from, and their licences."),
}


def load_pack_module():
    """scripts/pack.py (python3 -I leaves the script's folder off sys.path)."""
    spec = importlib.util.spec_from_file_location("pack", ROOT / "scripts" / "pack.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def config() -> dict:
    site = json.loads((SITE_SRC / "site.json").read_text(encoding="utf-8"))
    domain, repo = site["domain"].strip(), site["repo"].strip()
    downloads, bucket = site["downloads"].strip(), site["bucket"].strip()
    if not domain or "/" in domain or ":" in domain:
        raise SystemExit(f"site-src/site.json: domain {domain!r} must be a bare host name")
    if repo.count("/") != 1:
        raise SystemExit(f"site-src/site.json: repo {repo!r} must be OWNER/REPO")
    if not downloads.startswith("https://") or not downloads.endswith("/"):
        raise SystemExit(f"site-src/site.json: downloads {downloads!r} must be an https URL ending in /")
    if not bucket:
        raise SystemExit("site-src/site.json: bucket (the R2 bucket of the pack files) is empty")
    return {"domain": domain, "repo": repo, "downloads": downloads, "bucket": bucket,
            "index_url": f"https://{domain}/packs/index.json"}


# --- Packs -----------------------------------------------------------------------------------

def build_packs(pack_module, out: Path, downloads: str) -> list[dict]:
    """Zips every pack in packs/ into `out`, one file per pack id and version."""
    releases = []
    for folder in sorted(PACKS.glob("*.neopack")):
        manifest = json.loads((folder / "manifest.json").read_text(encoding="utf-8"))
        current = pack_module.listed(folder, manifest)
        if manifest.get("sha256") != current:
            raise SystemExit(f"{folder}: checksums stale; run scripts/pack.py {folder} --write")
        pid, version = manifest["id"], manifest["version"]
        archive = out / f"{pid}-{version}.neopack"
        pack_module.write_zip(folder, list(current), archive)
        entry = {
            "id": pid,
            "name": manifest["name"],
            "nativeName": manifest.get("nativeName"),
            "version": version,
            "script": manifest["script"],
            "minEngine": manifest["minEngine"],
            "size": archive.stat().st_size,
            "sha256": sha256(archive),
            "url": downloads + archive.name,
            "englishFingerprint": manifest.get("englishFingerprint"),
        }
        releases.append({"asset": archive, "manifest": manifest,
                         "entry": {k: v for k, v in entry.items() if v is not None}})
    return releases


def build_english(out: Path, downloads: str) -> dict:
    """english-base-<sha256 prefix>.zip: the keyboard's built-in English word lists, published
    for share-alike. The name changes with the bytes, as a pack's changes with its version."""
    scratch = out / "english-base.zip.tmp"
    out.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(scratch, "w", compression=zipfile.ZIP_STORED) as z:
        for name in ENGLISH_FILES:
            info = zipfile.ZipInfo(f"english-base/{name}", date_time=(1980, 1, 1, 0, 0, 0))
            info.external_attr = 0o644 << 16
            z.writestr(info, (LEXICON / name).read_bytes())
    digest = sha256(scratch)
    archive = scratch.replace(out / f"english-base-{digest[:12]}.zip")
    return {"asset": archive, "url": downloads + archive.name, "sha256": digest,
            "size": archive.stat().st_size}


def index_json(releases: list[dict]) -> str:
    packs = [r["entry"] for r in sorted(releases, key=lambda r: r["entry"]["id"])]
    return json.dumps({"format": INDEX_FORMAT, "packs": packs}, ensure_ascii=False, indent=2) + "\n"


def republished(old_index: Path, releases: list[dict]) -> list[str]:
    """Packs whose id and version are already published with other bytes."""
    if not old_index.is_file():
        return []
    old = {(p["id"], p["version"]): p["sha256"] for p in json.loads(old_index.read_text(encoding="utf-8"))["packs"]}
    return [f"{e['id']} {e['version']}" for e in (r["entry"] for r in releases)
            if old.get((e["id"], e["version"]), e["sha256"]) != e["sha256"]]


# --- Credits ---------------------------------------------------------------------------------

def source_attribution(url: str | None, pack_id: str | None) -> str | None:
    """A data/manifests source's attribution, for a pack credit that has none of its own: the
    manifest with the same homepage, preferring the one that names this pack."""
    if not url:
        return None
    found = []
    for path in sorted(DATA_MANIFESTS.glob("*.json")):
        m = json.loads(path.read_text(encoding="utf-8"))
        if (m.get("homepage") or "").rstrip("/") == url.rstrip("/") and m.get("attribution"):
            found.append(m)
    found.sort(key=lambda m: f"packs/{pack_id}.neopack" not in m.get("status", ""))
    return found[0]["attribution"] if found else None


def credit(raw: dict, pack_id: str | None = None) -> dict:
    # Every licence a combined one names ("CC BY and CC0 1.0"); else the manifest's own link.
    links = LICENCE_LINKS.get(raw["licence"]) or ([raw["licenceUrl"]] if raw.get("licenceUrl") else [])
    return {"name": raw["name"], "licence": raw["licence"], "url": raw.get("url"),
            "links": links, "attribution": raw.get("attribution") or source_attribution(raw.get("url"), pack_id)}


def english_credits() -> list[dict]:
    out = []
    for cid in ENGLISH_CREDIT_IDS:
        m = json.loads((DATA_MANIFESTS / f"{cid}.json").read_text(encoding="utf-8"))
        out.append(credit({"name": m["name"], "licence": m["licence"], "url": m.get("homepage"),
                           "licenceUrl": m.get("licence_url"), "attribution": m.get("attribution")}))
    return out


def credit_html(c: dict) -> str:
    e = html.escape
    name = f'<a href="{e(c["url"])}">{e(c["name"])}</a>' if c.get("url") else e(c["name"])
    parts = [f'<dt>{name}<span class="chip">{e(c["licence"])}</span></dt>']
    if c.get("attribution"):
        parts.append(f"<dd>{e(c['attribution'])}</dd>")
    if c["links"]:
        links = " · ".join(f'<a href="{e(u)}">{e(u)}</a>' for u in c["links"])
        parts.append(f"<dd>Licence: {links}</dd>")
    return "\n".join(parts)


def credits_page(releases: list[dict], english: dict) -> str:
    e = html.escape
    out = ["<h1>Credits and licences</h1>",
           '<p class="lead">NeoOne Keyboard’s word lists and prediction models were built from, and '
           "modified from, the text listed here: the text was filtered, counted, romanised and used to "
           "train models, and the results were compiled into word lists and model weights.</p>",
           "<h2>Licence of the built files</h2>",
           "<p>Several sources are licensed CC BY-SA, so the files built from them, each language pack "
           "and the keyboard’s built-in English word lists, are licensed under "
           f'<a href="{BY_SA_4}">Creative Commons Attribution-ShareAlike 4.0 International (CC BY-SA 4.0)</a>. '
           "You may copy, change and share them under that licence’s terms; they come with no warranty. "
           "The SMC word lists (CC BY-SA 3.0) are adapted under version 4.0, as their licence permits. "
           "The files are plain zips without DRM, published here:</p>",
           "<ul>"]
    for r in sorted(releases, key=lambda r: r["entry"]["name"]):
        ent = r["entry"]
        out.append(f'<li><a href="{e(ent["url"])}">{e(ent["name"])} pack {e(ent["version"])}</a> '
                   f'<span class="muted">({ent["size"] / 1e6:.1f} MB, SHA-256 <code>{ent["sha256"][:16]}…</code>)</span></li>')
    out.append(f'<li><a href="{e(english["url"])}">English base word lists</a> '
               f'<span class="muted">({english["size"] / 1e6:.1f} MB)</span></li>')
    out += ["</ul>",
            "<h2>English, built into the app</h2>", '<dl class="credits">',
            *(credit_html(c) for c in english_credits()), "</dl>"]
    for r in sorted(releases, key=lambda r: r["entry"]["name"]):
        m = r["manifest"]
        out += [f"<h2>{e(m['name'])} pack <span class=\"muted\">({e(m.get('nativeName', m['name']))})</span></h2>",
                '<dl class="credits">', *(credit_html(credit(c, m["id"])) for c in m.get("credits", [])), "</dl>"]
    out += ["<h2>Software</h2>",
            "<p>The keyboard’s engine links open-source Rust libraries under the MIT, Apache-2.0 and similar "
            "licences, and Mozilla’s UniFFI under the Mozilla Public License 2.0, whose source code is at "
            '<a href="https://github.com/mozilla/uniffi-rs">github.com/mozilla/uniffi-rs</a>. The app’s '
            "About screen lists every library with its licence.</p>"]
    return "\n".join(out) + "\n"


# --- Site ------------------------------------------------------------------------------------

def render(page: str, content: str, cfg: dict) -> str:
    title, description = PAGES[page]
    layout = (SITE_SRC / "layout.html").read_text(encoding="utf-8")
    values = {
        "title": "NeoOne Keyboard" if page == "index.html" else f"{title} · NeoOne Keyboard",
        "description": description,
        "content": content.rstrip("\n"),
        "nav_privacy": ' aria-current="page"' if page == "privacy.html" else "",
        "nav_support": ' aria-current="page"' if page == "support.html" else "",
        "nav_credits": ' aria-current="page"' if page == "credits.html" else "",
        "repo": html.escape(cfg["repo"]),
        "index_url": html.escape(cfg["index_url"]),
        "updated": PRIVACY_UPDATED,
    }
    text = layout.replace("{{content}}", values["content"])
    for key, value in values.items():
        text = text.replace("{{" + key + "}}", value)
    if "{{" in text:
        raise SystemExit(f"{page}: unfilled placeholder near {text[text.index('{{'):][:30]!r}")
    return text


def site_files(cfg: dict, releases: list[dict], english: dict) -> dict[str, bytes]:
    """Every file of site/, by path."""
    files = {
        # Workers static assets' header rules; the pack files' headers are hosting/worker.js's.
        "_headers": f"/packs/index.json\n  Cache-Control: {INDEX_CACHE}\n".encode(),
        "style.css": (SITE_SRC / "style.css").read_bytes(),
        "packs/index.json": index_json(releases).encode(),
    }
    for page in PAGES:
        if page == "credits.html":
            content = credits_page(releases, english)
        else:
            content = (SITE_SRC / page).read_text(encoding="utf-8")
        files[page] = render(page, content, cfg).encode()
    return files


def xcconfig(cfg: dict) -> str:
    # `//` starts a comment in an xcconfig; `$()` (empty) keeps the slashes.
    url = cfg["index_url"].replace("://", ":/$()/")
    return ("// Generated by scripts/publish-packs.py from site-src/site.json; don't edit.\n"
            "// The pack index the app downloads from (Info.plist NeoPackIndexURL).\n"
            f"ML_PACK_INDEX_URL = {url}\n")


def out_of_date(files: dict[str, bytes], cfg: dict) -> list[str]:
    """What in site/ and the xcconfig differs from `files` (missing, changed or extra)."""
    if not SITE.is_dir():
        return ["site/"]
    wrong = [p for p, data in files.items() if not (SITE / p).is_file() or (SITE / p).read_bytes() != data]
    wrong += sorted(str(p.relative_to(SITE)) for p in SITE.rglob("*")
                    if p.is_file() and str(p.relative_to(SITE)) not in files)
    if not XCCONFIG.is_file() or XCCONFIG.read_text(encoding="utf-8") != xcconfig(cfg):
        wrong.append(str(XCCONFIG.relative_to(ROOT)))
    return wrong


def uploads(cfg: dict, releases: list[dict], english: dict) -> list[tuple[Path, str, list[str]]]:
    """Each pack file with its download URL and the wrangler command that uploads it."""
    out = []
    for archive, url in [*((r["asset"], r["entry"]["url"]) for r in releases), (english["asset"], english["url"])]:
        command = [*WRANGLER, "r2", "object", "put", f"{cfg['bucket']}/{R2_PREFIX}{archive.name}",
                   "--file", str(archive.relative_to(ROOT)), "--content-type", "application/zip", "--remote"]
        out.append((archive, url, command))
    return out


def served_size(url: str) -> int | None:
    """The Content-Length the site answers a HEAD for `url` with; None when it has no such file
    (404) or can't be reached yet (a first deploy)."""
    request = urllib.request.Request(url, method="HEAD", headers={"User-Agent": "publish-packs"})
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return int(response.headers["Content-Length"])
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return None
        raise SystemExit(f"{url}: HTTP {error.code}")
    except urllib.error.URLError:
        return None


def deploy(cfg: dict, releases: list[dict], english: dict) -> int:
    for archive, url, command in uploads(cfg, releases, english):
        size = served_size(url)
        if size == archive.stat().st_size:
            print(f"{archive.name}: already served")
            continue
        if size is not None:
            print(f"{url} is served with {size} bytes, not {archive.stat().st_size}; a published file "
                  "doesn't change: raise the pack's version", file=sys.stderr)
            return 1
        print(f"{archive.name}: uploading")
        subprocess.run(command, cwd=ROOT, check=True)
    subprocess.run([*WRANGLER, "deploy", "--config", str(WRANGLER_CONFIG.relative_to(ROOT))], cwd=ROOT, check=True)
    missing = [url for archive, url, _ in uploads(cfg, releases, english) if served_size(url) != archive.stat().st_size]
    if missing:
        print(f"not served yet (the custom domain may take a minute): {', '.join(missing)}", file=sys.stderr)
        return 1
    print(f"deployed: https://{cfg['domain']}/, index {cfg['index_url']}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--check", action="store_true", help="compare with site/ and the xcconfig; write nothing")
    mode.add_argument("--deploy", action="store_true",
                      help="upload the pack files the site doesn't serve yet, then wrangler deploy")
    args = parser.parse_args()
    sys.stdout.reconfigure(line_buffering=True)  # in order with wrangler's output

    cfg = config()
    pack_module = load_pack_module()
    with tempfile.TemporaryDirectory() as scratch:
        out = Path(scratch) if args.check else BUILD
        if not args.check and BUILD.is_dir():
            for old in BUILD.iterdir():  # earlier versions' files
                if old.is_file():
                    old.unlink()
        releases = build_packs(pack_module, out, cfg["downloads"])
        english = build_english(out, cfg["downloads"])
        files = site_files(cfg, releases, english)

        stale = republished(SITE / "packs" / "index.json", releases)
        if stale:
            print(f"already published with other bytes: {', '.join(stale)}; raise the pack's version "
                  "(packs/<id>.neopack/manifest.json, then scripts/pack.py --write)", file=sys.stderr)
            return 1

        if args.check or args.deploy:
            wrong = out_of_date(files, cfg)
            if wrong:
                print(f"out of date: {', '.join(wrong)}; run make site-packs", file=sys.stderr)
                return 1
            if args.deploy:
                return deploy(cfg, releases, english)
            print("site: up to date")
            return 0

        for path in SITE.rglob("*") if SITE.is_dir() else []:
            if path.is_file() and str(path.relative_to(SITE)) not in files:
                path.unlink()  # a file the site no longer has
        for path, data in files.items():
            target = SITE / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
        XCCONFIG.write_text(xcconfig(cfg), encoding="utf-8")

    for r in releases:
        ent = r["entry"]
        print(f"{ent['id']} {ent['version']}: {ent['size']} bytes, sha256 {ent['sha256']}")
    print(f"english-base: {english['size']} bytes, sha256 {english['sha256']}")
    print(f"wrote site/ ({len(files)} files) and {XCCONFIG.relative_to(ROOT)}; index URL {cfg['index_url']}")
    print("\nmake site-deploy uploads the pack files the site doesn't serve yet and deploys the site. "
          "By hand, once per new file:")
    for _, _, command in uploads(cfg, releases, english):
        print(shlex.join(command))
    print(shlex.join([*WRANGLER, "deploy", "--config", str(WRANGLER_CONFIG.relative_to(ROOT))]))
    return 0


if __name__ == "__main__":
    sys.exit(main())
