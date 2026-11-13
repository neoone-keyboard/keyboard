#!/usr/bin/env python3
"""The download site and its pack index (docs/design/pack-format.md, "Where packs live", hosting).

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

Kept for older engines: when a pack's new version needs a higher minEngine, apps on the older
engine can't read it and would stop being offered the language. site-src/kept-packs.json lists,
by hand, the newest published file of such a pack that an older engine reads (its size, sha256
and url, copied from the index that published it), and the index lists it too. Each kept entry
must be older and need a lower engine than the pack in packs/, and be the newest entry of its id
that its engine reads; one that no app would pick any more is refused, so delete it then. Its
file stays in R2: --deploy checks it is still served and never uploads it. Within an id the
index lists the newest version first; every app picks, per id, the newest entry it can read.

Unlisted packs: site-src/unlisted-packs.json names, by hand, the ids of packs in packs/ that are
not published: trial packs handed out as files (scripts/pack.py --zip) and imported from Files.
Their checksums are checked like any pack's, but they get no zip in build/site-packs/, no entry
in the index, no line or section on the credits page and no upload. An id there that packs/
doesn't have is refused, so delete it; to publish the pack, take its id off the list.

Beta packs: site-src/beta-packs.json names, by hand, the ids of published packs that are listed
as beta, new and still learning. Their entries go in the index's `beta` section, which builds
before it never read: those decode only `format` and `packs` (KeyboardCore.PackIndex since it
was written, Android's PackIndex too), so they don't offer a beta pack as a normal language.
Builds that read `beta` list the entries of both sections and tag those in `beta` "Beta". The
ids in its `alsoInPacks` (Assamese and Tamil, listed before there was a beta section) stay in
`packs` too, the same entry in both, so users of older builds keep them. The credits page marks
beta packs. Which pack is full, beta or unlisted is the maintainer's decision, LISTING below; --check
fails when site-src/ lists them otherwise.

Then it prints the `wrangler r2 object put` commands that upload the pack files. It runs them
only with --deploy (`make site-deploy`), which uploads the files the site doesn't serve yet and
then runs `wrangler deploy`. A file's name carries its version, so a published file never
changes: a pack whose version is already in site/packs/index.json with other bytes is refused,
raise the manifest's `version` first.

Standard library only.
"""

import argparse
import functools
import hashlib
import html
import importlib.util
import json
import re
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
KEPT = SITE_SRC / "kept-packs.json"
UNLISTED = SITE_SRC / "unlisted-packs.json"
BETA = SITE_SRC / "beta-packs.json"
LEXICON = ROOT / "apps" / "ios" / "KeyboardExtension" / "Lexicon"
DATA_MANIFESTS = ROOT / "data" / "manifests"

INDEX_FORMAT = 1  # KeyboardCore PackIndex.format
# English's own sources, as KeyboardCore's Credits.english lists them (the app's About screen).
ENGLISH_CREDIT_IDS = ["enwiki", "scowl", "unicode-emoji", "cldr-annotations"]
ENGLISH_FILES = ["en.fst", "en-next.fst"]
# The maintainer's decision: which packs in packs/ are full languages, which are
# listed as beta and which stay unlisted (trial files; Sinhala, a trial, unlisted too).
# alsoInPacks: beta ids that stay in the index's `packs` for older builds, which listed them
# before the beta section existed.
LISTING = {
    "full": ["hi", "ml"],
    "beta": ["as", "gu", "kn", "ne", "pa", "ta", "te"],
    "unlisted": ["bho", "bn", "ja", "mai", "mr", "or", "si", "ur"],
    "alsoInPacks": ["as", "ta"],
}
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
    "support.html": ("Support", "Help with NeoOne Keyboard: common questions and the support email."),
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

def unlisted_ids(raw: dict, pack_ids: list[str]) -> list[str]:
    """The ids site-src/unlisted-packs.json (`raw`) leaves out of the site, checked against the
    ids of the packs in packs/ (`pack_ids`). Raises SystemExit for anything but a list of
    distinct ids of packs there."""
    ids = raw.get("ids", [])
    if not isinstance(ids, list) or not all(isinstance(i, str) for i in ids):
        raise SystemExit("unlisted-packs.json: ids must be a list of pack ids")
    twice = sorted({i for i in ids if ids.count(i) > 1})
    if twice:
        raise SystemExit(f"unlisted-packs.json: listed twice: {', '.join(twice)}")
    unknown = [i for i in ids if i not in pack_ids]
    if unknown:
        raise SystemExit(f"unlisted-packs.json: packs/ has no pack {', '.join(unknown)}; delete the id")
    return ids


