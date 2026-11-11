#!/usr/bin/env bash
# Fetches the Bhojpuri sources in data/manifests/ (bhwiki, aksharantar-hin) and extracts the
# wiki dump to plain text in data/raw/text/bho.txt, one paragraph per line. Bhojpuri has no
# Wikisource or Wikiquote of its own. Cached: delete data/raw/text/bho.txt to re-extract. Prints
# how many words each source gave (data/text-sources.sh).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RAW="$ROOT/data/raw"
TEXT="$RAW/text"
PIPELINE="$ROOT/data/pipeline"

py() { uv run --quiet --project "$PIPELINE" python -I "$@"; }
# shellcheck source=data/text-sources.sh
. "$ROOT/data/text-sources.sh"

"$ROOT/data/fetch.sh" --only bhwiki --only aksharantar-hin
if [ ! -d "$RAW/aksharantar-hin/extracted" ]; then
  unzip -q -o "$RAW/aksharantar-hin/hin.zip" -d "$RAW/aksharantar-hin/extracted"
fi
mkdir -p "$TEXT"

if [ ! -s "$TEXT/bho.txt" ]; then
  reset_sources bho
  py "$PIPELINE/extract_wiki.py" "$RAW"/bhwiki/*.bz2 --out "$TEXT/bho.txt.tmp"
  count_source bho bhwiki "$TEXT/bho.txt.tmp"
  mv "$TEXT/bho.txt.tmp" "$TEXT/bho.txt"
fi
echo "Bhojpuri text in $TEXT/bho.txt"
report_sources bho
