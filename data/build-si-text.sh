#!/usr/bin/env bash
# Fetches the Sinhala sources in data/manifests/ (siwiki, siwikibooks) and extracts the wiki
# dumps to plain text in data/raw/text/si.txt, one paragraph per line; Wikipedia's paragraphs
# that are mostly repeated text are dropped (drop_repeats.py). Sinhala has no Wikisource,
# Wikiquote or Wikinews of its own, so Wikibooks is the second source. Also extracts Dakshina's
# Sinhala lexicon (the human pairs data/langs/si/pairs.json names) from the Dakshina tar,
# fetching the tar (2 GB) only when the lexicon isn't there yet. Cached: delete
# data/raw/text/si.txt to re-extract. Prints how many words each source gave
# (data/text-sources.sh), and warns when one is most of the text.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RAW="$ROOT/data/raw"
TEXT="$RAW/text"
PIPELINE="$ROOT/data/pipeline"

py() { uv run --quiet --project "$PIPELINE" python -I "$@"; }
# shellcheck source=data/text-sources.sh
. "$ROOT/data/text-sources.sh"

"$ROOT/data/fetch.sh" --only siwiki --only siwikibooks
LEXICONS="$RAW/dakshina/dakshina_dataset_v1.0/si/lexicons"
if [ ! -s "$LEXICONS/si.translit.sampled.test.tsv" ]; then
  "$ROOT/data/fetch.sh" --only dakshina
  tar -xf "$RAW/dakshina/dakshina_dataset_v1.0.tar" -C "$RAW/dakshina" 'dakshina_dataset_v1.0/si/lexicons/*'
fi
mkdir -p "$TEXT"

if [ ! -s "$TEXT/si.txt" ]; then
  reset_sources si
  py "$PIPELINE/extract_wiki.py" "$RAW"/siwiki/*.bz2 --out "$TEXT/siwiki.txt.tmp"
  # Paragraphs that are mostly repeated text (template-written articles) are dropped, as for
  # Telugu, Gujarati, Kannada, Odia, Punjabi and Nepali.
  py "$PIPELINE/drop_repeats.py" "$TEXT/siwiki.txt.tmp" --out "$TEXT/si.txt.tmp"
  rm "$TEXT/siwiki.txt.tmp"
  count_source si siwiki "$TEXT/si.txt.tmp"
  py "$PIPELINE/extract_wiki.py" "$RAW"/siwikibooks/*.bz2 --out "$TEXT/siwb.txt.tmp"
  count_source si siwikibooks "$TEXT/siwb.txt.tmp"
  cat "$TEXT/siwb.txt.tmp" >> "$TEXT/si.txt.tmp"
  rm "$TEXT/siwb.txt.tmp"
  mv "$TEXT/si.txt.tmp" "$TEXT/si.txt"
fi
echo "Sinhala text in $TEXT/si.txt"
report_sources si
