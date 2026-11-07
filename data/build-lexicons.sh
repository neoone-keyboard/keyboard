#!/usr/bin/env bash
# Rebuilds en.fst and en-next.fst (the English base's own next words) in
# apps/ios/KeyboardExtension/Lexicon/, and ml.fst, manglish.fst and
# next.fst in the Malayalam pack (packs/ml.neopack/), then the pack's checksums, from the
# sources in data/manifests/: fetch, extract plain text, run lexbuild.
# Extracted text is cached in data/raw/text/; delete it to re-extract.
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RAW="$ROOT/data/raw"
TEXT="$RAW/text"
PIPELINE="$ROOT/data/pipeline"
OUT="$ROOT/apps/ios/KeyboardExtension/Lexicon"
PACK="$ROOT/packs/ml.neopack"
ENGLISH_WORDS=500000000

py() { uv run --quiet --project "$PIPELINE" python -I "$@"; }

"$ROOT/data/fetch.sh"
mkdir -p "$TEXT"

# Each step writes to a .tmp file first, so an interrupted run is redone, not trusted.
if [ ! -s "$TEXT/en.txt" ]; then
  py "$PIPELINE/extract_wiki.py" "$RAW"/enwiki/*.bz2 --max-words "$ENGLISH_WORDS" --out "$TEXT/en.txt.tmp"
  mv "$TEXT/en.txt.tmp" "$TEXT/en.txt"
fi
if [ ! -s "$TEXT/ml.txt" ]; then
  py "$PIPELINE/extract_wiki.py" "$RAW"/mlwiki/*.bz2 --out "$TEXT/ml.txt.tmp"
  # Wikisource proofread text is in the Page namespace (key 104), not the main one.
  py "$PIPELINE/extract_wiki.py" "$RAW"/mlwikisource/*.bz2 --extra-namespaces 104,Page,താൾ --out "$TEXT/mlws.txt.tmp"
  cat "$TEXT/mlws.txt.tmp" >> "$TEXT/ml.txt.tmp"
  rm "$TEXT/mlws.txt.tmp"
  mv "$TEXT/ml.txt.tmp" "$TEXT/ml.txt"
fi
if [ ! -s "$TEXT/scowl.txt" ]; then
  py "$PIPELINE/scowl_words.py" "$RAW/scowl/scowl-2020.12.07.tar.gz" --out "$TEXT/scowl.txt.tmp"
  mv "$TEXT/scowl.txt.tmp" "$TEXT/scowl.txt"
fi

smc=()
for f in "$RAW"/smc-words/*.txt; do smc+=(--smc "$f"); done

(cd "$ROOT/engine" && cargo build --release -p lexbuild)
rm -rf "$RAW/bundle"
"$ROOT/engine/target/release/lexbuild" \
  --english "$TEXT/en.txt" --malayalam "$TEXT/ml.txt" --scowl "$TEXT/scowl.txt" "${smc[@]}" \
  --english-list "$ROOT/data/indian-english.txt" \
  --romanisation "$PACK/romanisation.json" \
  --out "$RAW/bundle"

mkdir -p "$OUT" "$PACK"
mv "$RAW/bundle/en.fst" "$RAW/bundle/en-next.fst" "$OUT/"
mv "$RAW/bundle/ml.fst" "$RAW/bundle/manglish.fst" "$RAW/bundle/next.fst" "$PACK/"
mv "$RAW/bundle/stats.txt" "$ROOT/data/lexicon-stats.txt"
python3 -I "$ROOT/scripts/pack.py" "$PACK" --write
echo "Lexicons written to $OUT and $PACK; stats in data/lexicon-stats.txt"
