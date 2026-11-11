#!/usr/bin/env bash
# Fetches the Bengali sources in data/manifests/ (bnwiki, bnwikisource, bnwikiquote,
# aksharantar-ben) and extracts the wiki dumps to plain text in data/raw/text/bn.txt, one
# paragraph per line. Cached: delete data/raw/text/bn.txt to re-extract. Prints how many words each
# source gave (data/text-sources.sh), and warns when one is most of the text.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RAW="$ROOT/data/raw"
TEXT="$RAW/text"
PIPELINE="$ROOT/data/pipeline"

py() { uv run --quiet --project "$PIPELINE" python -I "$@"; }
# shellcheck source=data/text-sources.sh
. "$ROOT/data/text-sources.sh"

"$ROOT/data/fetch.sh" --only bnwiki --only bnwikisource --only bnwikiquote --only aksharantar-ben
if [ ! -d "$RAW/aksharantar-ben/extracted" ]; then
  unzip -q -o "$RAW/aksharantar-ben/ben.zip" -d "$RAW/aksharantar-ben/extracted"
fi
mkdir -p "$TEXT"

if [ ! -s "$TEXT/bn.txt" ]; then
  reset_sources bn
  py "$PIPELINE/extract_wiki.py" "$RAW"/bnwiki/*.bz2 --out "$TEXT/bn.txt.tmp"
  count_source bn bnwiki "$TEXT/bn.txt.tmp"
  # Wikisource proofread text is in the Page namespace (key 104, পাতা), not the main one.
  py "$PIPELINE/extract_wiki.py" "$RAW"/bnwikisource/*.bz2 --extra-namespaces 104,Page,পাতা \
    --out "$TEXT/bnws.txt.tmp"
  # Wikisource (older literature, much of it in the literary register) is larger than
  # Wikipedia; every third paragraph keeps it well under half of the text
  # (docs/make-a-language.md, step 3).
  awk 'NR % 3 == 1' "$TEXT/bnws.txt.tmp" > "$TEXT/bnws.thin.tmp" && mv "$TEXT/bnws.thin.tmp" "$TEXT/bnws.txt.tmp"
  py "$PIPELINE/extract_wiki.py" "$RAW"/bnwikiquote/*.bz2 --out "$TEXT/bnwq.txt.tmp"
  count_source bn bnwikisource "$TEXT/bnws.txt.tmp"
  count_source bn bnwikiquote "$TEXT/bnwq.txt.tmp"
  cat "$TEXT/bnws.txt.tmp" "$TEXT/bnwq.txt.tmp" >> "$TEXT/bn.txt.tmp"
  rm "$TEXT/bnws.txt.tmp" "$TEXT/bnwq.txt.tmp"
  mv "$TEXT/bn.txt.tmp" "$TEXT/bn.txt"
fi
echo "Bengali text in $TEXT/bn.txt"
report_sources bn
