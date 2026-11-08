#!/usr/bin/env python3
"""The download site and its pack index (M6 design, "Where packs live", hosting).

    python3 -I scripts/publish-packs.py           build the packs, write site/ and the app's index URL
    python3 -I scripts/publish-packs.py --check   fail if site/ or the xcconfig is out of date

The site's address and the GitHub repository are in one place, site-src/site.json. From it and
the pack folders in packs/ this writes:

- build/site-packs/<id>.neopack: each pack's zip (scripts/pack.py, byte-for-byte reproducible),
  and english-base.zip, the keyboard's built-in English word lists. These are GitHub release
  assets, not part of the site. Also a release-notes file for each.
- site/: the GitHub Pages site. index.html, privacy.html, support.html and credits.html from
  site-src/ (credits from the pack manifests and data/manifests), style.css, CNAME, .nojekyll,
  and packs/index.json, the pack index the app downloads from (KeyboardCore.PackIndex, format 1),
  whose entries point at the release assets.
- apps/ios/PackIndex.xcconfig: ML_PACK_INDEX_URL, the app's Info.plist NeoPackIndexURL.

Then it prints the `gh release create` commands that publish the assets. It never runs them.
A pack whose version is already in site/packs/index.json with other bytes is refused (its
release already exists): raise the manifest's `version` first.

Standard library only.
"""

import argparse
import hashlib
import html
import importlib.util
import json
import shlex
import sys
import tempfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SITE_SRC = ROOT / "site-src"
SITE = ROOT / "site"
PACKS = ROOT / "packs"
BUILD = ROOT / "build" / "site-packs"
XCCONFIG = ROOT / "apps" / "ios" / "PackIndex.xcconfig"
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
    "index.html": ("NeoOne Keyboard", "An offline iPhone keyboard for English and Indian languages typed phonetically."),
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
    if not domain or "/" in domain or ":" in domain:
        raise SystemExit(f"site-src/site.json: domain {domain!r} must be a bare host name")
    if repo.count("/") != 1:
        raise SystemExit(f"site-src/site.json: repo {repo!r} must be OWNER/REPO")
    return {"domain": domain, "repo": repo, "index_url": f"https://{domain}/packs/index.json"}


def asset_url(repo: str, tag: str, name: str) -> str:
    return f"https://github.com/{repo}/releases/download/{tag}/{name}"


# --- Packs -----------------------------------------------------------------------------------

def build_packs(pack_module, out: Path, repo: str) -> list[dict]:
    """Zips every pack in packs/ into `out`; one release per pack id and version."""
    releases = []
    for folder in sorted(PACKS.glob("*.neopack")):
        manifest = json.loads((folder / "manifest.json").read_text(encoding="utf-8"))
        current = pack_module.listed(folder, manifest)
        if manifest.get("sha256") != current:
            raise SystemExit(f"{folder}: checksums stale; run scripts/pack.py {folder} --write")
        pid, version = manifest["id"], manifest["version"]
        archive = out / f"{pid}.neopack"
        pack_module.write_zip(folder, list(current), archive)
        tag = f"packs-{pid}-{version}"
        entry = {
            "id": pid,
            "name": manifest["name"],
            "nativeName": manifest.get("nativeName"),
            "version": version,
            "script": manifest["script"],
            "minEngine": manifest["minEngine"],
            "size": archive.stat().st_size,
            "sha256": sha256(archive),
            "url": asset_url(repo, tag, archive.name),
            "englishFingerprint": manifest.get("englishFingerprint"),
        }
        releases.append({
            "tag": tag, "asset": archive, "entry": {k: v for k, v in entry.items() if v is not None},
            "title": f"{manifest['name']} language pack {version}", "manifest": manifest,
        })
    return releases


def build_english(out: Path, repo: str) -> dict:
    """english-base.zip: the keyboard's built-in English word lists, published for share-alike."""
    archive = out / "english-base.zip"
    out.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_STORED) as z:
        for name in ENGLISH_FILES:
            info = zipfile.ZipInfo(f"english-base/{name}", date_time=(1980, 1, 1, 0, 0, 0))
            info.external_attr = 0o644 << 16
            z.writestr(info, (LEXICON / name).read_bytes())
    digest = sha256(archive)
    tag = f"english-base-{digest[:12]}"
    return {"tag": tag, "asset": archive, "url": asset_url(repo, tag, archive.name),
            "title": "English base word lists", "sha256": digest, "size": archive.stat().st_size}


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


