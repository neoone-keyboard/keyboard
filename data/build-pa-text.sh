#!/usr/bin/env bash
# Fetches the Punjabi sources in data/manifests/ (pawiki, pawikisource, aksharantar-pan) and
# extracts the wiki dumps to plain text in data/raw/text/pa.txt, one paragraph per line;
# Wikipedia's paragraphs that are mostly repeated text are dropped (drop_repeats.py). Punjabi
# (Gurmukhi) has no Wikiquote. Cached: delete data/raw/text/pa.txt to re-extract. Prints how
# many words each source gave (data/text-sources.sh), and warns when one is most of the text.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RAW="$ROOT/data/raw"
TEXT="$RAW/text"
PIPELINE="$ROOT/data/pipeline"

py() { uv run --quiet --project "$PIPELINE" python -I "$@"; }
# shellcheck source=data/text-sources.sh
. "$ROOT/data/text-sources.sh"

"$ROOT/data/fetch.sh" --only pawiki --only pawikisource --only aksharantar-pan
if [ ! -d "$RAW/aksharantar-pan/extracted" ]; then
  unzip -q -o "$RAW/aksharantar-pan/pan.zip" -d "$RAW/aksharantar-pan/extracted"
fi
mkdir -p "$TEXT"

if [ ! -s "$TEXT/pa.txt" ]; then
  reset_sources pa
  py "$PIPELINE/extract_wiki.py" "$RAW"/pawiki/*.bz2 --out "$TEXT/pawiki.txt.tmp"
  # Paragraphs that are mostly repeated text (template-written articles) are dropped, as for
  # Telugu, Gujarati and Kannada.
  py "$PIPELINE/drop_repeats.py" "$TEXT/pawiki.txt.tmp" --out "$TEXT/pa.txt.tmp"
  rm "$TEXT/pawiki.txt.tmp"
  count_source pa pawiki "$TEXT/pa.txt.tmp"
  # Wikisource proofread text is in the Page namespace (key 250, ਪੰਨਾ, on Punjabi Wikisource), not the main one.
  py "$PIPELINE/extract_wiki.py" "$RAW"/pawikisource/*.bz2 --extra-namespaces 250,Page,ਪੰਨਾ \
    --out "$TEXT/paws.txt.tmp"
  count_source pa pawikisource "$TEXT/paws.txt.tmp"
  cat "$TEXT/paws.txt.tmp" >> "$TEXT/pa.txt.tmp"
  rm "$TEXT/paws.txt.tmp"
  mv "$TEXT/pa.txt.tmp" "$TEXT/pa.txt"
fi
echo "Punjabi text in $TEXT/pa.txt"
report_sources pa
