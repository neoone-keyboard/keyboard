#!/usr/bin/env bash
# Fetches the Tamil sources in data/manifests/ (tawiki, tawikisource, tawikiquote,
# aksharantar-tam) and extracts the wiki dumps to plain text in data/raw/text/ta.txt, one
# paragraph per line. Cached: delete data/raw/text/ta.txt to re-extract. Prints how many words each
# source gave (data/text-sources.sh), and warns when one is most of the text.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RAW="$ROOT/data/raw"
TEXT="$RAW/text"
PIPELINE="$ROOT/data/pipeline"

py() { uv run --quiet --project "$PIPELINE" python -I "$@"; }
# shellcheck source=data/text-sources.sh
. "$ROOT/data/text-sources.sh"

"$ROOT/data/fetch.sh" --only tawiki --only tawikisource --only tawikiquote --only aksharantar-tam
if [ ! -d "$RAW/aksharantar-tam/extracted" ]; then
  unzip -q -o "$RAW/aksharantar-tam/tam.zip" -d "$RAW/aksharantar-tam/extracted"
fi
mkdir -p "$TEXT"

if [ ! -s "$TEXT/ta.txt" ]; then
  reset_sources ta
  py "$PIPELINE/extract_wiki.py" "$RAW"/tawiki/*.bz2 --out "$TEXT/ta.txt.tmp"
  count_source ta tawiki "$TEXT/ta.txt.tmp"
  # Wikisource proofread text is in the Page namespace (key 250 on Tamil Wikisource), not the main one.
  py "$PIPELINE/extract_wiki.py" "$RAW"/tawikisource/*.bz2 --extra-namespaces 250,Page,பக்கம் \
    --out "$TEXT/taws.txt.tmp"
  py "$PIPELINE/extract_wiki.py" "$RAW"/tawikiquote/*.bz2 --out "$TEXT/tawq.txt.tmp"
  count_source ta tawikisource "$TEXT/taws.txt.tmp"
  count_source ta tawikiquote "$TEXT/tawq.txt.tmp"
  cat "$TEXT/taws.txt.tmp" "$TEXT/tawq.txt.tmp" >> "$TEXT/ta.txt.tmp"
  rm "$TEXT/taws.txt.tmp" "$TEXT/tawq.txt.tmp"
  mv "$TEXT/ta.txt.tmp" "$TEXT/ta.txt"
fi
echo "Tamil text in $TEXT/ta.txt"
report_sources ta
