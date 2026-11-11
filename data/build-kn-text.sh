#!/usr/bin/env bash
# Fetches the Kannada sources in data/manifests/ (knwiki, knwikisource, knwikiquote,
# aksharantar-kan) and extracts the wiki dumps to plain text in data/raw/text/kn.txt, one
# paragraph per line; Wikipedia's paragraphs that are mostly repeated text are dropped
# (drop_repeats.py). Cached: delete data/raw/text/kn.txt to re-extract. Prints how many words
# each source gave (data/text-sources.sh), and warns when one is most of the text.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RAW="$ROOT/data/raw"
TEXT="$RAW/text"
PIPELINE="$ROOT/data/pipeline"

py() { uv run --quiet --project "$PIPELINE" python -I "$@"; }
# shellcheck source=data/text-sources.sh
. "$ROOT/data/text-sources.sh"

"$ROOT/data/fetch.sh" --only knwiki --only knwikisource --only knwikiquote --only aksharantar-kan
if [ ! -d "$RAW/aksharantar-kan/extracted" ]; then
  unzip -q -o "$RAW/aksharantar-kan/kan.zip" -d "$RAW/aksharantar-kan/extracted"
fi
mkdir -p "$TEXT"

if [ ! -s "$TEXT/kn.txt" ]; then
  reset_sources kn
  py "$PIPELINE/extract_wiki.py" "$RAW"/knwiki/*.bz2 --out "$TEXT/knwiki.txt.tmp"
  # Paragraphs that are mostly repeated text (template-written articles) are dropped, as for
  # Telugu and Gujarati. Kannada Wikipedia has few: 31,804 of 614,493 paragraphs, 364,073 of
  # its 19.9M words (2026-11-01 dump).
  py "$PIPELINE/drop_repeats.py" "$TEXT/knwiki.txt.tmp" --out "$TEXT/kn.txt.tmp"
  rm "$TEXT/knwiki.txt.tmp"
  count_source kn knwiki "$TEXT/kn.txt.tmp"
  # Wikisource proofread text is in the Page namespace (key 104, ಪುಟ, on Kannada Wikisource), not the main one.
  py "$PIPELINE/extract_wiki.py" "$RAW"/knwikisource/*.bz2 --extra-namespaces 104,Page,ಪುಟ \
    --out "$TEXT/knws.txt.tmp"
  py "$PIPELINE/extract_wiki.py" "$RAW"/knwikiquote/*.bz2 --out "$TEXT/knwq.txt.tmp"
  count_source kn knwikisource "$TEXT/knws.txt.tmp"
  count_source kn knwikiquote "$TEXT/knwq.txt.tmp"
  cat "$TEXT/knws.txt.tmp" "$TEXT/knwq.txt.tmp" >> "$TEXT/kn.txt.tmp"
  rm "$TEXT/knws.txt.tmp" "$TEXT/knwq.txt.tmp"
  mv "$TEXT/kn.txt.tmp" "$TEXT/kn.txt"
fi
echo "Kannada text in $TEXT/kn.txt"
report_sources kn
