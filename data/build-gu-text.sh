#!/usr/bin/env bash
# Fetches the Gujarati sources in data/manifests/ (guwiki, guwikisource, guwikiquote,
# aksharantar-guj) and extracts the wiki dumps to plain text in data/raw/text/gu.txt, one
# paragraph per line. Cached: delete data/raw/text/gu.txt to re-extract. Prints how many words each
# source gave (data/text-sources.sh), and warns when one is most of the text.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RAW="$ROOT/data/raw"
TEXT="$RAW/text"
PIPELINE="$ROOT/data/pipeline"

py() { uv run --quiet --project "$PIPELINE" python -I "$@"; }
# shellcheck source=data/text-sources.sh
. "$ROOT/data/text-sources.sh"

"$ROOT/data/fetch.sh" --only guwiki --only guwikisource --only guwikiquote --only aksharantar-guj
if [ ! -d "$RAW/aksharantar-guj/extracted" ]; then
  unzip -q -o "$RAW/aksharantar-guj/guj.zip" -d "$RAW/aksharantar-guj/extracted"
fi
mkdir -p "$TEXT"

if [ ! -s "$TEXT/gu.txt" ]; then
  reset_sources gu
  py "$PIPELINE/extract_wiki.py" "$RAW"/guwiki/*.bz2 --out "$TEXT/gu.txt.tmp"
  count_source gu guwiki "$TEXT/gu.txt.tmp"
  # Wikisource proofread text is in the Page namespace (key 104 on Gujarati Wikisource), not the main one.
  py "$PIPELINE/extract_wiki.py" "$RAW"/guwikisource/*.bz2 --extra-namespaces 104,Page,પૃષ્ઠ \
    --out "$TEXT/guws.txt.tmp"
  py "$PIPELINE/extract_wiki.py" "$RAW"/guwikiquote/*.bz2 --out "$TEXT/guwq.txt.tmp"
  count_source gu guwikisource "$TEXT/guws.txt.tmp"
  count_source gu guwikiquote "$TEXT/guwq.txt.tmp"
  cat "$TEXT/guws.txt.tmp" "$TEXT/guwq.txt.tmp" >> "$TEXT/gu.txt.tmp"
  rm "$TEXT/guws.txt.tmp" "$TEXT/guwq.txt.tmp"
  mv "$TEXT/gu.txt.tmp" "$TEXT/gu.txt"
fi
echo "Gujarati text in $TEXT/gu.txt"
report_sources gu