def credits_page(releases: list[dict], english: dict, repo: str) -> str:
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
           "The files are plain zips without DRM, published on the project’s GitHub releases:</p>",
           "<ul>"]
    for r in sorted(releases, key=lambda r: r["entry"]["name"]):
        ent = r["entry"]
        out.append(f'<li><a href="{e(ent["url"])}">{e(ent["name"])} pack {e(ent["version"])}</a> '
                   f'<span class="muted">({ent["size"] / 1e6:.1f} MB, SHA-256 <code>{ent["sha256"][:16]}…</code>)</span></li>')
    out.append(f'<li><a href="{e(english["url"])}">English base word lists</a> '
               f'<span class="muted">({english["size"] / 1e6:.1f} MB)</span></li>')
    out += ["</ul>",
            f'<p>All releases: <a href="https://github.com/{e(repo)}/releases">github.com/{e(repo)}/releases</a>.</p>',
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


def release_notes(manifest: dict | None, domain: str) -> str:
    lines = []
    if manifest:
        lines += [f"{manifest['name']} ({manifest.get('nativeName', manifest['name'])}) language pack for "
                  f"NeoOne Keyboard, version {manifest['version']}. The app downloads it from its Languages "
                  "screen; it can also be imported from Files as a .neopack file.", ""]
        sources = [credit(c, manifest["id"]) for c in manifest.get("credits", [])]
    else:
        lines += ["The word lists built into NeoOne Keyboard for English (en.fst, en-next.fst), "
                  "published under CC BY-SA 4.0.", ""]
        sources = english_credits()
    lines += ["Licence: CC BY-SA 4.0 (https://creativecommons.org/licenses/by-sa/4.0/). These word lists and "
              "models were built from, and modified from, the sources below.", "", "Sources:"]
    for c in sources:
        lines.append(f"- {c['name']}, {c['licence']}" + (f", {c['url']}" if c.get("url") else ""))
    lines += ["", f"Full credits: https://{domain}/credits.html"]
    return "\n".join(lines) + "\n"


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
        "CNAME": (cfg["domain"] + "\n").encode(),
        ".nojekyll": b"",
        "style.css": (SITE_SRC / "style.css").read_bytes(),
        "packs/index.json": index_json(releases).encode(),
    }
    for page in PAGES:
        if page == "credits.html":
            content = credits_page(releases, english, cfg["repo"])
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


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--check", action="store_true", help="compare with site/ and the xcconfig; write nothing")
    args = parser.parse_args()

    cfg = config()
    pack_module = load_pack_module()
    with tempfile.TemporaryDirectory() as scratch:
        out = Path(scratch) if args.check else BUILD
        releases = build_packs(pack_module, out, cfg["repo"])
        english = build_english(out, cfg["repo"])
        files = site_files(cfg, releases, english)

        stale = republished(SITE / "packs" / "index.json", releases)
        if stale:
            print(f"already published with other bytes: {', '.join(stale)}; raise the pack's version "
                  "(packs/<id>.neopack/manifest.json, then scripts/pack.py --write)", file=sys.stderr)
            return 1

        if args.check:
            wrong = [p for p, data in files.items() if (SITE / p).read_bytes() != data] if SITE.is_dir() else ["site/"]
            extra = sorted(str(p.relative_to(SITE)) for p in SITE.rglob("*")
                           if p.is_file() and str(p.relative_to(SITE)) not in files) if SITE.is_dir() else []
            if not XCCONFIG.is_file() or XCCONFIG.read_text(encoding="utf-8") != xcconfig(cfg):
                wrong.append(str(XCCONFIG.relative_to(ROOT)))
            if wrong or extra:
                print(f"out of date: {', '.join(wrong + extra)}; run make site-packs", file=sys.stderr)
                return 1
            print("site: up to date")
            return 0

        for path, data in files.items():
            target = SITE / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
        XCCONFIG.write_text(xcconfig(cfg), encoding="utf-8")
        for r in releases:
            (BUILD / f"notes-{r['entry']['id']}.md").write_text(release_notes(r["manifest"], cfg["domain"]), encoding="utf-8")
        (BUILD / "notes-english-base.md").write_text(release_notes(None, cfg["domain"]), encoding="utf-8")

    for r in releases:
        ent = r["entry"]
        print(f"{ent['id']} {ent['version']}: {ent['size']} bytes, sha256 {ent['sha256']}")
    print(f"english-base: {english['size']} bytes, sha256 {english['sha256']}")
    print(f"wrote site/ ({len(files)} files) and {XCCONFIG.relative_to(ROOT)}; index URL {cfg['index_url']}")
    print("\nTo publish the assets (run once per new tag; an existing tag is already published):")
    rel = lambda p: shlex.quote(str(p.relative_to(ROOT)))
    for r in [*releases, {"tag": english["tag"], "asset": english["asset"], "title": english["title"],
                          "entry": {"id": "english-base"}}]:
        notes = BUILD / f"notes-{r['entry']['id']}.md"
        print(f"gh release create {r['tag']} {rel(r['asset'])} --repo {cfg['repo']} "
              f"--title {shlex.quote(r['title'])} --notes-file {rel(notes)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