def read_unlisted(manifests: list[tuple[Path, dict]]) -> list[str]:
    if not UNLISTED.is_file():
        return []
    return unlisted_ids(json.loads(UNLISTED.read_text(encoding="utf-8")), [m["id"] for _, m in manifests])


def beta_ids(raw: dict, pack_ids: list[str], unlisted: list[str]) -> tuple[list[str], list[str]]:
    """The ids site-src/beta-packs.json (`raw`) lists as beta, and those of them that stay in the
    index's `packs` too (`alsoInPacks`). Raises SystemExit for anything but distinct ids of
    published packs in packs/ (`pack_ids` less `unlisted`), and alsoInPacks ids that are beta."""
    out = []
    for key in ("ids", "alsoInPacks"):
        ids = raw.get(key, [])
        if not isinstance(ids, list) or not all(isinstance(i, str) for i in ids):
            raise SystemExit(f"beta-packs.json: {key} must be a list of pack ids")
        twice = sorted({i for i in ids if ids.count(i) > 1})
        if twice:
            raise SystemExit(f"beta-packs.json: {key}: listed twice: {', '.join(twice)}")
        out.append(ids)
    ids, also = out
    unknown = [i for i in ids if i not in pack_ids]
    if unknown:
        raise SystemExit(f"beta-packs.json: packs/ has no pack {', '.join(unknown)}; delete the id")
    both = [i for i in ids if i in unlisted]
    if both:
        raise SystemExit(f"beta-packs.json: {', '.join(both)} also in unlisted-packs.json; a beta pack is published")
    stray = [i for i in also if i not in ids]
    if stray:
        raise SystemExit(f"beta-packs.json: alsoInPacks {', '.join(stray)} not in ids")
    return ids, also


def read_beta(manifests: list[tuple[Path, dict]], unlisted: list[str]) -> tuple[list[str], list[str]]:
    if not BETA.is_file():
        return [], []
    return beta_ids(json.loads(BETA.read_text(encoding="utf-8")), [m["id"] for _, m in manifests], unlisted)


def listing_problems(pack_ids: list[str], unlisted: list[str], beta: list[str], also: list[str],
                     decision: dict = LISTING) -> list[str]:
    """How the site's listing (the packs in packs/, unlisted and beta) differs from `decision`;
    empty when it matches."""
    wrong = []
    got = {"unlisted": sorted(unlisted), "beta": sorted(beta), "alsoInPacks": sorted(also),
           "full": sorted(set(pack_ids) - set(unlisted) - set(beta))}
    for kind in ("full", "beta", "unlisted", "alsoInPacks"):
        want = sorted(decision[kind])
        if got[kind] != want:
            extra = sorted(set(got[kind]) - set(want))
            missing = sorted(set(want) - set(got[kind]))
            wrong.append(f"{kind}: {', '.join(extra) or 'nothing'} should not be, "
                         f"{', '.join(missing) or 'nothing'} should be")
    return wrong


def pack_manifests() -> list[tuple[Path, dict]]:
    """Every pack folder in packs/ with its manifest."""
    return [(folder, json.loads((folder / "manifest.json").read_text(encoding="utf-8")))
            for folder in sorted(PACKS.glob("*.neopack"))]


def build_packs(pack_module, out: Path, downloads: str, manifests: list[tuple[Path, dict]],
                unlisted: list[str]) -> list[dict]:
    """Zips every pack of `manifests` into `out`, one file per pack id and version, but for the
    `unlisted` ones, whose checksums are checked and nothing more."""
    releases = []
    for folder, manifest in manifests:
        current = pack_module.listed(folder, manifest)
        if manifest.get("sha256") != current:
            raise SystemExit(f"{folder}: checksums stale; run scripts/pack.py {folder} --write")
        pid, version = manifest["id"], manifest["version"]
        if pid in unlisted:
            continue
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


