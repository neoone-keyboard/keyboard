#!/usr/bin/env bash
# Fetches the Assamese sources in data/manifests/ (aswiki, aswikisource, aswikiquote,
# aksharantar-asm) and extracts the wiki dumps to plain text in data/raw/text/as.txt, one
# paragraph per line. Cached: delete data/raw/text/as.txt to re-extract. Prints how many words each
# source gave (data/text-sources.sh), and warns when one is most of the text.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RAW="$ROOT/data/raw"
TEXT="$RAW/text"
PIPELINE="$ROOT/data/pipeline"

py() { uv run --quiet --project "$PIPELINE" python -I "$@"; }
# shellcheck source=data/text-sources.sh
. "$ROOT/data/text-sources.sh"

"$ROOT/data/fetch.sh" --only aswiki --only aswikisource --only aswikiquote --only aksharantar-asm
if [ ! -d "$RAW/aksharantar-asm/extracted" ]; then
  unzip -q -o "$RAW/aksharantar-asm/asm.zip" -d "$RAW/aksharantar-asm/extracted"
fi
mkdir -p "$TEXT"

if [ ! -s "$TEXT/as.txt" ]; then
  reset_sources as
  py "$PIPELINE/extract_wiki.py" "$RAW"/aswiki/*.bz2 --out "$TEXT/as.txt.tmp"
  count_source as aswiki "$TEXT/as.txt.tmp"
  # Wikisource proofread text is in the Page namespace (key 104), not the main one.
  py "$PIPELINE/extract_wiki.py" "$RAW"/aswikisource/*.bz2 --extra-namespaces 104,Page,পৃষ্ঠা \
    --out "$TEXT/asws.txt.tmp"
  py "$PIPELINE/extract_wiki.py" "$RAW"/aswikiquote/*.bz2 --out "$TEXT/aswq.txt.tmp"
  count_source as aswikisource "$TEXT/asws.txt.tmp"
  count_source as aswikiquote "$TEXT/aswq.txt.tmp"
  cat "$TEXT/asws.txt.tmp" "$TEXT/aswq.txt.tmp" >> "$TEXT/as.txt.tmp"
  rm "$TEXT/asws.txt.tmp" "$TEXT/aswq.txt.tmp"
  mv "$TEXT/as.txt.tmp" "$TEXT/as.txt"
fi
echo "Assamese text in $TEXT/as.txt"
report_sources as
