#!/usr/bin/env bash
# Rebuilds a pack's lexicons: <lang>.fst, the romanised lexicon and next.fst in
# packs/<lang>.neopack/, then the pack's checksums. For languages whose romanisation is data
# (packs/<lang>.neopack/romanisation.json) and whose sources are listed in data/lang/<lang>.conf.
# Malayalam is built by data/build-lexicons.sh. English counts against the base's en.fst, so
# run that first if en.fst changed.
#
#   data/build-pack-lexicons.sh hi
#
# Extracted text is cached in data/raw/text/<lang>.txt; delete it to re-extract.
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"

LANG_ID="${1:?usage: build-pack-lexicons.sh <lang>}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RAW="$ROOT/data/raw"
TEXT="$RAW/text"
PIPELINE="$ROOT/data/pipeline"
PACK="$ROOT/packs/$LANG_ID.neopack"
CONF="$ROOT/data/lang/$LANG_ID.conf"
ENGLISH_FST="$ROOT/apps/ios/KeyboardExtension/Lexicon/en.fst"
WIKIS=""
ROMANISED=""
# shellcheck source=/dev/null
source "$CONF"

py() { uv run --quiet --project "$PIPELINE" python -I "$@"; }

ids=()
for wiki in $WIKIS; do ids+=(--only "${wiki%%:*}"); done
uv run --quiet --project "$PIPELINE" python -I "$PIPELINE/fetch.py" \
  --manifests "$ROOT/data/manifests" --raw "$RAW" "${ids[@]}"
mkdir -p "$TEXT"

# Each step writes to a .tmp file first, so an interrupted run is redone, not trusted.
if [ ! -s "$TEXT/$LANG_ID.txt" ]; then
  : > "$TEXT/$LANG_ID.txt.tmp"
  for wiki in $WIKIS; do
    id="${wiki%%:*}"
    extra=""
    [ "$id" != "$wiki" ] && extra="${wiki#*:}"
    py "$PIPELINE/extract_wiki.py" "$RAW/$id"/*.bz2 --extra-namespaces "$extra" --out "$TEXT/$id.txt.tmp"
    cat "$TEXT/$id.txt.tmp" >> "$TEXT/$LANG_ID.txt.tmp"
    rm "$TEXT/$id.txt.tmp"
  done
  mv "$TEXT/$LANG_ID.txt.tmp" "$TEXT/$LANG_ID.txt"
fi

(cd "$ROOT/engine" && cargo build --release -p lexbuild)
rm -rf "$RAW/bundle-$LANG_ID"
"$ROOT/engine/target/release/lexbuild" \
  --english "$TEXT/en.txt" --english-fst "$ENGLISH_FST" \
  --lang "$LANG_ID" --native "$TEXT/$LANG_ID.txt" \
  --romanisation "$PACK/romanisation.json" --romanised-name "$ROMANISED" \
  --out "$RAW/bundle-$LANG_ID"

mv "$RAW/bundle-$LANG_ID/$LANG_ID.fst" "$RAW/bundle-$LANG_ID/$ROMANISED.fst" "$RAW/bundle-$LANG_ID/next.fst" "$PACK/"
mv "$RAW/bundle-$LANG_ID/stats.txt" "$ROOT/data/lexicon-stats-$LANG_ID.txt"
python3 -I "$ROOT/scripts/pack.py" "$PACK" --write
echo "Lexicons written to $PACK; stats in data/lexicon-stats-$LANG_ID.txt"
