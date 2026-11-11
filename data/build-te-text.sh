#!/usr/bin/env bash
# Fetches the Telugu sources in data/manifests/ (tewiki, tewikisource, tewikiquote,
# aksharantar-tel) and extracts the wiki dumps to plain text in data/raw/text/te.txt, one
# paragraph per line; Wikipedia's bot-written village articles are dropped (drop_repeats.py). Cached: delete data/raw/text/te.txt to re-extract. Prints how many words each
# source gave (data/text-sources.sh), and warns when one is most of the text.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RAW="$ROOT/data/raw"
TEXT="$RAW/text"
PIPELINE="$ROOT/data/pipeline"

py() { uv run --quiet --project "$PIPELINE" python -I "$@"; }
# shellcheck source=data/text-sources.sh
. "$ROOT/data/text-sources.sh"

"$ROOT/data/fetch.sh" --only tewiki --only tewikisource --only tewikiquote --only aksharantar-tel
if [ ! -d "$RAW/aksharantar-tel/extracted" ]; then
  unzip -q -o "$RAW/aksharantar-tel/tel.zip" -d "$RAW/aksharantar-tel/extracted"
fi
mkdir -p "$TEXT"

if [ ! -s "$TEXT/te.txt" ]; then
  reset_sources te
  py "$PIPELINE/extract_wiki.py" "$RAW"/tewiki/*.bz2 --out "$TEXT/tewiki.txt.tmp"
  # Telugu Wikipedia has about 30,000 village articles written by a bot from one census
  # template; whole, their words lead the lexicon (గ్రామం, దూరంలో, హెక్టార్లు) and their sentences
  # the next-word pairs. Paragraphs that are mostly repeated text are dropped.
  py "$PIPELINE/drop_repeats.py" "$TEXT/tewiki.txt.tmp" --out "$TEXT/te.txt.tmp"
  rm "$TEXT/tewiki.txt.tmp"
  count_source te tewiki "$TEXT/te.txt.tmp"
  # Wikisource proofread text is in the Page namespace (key 104 on Telugu Wikisource), not the main one.
  py "$PIPELINE/extract_wiki.py" "$RAW"/tewikisource/*.bz2 --extra-namespaces 104,Page,పుట \
    --out "$TEXT/tews.txt.tmp"
  py "$PIPELINE/extract_wiki.py" "$RAW"/tewikiquote/*.bz2 --out "$TEXT/tewq.txt.tmp"
  count_source te tewikisource "$TEXT/tews.txt.tmp"
  count_source te tewikiquote "$TEXT/tewq.txt.tmp"
  cat "$TEXT/tews.txt.tmp" "$TEXT/tewq.txt.tmp" >> "$TEXT/te.txt.tmp"
  rm "$TEXT/tews.txt.tmp" "$TEXT/tewq.txt.tmp"
  mv "$TEXT/te.txt.tmp" "$TEXT/te.txt"
fi
echo "Telugu text in $TEXT/te.txt"
report_sources te