def compare_versions(a: str, b: str) -> int:
    """-1, 0 or 1, as KeyboardCore's PackVersion orders a manifest's `version`: part by part at
    the dots, numerically where both parts are numbers, else as text; a missing part is "0"."""
    x, y = a.split("."), b.split(".")
    for i in range(max(len(x), len(y))):
        p = x[i] if i < len(x) else "0"
        q = y[i] if i < len(y) else "0"
        if p.isdigit() and q.isdigit():
            p, q = int(p), int(q)
        if p != q:
            return -1 if p < q else 1
    return 0


version_key = functools.cmp_to_key(compare_versions)

KEPT_FIELDS = {"id": str, "name": str, "nativeName": str, "version": str, "script": str,
               "minEngine": int, "size": int, "sha256": str, "url": str, "englishFingerprint": str}
KEPT_REQUIRED = {"id", "name", "version", "script", "minEngine", "size", "sha256", "url"}


def kept_entries(kept: dict, releases: list[dict], downloads: str) -> list[dict]:
    """The index entries in site-src/kept-packs.json (`kept`), checked against the packs in
    packs/ (`releases`): each for a pack id there, older and for a lower engine than that pack,
    with a pack file name of its id and version at `downloads`, and still the newest entry of
    its id that some engine reads. Raises SystemExit naming the entry otherwise."""
    current = {r["entry"]["id"]: r["entry"] for r in releases}
    out = []
    for raw in kept.get("packs", []):
        name = f"kept-packs.json: {raw.get('id')} {raw.get('version')}"
        unknown = set(raw) - set(KEPT_FIELDS)
        missing = KEPT_REQUIRED - set(raw)
        if unknown or missing:
            raise SystemExit(f"{name}: unknown fields {sorted(unknown)}, missing {sorted(missing)}")
        for key, kind in KEPT_FIELDS.items():
            if key in raw and (type(raw[key]) is not kind or (kind is int and raw[key] <= 0)):
                raise SystemExit(f"{name}: {key} must be a{' positive' if kind is int else ''} {kind.__name__}")
        pack = current.get(raw["id"])
        if pack is None:
            raise SystemExit(f"{name}: packs/ has no pack {raw['id']}; delete the entry")
        if not (compare_versions(raw["version"], pack["version"]) < 0 and raw["minEngine"] < pack["minEngine"]):
            raise SystemExit(f"{name}: must be older than {pack['version']} and need an engine below "
                             f"{pack['minEngine']}, the version in packs/")
        if raw["url"] != f"{downloads}{raw['id']}-{raw['version']}.neopack":
            raise SystemExit(f"{name}: url must be {downloads}{raw['id']}-{raw['version']}.neopack")
        if not re.fullmatch(r"[0-9a-f]{64}", raw["sha256"]):
            raise SystemExit(f"{name}: sha256 must be 64 lowercase hex digits")
        out.append(dict(raw))
    every = [r["entry"] for r in releases] + out
    for entry in out:
        # An app on engine e picks the newest entry with minEngine <= e. This one is picked by
        # some engine only if no other entry of its id is as new and needs no more than it does.
        shadow = [o for o in every if o is not entry and o["id"] == entry["id"] and o["minEngine"] <= entry["minEngine"]
                  and compare_versions(o["version"], entry["version"]) >= 0]
        if shadow:
            raise SystemExit(f"kept-packs.json: {entry['id']} {entry['version']}: no app picks it, "
                             f"{shadow[0]['version']} (minEngine {shadow[0]['minEngine']}) is at least as new "
                             "for the same engines; delete the entry")
    return out


def read_kept(releases: list[dict], downloads: str) -> list[dict]:
    if not KEPT.is_file():
        return []
    return kept_entries(json.loads(KEPT.read_text(encoding="utf-8")), releases, downloads)


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


def index_entries(releases: list[dict], kept: list[dict]) -> list[dict]:
    """By id, and within an id the newest version first (the pack in packs/, then kept ones)."""
    entries = [r["entry"] for r in releases] + kept
    by_version = sorted(entries, key=lambda e: version_key(e["version"]), reverse=True)
    return sorted(by_version, key=lambda e: e["id"])


