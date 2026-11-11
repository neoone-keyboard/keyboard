#!/usr/bin/env bash
# Fetches the Maithili sources in data/manifests/ (maiwiki, aksharantar-mai) and extracts the
# wiki dump to plain text in data/raw/text/mai.txt, one paragraph per line. Maithili has no
# Wikisource or Wikiquote of its own. Cached: delete data/raw/text/mai.txt to re-extract. Prints
# how many words each source gave (data/text-sources.sh).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RAW="$ROOT/data/raw"
TEXT="$RAW/text"
PIPELINE="$ROOT/data/pipeline"

py() { uv run --quiet --project "$PIPELINE" python -I "$@"; }
# shellcheck source=data/text-sources.sh
. "$ROOT/data/text-sources.sh"

"$ROOT/data/fetch.sh" --only maiwiki --only aksharantar-mai
if [ ! -d "$RAW/aksharantar-mai/extracted" ]; then
  unzip -q -o "$RAW/aksharantar-mai/mai.zip" -d "$RAW/aksharantar-mai/extracted"
fi
mkdir -p "$TEXT"

if [ ! -s "$TEXT/mai.txt" ]; then
  reset_sources mai
  py "$PIPELINE/extract_wiki.py" "$RAW"/maiwiki/*.bz2 --out "$TEXT/mai.txt.tmp"
  count_source mai maiwiki "$TEXT/mai.txt.tmp"
  mv "$TEXT/mai.txt.tmp" "$TEXT/mai.txt"
fi
echo "Maithili text in $TEXT/mai.txt"
report_sources mai
