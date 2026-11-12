#!/usr/bin/env bash
# Fetches the Odia sources in data/manifests/ (orwiki, orwikisource, aksharantar-ori) and
# extracts the wiki dumps to plain text in data/raw/text/or.txt, one paragraph per line;
# Wikipedia's paragraphs that are mostly repeated text are dropped (drop_repeats.py). Odia has
# no Wikiquote. Cached: delete data/raw/text/or.txt to re-extract. Prints how many words each
# source gave (data/text-sources.sh), and warns when one is most of the text.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RAW="$ROOT/data/raw"
TEXT="$RAW/text"
PIPELINE="$ROOT/data/pipeline"

py() { uv run --quiet --project "$PIPELINE" python -I "$@"; }
# shellcheck source=data/text-sources.sh
. "$ROOT/data/text-sources.sh"

"$ROOT/data/fetch.sh" --only orwiki --only orwikisource --only aksharantar-ori
if [ ! -d "$RAW/aksharantar-ori/extracted" ]; then
  unzip -q -o "$RAW/aksharantar-ori/ori.zip" -d "$RAW/aksharantar-ori/extracted"
fi
mkdir -p "$TEXT"

if [ ! -s "$TEXT/or.txt" ]; then
  reset_sources or
  py "$PIPELINE/extract_wiki.py" "$RAW"/orwiki/*.bz2 --out "$TEXT/orwiki.txt.tmp"
  # Paragraphs that are mostly repeated text (template-written articles) are dropped, as for
  # Telugu, Gujarati and Kannada.
  py "$PIPELINE/drop_repeats.py" "$TEXT/orwiki.txt.tmp" --out "$TEXT/or.txt.tmp"
  rm "$TEXT/orwiki.txt.tmp"
  count_source or orwiki "$TEXT/or.txt.tmp"
  # Wikisource proofread text is in the Page namespace (key 250, ପୃଷ୍ଠା, on Odia Wikisource), not the main one.
  py "$PIPELINE/extract_wiki.py" "$RAW"/orwikisource/*.bz2 --extra-namespaces 250,Page,ପୃଷ୍ଠା \
    --out "$TEXT/orws.txt.tmp"
  count_source or orwikisource "$TEXT/orws.txt.tmp"
  cat "$TEXT/orws.txt.tmp" >> "$TEXT/or.txt.tmp"
  rm "$TEXT/orws.txt.tmp"
  mv "$TEXT/or.txt.tmp" "$TEXT/or.txt"
fi
echo "Odia text in $TEXT/or.txt"
report_sources or