def index_json(releases: list[dict], kept: list[dict], beta=(), also=()) -> str:
    """The index: `packs`, what every build reads, without the `beta` ids but for those in
    `also`; and, when there are beta packs, `beta`, their entries, which older builds ignore."""
    entries = index_entries(releases, kept)
    index = {"format": INDEX_FORMAT,
             "packs": [e for e in entries if e["id"] not in beta or e["id"] in also]}
    if beta:
        index["beta"] = [e for e in entries if e["id"] in beta]
    return json.dumps(index, ensure_ascii=False, indent=2) + "\n"


def republished(old_index: Path, entries: list[dict]) -> list[str]:
    """Index entries whose id and version are already published with other bytes."""
    if not old_index.is_file():
        return []
    raw = json.loads(old_index.read_text(encoding="utf-8"))
    old = {(p["id"], p["version"]): p["sha256"] for p in raw["packs"] + raw.get("beta", [])}
    return [f"{e['id']} {e['version']}" for e in entries
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


def credits_page(releases: list[dict], kept: list[dict], english: dict, beta=()) -> str:
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
    older = [id(k) for k in kept]
    for ent in sorted(index_entries(releases, kept), key=lambda x: x["name"]):  # stable: newest first
        note = ", for older app versions" if id(ent) in older else ""
        note += ", beta" if ent["id"] in beta else ""
        out.append(f'<li><a href="{e(ent["url"])}">{e(ent["name"])} pack {e(ent["version"])}</a>{note} '
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
        "domain": html.escape(cfg["domain"]),
        "index_url": html.escape(cfg["index_url"]),
        "updated": PRIVACY_UPDATED,
    }
    text = layout.replace("{{content}}", values["content"])
    for key, value in values.items():
        text = text.replace("{{" + key + "}}", value)
    if "{{" in text:
        raise SystemExit(f"{page}: unfilled placeholder near {text[text.index('{{'):][:30]!r}")
    return text


def site_files(cfg: dict, releases: list[dict], kept: list[dict], english: dict,
               beta=(), also=()) -> dict[str, bytes]:
    """Every file of site/, by path."""
    files = {
        # Workers static assets' header rules; the pack files' headers are hosting/worker.js's.
        "_headers": f"/packs/index.json\n  Cache-Control: {INDEX_CACHE}\n".encode(),
        "style.css": (SITE_SRC / "style.css").read_bytes(),
        "packs/index.json": index_json(releases, kept, beta, also).encode(),
    }
    for page in PAGES:
        if page == "credits.html":
            content = credits_page(releases, kept, english, beta)
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


def deploy(cfg: dict, releases: list[dict], kept: list[dict], english: dict) -> int:
    for entry in kept:  # uploaded when they were the version in packs/; never uploaded here
        size = served_size(entry["url"])
        if size != entry["size"]:
            print(f"{entry['url']} (kept for older engines) is served with {size} bytes, not {entry['size']}; "
                  "upload it again or delete its entry from site-src/kept-packs.json", file=sys.stderr)
            return 1
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
        manifests = pack_manifests()
        unlisted = read_unlisted(manifests)
        beta, also = read_beta(manifests, unlisted)
        decision = listing_problems([m["id"] for _, m in manifests], unlisted, beta, also)
        if decision:
            print("the packs are not listed as decided (LISTING in scripts/publish-packs.py): "
                  f"{'; '.join(decision)}", file=sys.stderr)
            return 1
        releases = build_packs(pack_module, out, cfg["downloads"], manifests, unlisted)
        kept = read_kept(releases, cfg["downloads"])
        english = build_english(out, cfg["downloads"])
        files = site_files(cfg, releases, kept, english, beta, also)

        stale = republished(SITE / "packs" / "index.json", [r["entry"] for r in releases] + kept)
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
                return deploy(cfg, releases, kept, english)
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
        tag = " (beta)" if ent["id"] in beta else ""
        print(f"{ent['id']} {ent['version']}{tag}: {ent['size']} bytes, sha256 {ent['sha256']}")
    for _, m in manifests:
        if m["id"] in unlisted:
            print(f"{m['id']} {m['version']}: unlisted (site-src/unlisted-packs.json), not published")
    for ent in kept:
        print(f"{ent['id']} {ent['version']}: kept for engine {ent['minEngine']}, served at {ent['url']}")
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
