#!/usr/bin/env bash
# Rebuilds a pack language's lexicons in packs/<lang>.neopack/ from data/langs/<lang>/:
#   data/build-<lang>-text.sh     fetch the sources, extract data/raw/text/<lang>.txt
#   lexbuild-native               <lang>.fst, next.fst (+ the Malayalam pack's English pairs)
#   mlkb.langs.format1            romanisation.json (format 1) from romanisation-weights.json
#   mlkb.langs.romanised_keys     the romanised lookup keys, by the format-1 walk and skeleton
#   lexbuild-native               romanised.fst
# then copies romanisation.json into the pack and rewrites its checksums. The weights are
# fitted by hand-run `python -m mlkb.langs.calibrate --lang <lang> --write`.
#
#   data/build-pack-lexicons.sh as
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"

LANG_ID="${1:?usage: $0 <lang>}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RAW="$ROOT/data/raw"
CONFIG="$ROOT/data/langs/$LANG_ID"
PACK="$ROOT/packs/$LANG_ID.neopack"
BUNDLE="$RAW/bundle-$LANG_ID"
# The English pairs of next.fst come from a pack built against the same en.fst.
ENGLISH_NEXT="${ENGLISH_NEXT:-$ROOT/packs/ml.neopack/next.fst}"

"$ROOT/data/build-$LANG_ID-text.sh"

(cd "$ROOT/engine" && cargo build --release -p lexbuild)
LEXBUILD="$ROOT/engine/target/release/lexbuild-native"
rm -rf "$BUNDLE"
"$LEXBUILD" --lang-config "$CONFIG/lang.json" --native "$RAW/text/$LANG_ID.txt" \
  --english-next "$ENGLISH_NEXT" --out "$BUNDLE"
(cd "$ROOT/training" && uv run --quiet python -m mlkb.langs.format1 --lang "$LANG_ID")
(cd "$ROOT/training" && uv run --quiet python -m mlkb.langs.romanised_keys --lang "$LANG_ID" \
  --words "$BUNDLE/words.tsv" --out "$BUNDLE/romanised-keys.tsv")
"$LEXBUILD" --lang-config "$CONFIG/lang.json" --romanised-keys "$BUNDLE/romanised-keys.tsv" --out "$BUNDLE"

mkdir -p "$PACK"
cp "$BUNDLE/$LANG_ID.fst" "$BUNDLE/next.fst" "$BUNDLE/romanised.fst" "$PACK/"
cp "$CONFIG/romanisation.json" "$PACK/"
cp "$BUNDLE/stats.txt" "$CONFIG/lexicon-stats.txt"
python3 -I "$ROOT/scripts/pack.py" "$PACK" --write
echo "Lexicons written to $PACK; stats in data/langs/$LANG_ID/lexicon-stats.txt"
