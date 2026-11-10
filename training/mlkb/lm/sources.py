"""Raw inputs for the M3b LM, as plain text with one paragraph per line, in data/raw/text/.

`uv run python -m mlkb.lm.sources` after `data/fetch.sh`. Each step writes a `.tmp` file
first and renames it, as data/build-lexicons.sh does, so an interrupted run is redone, not
trusted. Delete an output to rebuild it.

- `simplewiki.txt`, `wikinews.txt`: data/pipeline/extract_wiki.py over the dumps.
- `gutenberg.txt`: books from the pinned catalogue whose authors all died before 1956 (see
  `gutenberg.py`), downloaded from Project Gutenberg mirrors (never the main site, as
  gutenberg.org/policy/robot_access asks), at most `GUTENBERG_CONNECTIONS` at a time, header and
  footer stripped. The selection
  is written to data/manifests/gutenberg-books.tsv. Mostly-dialogue paragraphs are written
  three times.
- Dakshina `ml/romanized/` rejoined dev and test sentences, extracted from the tar we already
  have; their SHA-256s go under `extracted` in data/manifests/dakshina.json.

With `--lang hi` the Dakshina files are `hi/romanized/`'s. The native text itself
(`data/raw/text/ml.txt`, `hi.txt`) comes from the lexicon builds (data/build-lexicons.sh,
data/build-pack-lexicons.sh hi).
"""
import argparse
import csv
import hashlib
import json
import re
import subprocess
import sys
import time
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from mlkb.lm.gutenberg import CUTOFF, authors_dead_before, dialogue_lines, strip_boilerplate

REPO = Path(__file__).resolve().parents[3]
RAW = REPO / "data" / "raw"
TEXT = RAW / "text"
MANIFESTS = REPO / "data" / "manifests"
PIPELINE = REPO / "data" / "pipeline"

GUTENBERG_WORDS = 85_000_000
# Both are listed in https://www.gutenberg.org/dirs/MIRRORS.ALL ("High speed connection" and
# PG's own high-speed mirror); the first is several times faster from here.
GUTENBERG_MIRRORS = (
    "https://mirror.cs.odu.edu/gutenberg-epub/{id}/pg{id}.txt",
    "https://gutenberg.pglaf.org/cache/epub/{id}/pg{id}.txt",
)
GUTENBERG_PAUSE = 0.5  # seconds before each request, per connection
GUTENBERG_CONNECTIONS = 3
GUTENBERG_BATCH = 60  # books fetched ahead of the selection loop
DIALOGUE_SHARE = 0.5  # a paragraph is dialogue when quoted speech is at least this share of it
DIALOGUE_WEIGHT = 3

DAKSHINA_TAR = RAW / "dakshina" / "dakshina_dataset_v1.0.tar"
DAKSHINA_ROMANIZED = "dakshina_dataset_v1.0/ml/romanized/"
DAKSHINA_FILES = [
    f"{DAKSHINA_ROMANIZED}ml.romanized.rejoined.{split}.{form}.txt"
    for split in ("dev", "test")
    for form in ("native", "roman")
]


def dakshina_files(lang: str = "ml") -> list[str]:
    """The rejoined dev and test sentences of one Dakshina language, as paths in the tar."""
    return [f"dakshina_dataset_v1.0/{lang}/romanized/{lang}.romanized.rejoined.{split}.{form}.txt"
            for split in ("dev", "test") for form in ("native", "roman")]


def _publish(tmp: Path, out: Path) -> None:
    tmp.replace(out)
    print(f"wrote {out}", file=sys.stderr)


def _dumps(manifest_id: str) -> list[Path]:
    manifest = json.loads((MANIFESTS / f"{manifest_id}.json").read_text(encoding="utf-8"))
    return [RAW / manifest_id / f["path"] for f in manifest["files"]]


def extract_wiki(manifest_id: str, out: Path) -> None:
    """Runs the lexicon pipeline's extractor, in its own environment, as the lexicon build does."""
    if out.exists() and out.stat().st_size:
        return
    tmp = out.with_name(out.name + ".tmp")
    subprocess.run(
        ["uv", "run", "--quiet", "--project", str(PIPELINE), "python", "-I",
         str(PIPELINE / "extract_wiki.py"), *map(str, _dumps(manifest_id)), "--out", str(tmp)],
        check=True,
    )
    _publish(tmp, out)


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        while chunk := f.read(1 << 20):
            h.update(chunk)
    return h.hexdigest()


def gutenberg_selection(catalogue: Path) -> list[int]:
    """Ebook ids of English texts whose every listed person died before `CUTOFF`, in id order.

    The catalogue has no download counts, so the order is the ebook id (the plan's fallback)."""
    ids = []
    with open(catalogue, encoding="utf-8", newline="") as f:
        for row in csv.DictReader(f):
            if row["Type"] == "Text" and row["Language"] == "en" and authors_dead_before(row["Authors"], CUTOFF):
                ids.append(int(row["Text#"]))
    return sorted(ids)


