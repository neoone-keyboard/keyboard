#!/usr/bin/env bash
# Fetches the Marathi sources in data/manifests/ (mrwiki, mrwikisource, mrwikiquote,
# aksharantar-mar) and extracts the wiki dumps to plain text in data/raw/text/mr.txt, one
# paragraph per line. Cached: delete data/raw/text/mr.txt to re-extract. Prints how many words each
# source gave (data/text-sources.sh), and warns when one is most of the text.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RAW="$ROOT/data/raw"
TEXT="$RAW/text"
PIPELINE="$ROOT/data/pipeline"

py() { uv run --quiet --project "$PIPELINE" python -I "$@"; }
# shellcheck source=data/text-sources.sh
. "$ROOT/data/text-sources.sh"

"$ROOT/data/fetch.sh" --only mrwiki --only mrwikisource --only mrwikiquote --only aksharantar-mar
if [ ! -d "$RAW/aksharantar-mar/extracted" ]; then
  unzip -q -o "$RAW/aksharantar-mar/mar.zip" -d "$RAW/aksharantar-mar/extracted"
fi
mkdir -p "$TEXT"

if [ ! -s "$TEXT/mr.txt" ]; then
  reset_sources mr
  py "$PIPELINE/extract_wiki.py" "$RAW"/mrwiki/*.bz2 --out "$TEXT/mr.txt.tmp"
  count_source mr mrwiki "$TEXT/mr.txt.tmp"
  # Wikisource proofread text is in the Page namespace (key 104, पान), not the main one.
  py "$PIPELINE/extract_wiki.py" "$RAW"/mrwikisource/*.bz2 --extra-namespaces 104,Page,पान \
    --out "$TEXT/mrws.txt.tmp"
  # Wikisource (older literature and scanned books) would be 61% of the words; every second
  # paragraph keeps it under half (docs/make-a-language.md, step 3).
  awk 'NR % 2 == 1' "$TEXT/mrws.txt.tmp" > "$TEXT/mrws.thin.tmp" && mv "$TEXT/mrws.thin.tmp" "$TEXT/mrws.txt.tmp"
  py "$PIPELINE/extract_wiki.py" "$RAW"/mrwikiquote/*.bz2 --out "$TEXT/mrwq.txt.tmp"
  count_source mr mrwikisource "$TEXT/mrws.txt.tmp"
  count_source mr mrwikiquote "$TEXT/mrwq.txt.tmp"
  cat "$TEXT/mrws.txt.tmp" "$TEXT/mrwq.txt.tmp" >> "$TEXT/mr.txt.tmp"
  rm "$TEXT/mrws.txt.tmp" "$TEXT/mrwq.txt.tmp"
  mv "$TEXT/mr.txt.tmp" "$TEXT/mr.txt"
fi
echo "Marathi text in $TEXT/mr.txt"
report_sources mr
