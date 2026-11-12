#!/usr/bin/env bash
# Fetches the Nepali sources in data/manifests/ (newiki, newikibooks, aksharantar-nep) and
# extracts the wiki dumps to plain text in data/raw/text/ne.txt, one paragraph per line;
# Wikipedia's paragraphs that are mostly repeated text are dropped (drop_repeats.py). Nepali has
# no Wikisource or Wikiquote of its own, so Wikibooks is the second source. Cached: delete
# data/raw/text/ne.txt to re-extract. Prints how many words each source gave
# (data/text-sources.sh), and warns when one is most of the text.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RAW="$ROOT/data/raw"
TEXT="$RAW/text"
PIPELINE="$ROOT/data/pipeline"

py() { uv run --quiet --project "$PIPELINE" python -I "$@"; }
# shellcheck source=data/text-sources.sh
. "$ROOT/data/text-sources.sh"

"$ROOT/data/fetch.sh" --only newiki --only newikibooks --only aksharantar-nep
if [ ! -d "$RAW/aksharantar-nep/extracted" ]; then
  unzip -q -o "$RAW/aksharantar-nep/nep.zip" -d "$RAW/aksharantar-nep/extracted"
fi
mkdir -p "$TEXT"

if [ ! -s "$TEXT/ne.txt" ]; then
  reset_sources ne
  py "$PIPELINE/extract_wiki.py" "$RAW"/newiki/*.bz2 --out "$TEXT/newiki.txt.tmp"
  # Paragraphs that are mostly repeated text (template-written articles) are dropped, as for
  # Telugu, Gujarati, Kannada, Odia and Punjabi.
  py "$PIPELINE/drop_repeats.py" "$TEXT/newiki.txt.tmp" --out "$TEXT/ne.txt.tmp"
  rm "$TEXT/newiki.txt.tmp"
  count_source ne newiki "$TEXT/ne.txt.tmp"
  py "$PIPELINE/extract_wiki.py" "$RAW"/newikibooks/*.bz2 --out "$TEXT/newb.txt.tmp"
  count_source ne newikibooks "$TEXT/newb.txt.tmp"
  cat "$TEXT/newb.txt.tmp" >> "$TEXT/ne.txt.tmp"
  rm "$TEXT/newb.txt.tmp"
  mv "$TEXT/ne.txt.tmp" "$TEXT/ne.txt"
fi
echo "Nepali text in $TEXT/ne.txt"
report_sources ne