def _download(ebook: int, dst: Path) -> bool:
    """Fetches one book from the first mirror that has it; False if none does."""
    if dst.exists() and dst.stat().st_size:
        return True
    dst.parent.mkdir(parents=True, exist_ok=True)
    for pattern in GUTENBERG_MIRRORS:
        url = pattern.format(id=ebook)
        request = urllib.request.Request(url, headers={"User-Agent": "ios-keyboard-lm-pipeline/1.0"})
        for attempt in range(3):
            time.sleep(GUTENBERG_PAUSE * (2**attempt))
            try:
                with urllib.request.urlopen(request, timeout=60) as response:
                    data = response.read()
                tmp = dst.with_name(dst.name + ".part")
                tmp.write_bytes(data)
                tmp.replace(dst)
                return True
            except urllib.error.HTTPError as e:
                if e.code == 404:
                    break
            except OSError:
                continue
    return False


_HEADING = re.compile(r"^[^a-z]*$")  # no lower-case letter: CHAPTER I, THE END, rules
_SPACES = re.compile(r"\s+")


def book_paragraphs(body: str) -> list[str]:
    """Blank-line separated paragraphs, re-joined from PG's hard-wrapped lines. Headings,
    illustrations and anything that names Project Gutenberg (credits, notes) are dropped."""
    out = []
    for block in re.split(r"\n\s*\n", body):
        para = _SPACES.sub(" ", block.replace("_", "")).strip()
        if len(para.split()) < 2 or _HEADING.match(para) or para.startswith("[") or para.endswith("]"):
            continue
        if para[-1].isdigit():  # contents, lists of illustrations, index entries
            continue
        if "gutenberg" in para.lower() or para.lower().startswith(("produced by", "transcriber")):
            continue
        out.append(para)
    return out


def is_dialogue(paragraph: str) -> bool:
    spoken = sum(len(line) for line in dialogue_lines(paragraph))
    return spoken >= DIALOGUE_SHARE * len(paragraph)


def build_gutenberg(out: Path, books_tsv: Path) -> None:
    if out.exists() and out.stat().st_size:
        return
    catalogue = _dumps("gutenberg")[0]
    books = RAW / "gutenberg" / "books"
    tmp = out.with_name(out.name + ".tmp")
    words = refused = missing = 0
    rows = []
    selection = gutenberg_selection(catalogue)
    fetched: dict[int, bool] = {}
    with open(tmp, "w", encoding="utf-8") as text, ThreadPoolExecutor(GUTENBERG_CONNECTIONS) as pool:
        for k, ebook in enumerate(selection):
            if words >= GUTENBERG_WORDS:
                break
            if ebook not in fetched:
                batch = selection[k : k + GUTENBERG_BATCH]
                ok = pool.map(lambda e: _download(e, books / f"pg{e}.txt"), batch)
                fetched.update(zip(batch, ok))
            path = books / f"pg{ebook}.txt"
            if not fetched[ebook]:
                missing += 1
                continue
            body = strip_boilerplate(path.read_bytes().decode("utf-8-sig", errors="replace").replace("\r\n", "\n"))
            if body is None:
                refused += 1
                continue
            paragraphs = book_paragraphs(body)
            book_words = sum(len(p.split()) for p in paragraphs)
            if not book_words:
                continue
            for para in paragraphs:
                for _ in range(DIALOGUE_WEIGHT if is_dialogue(para) else 1):
                    text.write(para + "\n")
            words += book_words
            rows.append((ebook, sha256(path), book_words))
            if len(rows) % 100 == 0:
                print(f"gutenberg: {len(rows)} books, {words:,} words", file=sys.stderr)
    books_tsv.write_text(
        "# Project Gutenberg books in data/raw/text/gutenberg.txt, selected by training/mlkb/lm/sources.py from\n"
        "# pg_catalog-2026-11-03.csv: ebook id, SHA-256 of cache/epub/<id>/pg<id>.txt as downloaded, words kept.\n"
        "ebook_id\tsha256\twords\n" + "".join(f"{i}\t{h}\t{w}\n" for i, h, w in rows),
        encoding="utf-8",
    )
    print(f"gutenberg: {len(rows)} books, {words:,} words; {refused} refused as copyrighted, "
          f"{missing} not on the mirrors", file=sys.stderr)
    _publish(tmp, out)


def extract_dakshina(lang: str = "ml") -> None:
    """The romanised dev and test sentences, with their SHA-256s recorded in the manifest."""
    dst = RAW / "dakshina"
    files = dakshina_files(lang)
    if not all((dst / name).exists() for name in files):
        subprocess.run(["tar", "-xf", str(DAKSHINA_TAR), "-C", str(dst), *files], check=True)
    manifest_path = MANIFESTS / "dakshina.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    for name in files:
        manifest["extracted"][name] = sha256(dst / name)
    manifest_path.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--only", choices=["simplewiki", "wikinews", "gutenberg", "dakshina"], action="append")
    parser.add_argument("--lang", default="ml", choices=["ml", "hi"], help="whose Dakshina sentences")
    args = parser.parse_args(argv)
    steps = args.only or ["simplewiki", "wikinews", "gutenberg", "dakshina"]
    TEXT.mkdir(parents=True, exist_ok=True)
    if "dakshina" in steps:
        extract_dakshina(args.lang)
    if "simplewiki" in steps:
        extract_wiki("simplewiki", TEXT / "simplewiki.txt")
    if "wikinews" in steps:
        extract_wiki("enwikinews", TEXT / "wikinews.txt")
    if "gutenberg" in steps:
        build_gutenberg(TEXT / "gutenberg.txt", MANIFESTS / "gutenberg-books.tsv")
    return 0


if __name__ == "__main__":
    sys.exit(main())
