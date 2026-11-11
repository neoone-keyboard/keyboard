#!/usr/bin/env bash
# Fetches the Urdu sources in data/manifests/ (urwiki, urwikisource, urwikiquote,
# aksharantar-urd) and extracts the wiki dumps to plain text in data/raw/text/ur.txt, one
# paragraph per line. Wikipedia's bot-written articles are dropped (drop_repeats.py); then
# urdu_text.py folds presentation forms by NFKC, applies data/langs/ur/lang.json's replacements
# and cuts words typed without a space between them (کےساتھ to کے ساتھ). Cached: delete
# data/raw/text/ur.txt to re-extract. Prints how many words each source gave
# (data/text-sources.sh), and warns when one is most of the text.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RAW="$ROOT/data/raw"
TEXT="$RAW/text"
PIPELINE="$ROOT/data/pipeline"

py() { uv run --quiet --project "$PIPELINE" python -I "$@"; }
# shellcheck source=data/text-sources.sh
. "$ROOT/data/text-sources.sh"

"$ROOT/data/fetch.sh" --only urwiki --only urwikisource --only urwikiquote --only aksharantar-urd
if [ ! -d "$RAW/aksharantar-urd/extracted" ]; then
  unzip -q -o "$RAW/aksharantar-urd/urd.zip" -d "$RAW/aksharantar-urd/extracted"
fi
mkdir -p "$TEXT"

if [ ! -s "$TEXT/ur.txt" ]; then
  reset_sources ur
  py "$PIPELINE/extract_wiki.py" "$RAW"/urwiki/*.bz2 --out "$TEXT/urwiki.txt.tmp"
  # Urdu Wikipedia has thousands of articles written by a bot from one template (places,
  # stations); whole, their words would lead the lexicon. Paragraphs that are mostly repeated
  # text are dropped.
  py "$PIPELINE/drop_repeats.py" "$TEXT/urwiki.txt.tmp" --out "$TEXT/ur-raw.txt.tmp"
  rm "$TEXT/urwiki.txt.tmp"
  count_source ur urwiki "$TEXT/ur-raw.txt.tmp"
  # Wikisource proofread text is in the Page namespace (key 250 on Urdu Wikisource, صفحہ).
  py "$PIPELINE/extract_wiki.py" "$RAW"/urwikisource/*.bz2 --extra-namespaces 250,Page,صفحہ \
    --out "$TEXT/urws.txt.tmp"
  py "$PIPELINE/extract_wiki.py" "$RAW"/urwikiquote/*.bz2 --out "$TEXT/urwq.txt.tmp"
  count_source ur urwikisource "$TEXT/urws.txt.tmp"
  count_source ur urwikiquote "$TEXT/urwq.txt.tmp"
  cat "$TEXT/urws.txt.tmp" "$TEXT/urwq.txt.tmp" >> "$TEXT/ur-raw.txt.tmp"
  rm "$TEXT/urws.txt.tmp" "$TEXT/urwq.txt.tmp"
  py "$PIPELINE/urdu_text.py" "$TEXT/ur-raw.txt.tmp" --lang "$ROOT/data/langs/ur/lang.json" \
    --out "$TEXT/ur.txt.tmp"
  rm "$TEXT/ur-raw.txt.tmp"
  mv "$TEXT/ur.txt.tmp" "$TEXT/ur.txt"
fi
echo "Urdu text in $TEXT/ur.txt"
report_sources ur
