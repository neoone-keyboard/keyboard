#!/usr/bin/env bash
# Rebuilds a pack language's lexicons in packs/<lang>.neopack/, then the pack's checksums. For
# languages whose romanisation is data (packs/<lang>.neopack/romanisation.json); Malayalam is
# built by data/build-lexicons.sh. Two kinds of language config, picked by which one exists:
#
# data/lang/<lang>.conf (hi): the wiki dumps it lists are fetched and extracted, then
#   lexbuild --lang   <lang>.fst, the romanised lexicon (keyed by the pack's romanisation.json)
#                     and next.fst; English counts against the base's en.fst, so run
#                     data/build-lexicons.sh first if en.fst changed.
#   Stats go to data/lexicon-stats-<lang>.txt.
#
# data/langs/<lang>/lang.json (as):
#   data/build-<lang>-text.sh     fetch the sources, extract data/raw/text/<lang>.txt
#   lexbuild-native               <lang>.fst, next.fst (+ the Malayalam pack's English pairs)
#   mlkb.langs.format1            romanisation.json (format 1) from romanisation-weights.json
#   mlkb.langs.romanised_keys     the romanised lookup keys, by the format-1 walk and skeleton
#   lexbuild-native               romanised.fst
#   then copies romanisation.json into the pack. The weights are fitted by hand-run
#   `python -m mlkb.langs.calibrate --lang <lang> --write`. Stats go to
#   data/langs/<lang>/lexicon-stats.txt: the builder's counts, the most frequent tokens it
#   dropped as not words, and the words each source gave (data/text-sources.sh).
#
#   data/build-pack-lexicons.sh hi
#   data/build-pack-lexicons.sh as
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
BUNDLE="$RAW/bundle-$LANG_ID"

py() { uv run --quiet --project "$PIPELINE" python -I "$@"; }

# data/lang/<lang>.conf: sources listed in the conf, lexicons by `lexbuild --lang`.
build_conf() {
  local conf="$ROOT/data/lang/$LANG_ID.conf"
  local english_fst="$ROOT/apps/ios/KeyboardExtension/Lexicon/en.fst"
  WIKIS=""
  ROMANISED=""
  # shellcheck source=/dev/null
  source "$conf"

  local ids=()
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
  rm -rf "$BUNDLE"
  "$ROOT/engine/target/release/lexbuild" \
    --english "$TEXT/en.txt" --english-fst "$english_fst" \
    --lang "$LANG_ID" --native "$TEXT/$LANG_ID.txt" \
    --romanisation "$PACK/romanisation.json" --romanised-name "$ROMANISED" \
    --out "$BUNDLE"

  mv "$BUNDLE/$LANG_ID.fst" "$BUNDLE/$ROMANISED.fst" "$BUNDLE/next.fst" "$PACK/"
  mv "$BUNDLE/stats.txt" "$ROOT/data/lexicon-stats-$LANG_ID.txt"
  python3 -I "$ROOT/scripts/pack.py" "$PACK" --write
  echo "Lexicons written to $PACK; stats in data/lexicon-stats-$LANG_ID.txt"
}

# data/langs/<lang>/lang.json: lexicons by `lexbuild-native` and the tables in data/langs/<lang>.
build_lang_config() {
  local config="$ROOT/data/langs/$LANG_ID"
  # The English pairs of next.fst come from a pack built against the same en.fst.
  local english_next="${ENGLISH_NEXT:-$ROOT/packs/ml.neopack/next.fst}"
  local lexbuild="$ROOT/engine/target/release/lexbuild-native"

  # What the build needs from earlier steps of docs/make-a-language.md, checked before the
  # minutes of work that would otherwise end in a traceback.
  local missing=()
  [ -x "$ROOT/data/build-$LANG_ID-text.sh" ] ||
    missing+=("data/build-$LANG_ID-text.sh, executable (step 3, Get the text)")
  [ -f "$config/romanisation-weights.json" ] ||
    missing+=("data/langs/$LANG_ID/romanisation-weights.json (step 4: calibrate --write, or a copy of romanisation.hand.json)")
  [ -f "$PACK/manifest.json" ] ||
    missing+=("packs/$LANG_ID.neopack/manifest.json (step 5, The manifest)")
  if [ ${#missing[@]} -gt 0 ]; then
    echo "build-pack-lexicons.sh $LANG_ID: missing" >&2
    printf '  %s\n' "${missing[@]}" >&2
    exit 1
  fi

  "$ROOT/data/build-$LANG_ID-text.sh"

  (cd "$ROOT/engine" && cargo build --release -p lexbuild)
  rm -rf "$BUNDLE"
  "$lexbuild" --lang-config "$config/lang.json" --native "$TEXT/$LANG_ID.txt" \
    --english-next "$english_next" --out "$BUNDLE"
  (cd "$ROOT/training" && uv run --quiet python -m mlkb.langs.format1 --lang "$LANG_ID")
  (cd "$ROOT/training" && uv run --quiet python -m mlkb.langs.romanised_keys --lang "$LANG_ID" \
    --words "$BUNDLE/words.tsv" --out "$BUNDLE/romanised-keys.tsv")
  "$lexbuild" --lang-config "$config/lang.json" --romanised-keys "$BUNDLE/romanised-keys.tsv" --out "$BUNDLE"

  mkdir -p "$PACK"
  cp "$BUNDLE/$LANG_ID.fst" "$BUNDLE/next.fst" "$BUNDLE/romanised.fst" "$PACK/"
  cp "$config/romanisation.json" "$PACK/"
  # Tables that give some words a second key (optionalFinals, dropTrailing.alsoKept): the keys
  # above are one per word, so the romanised lexicon is rebuilt from the tables, which adds them.
  if grep -qE '"optionalFinals"|"alsoKept": *true' "$PACK/romanisation.json"; then
    "$ROOT/engine/target/release/lexbuild-rekey" "$PACK/romanisation.json" "$PACK/$LANG_ID.fst" "$PACK/romanised.fst"
  fi
  cp "$BUNDLE/stats.txt" "$config/lexicon-stats.txt"
  # shellcheck source=data/text-sources.sh
  . "$ROOT/data/text-sources.sh"
  source_words "$LANG_ID" >> "$config/lexicon-stats.txt"
  python3 -I "$ROOT/scripts/pack.py" "$PACK" --write
  echo "Lexicons written to $PACK; stats in data/langs/$LANG_ID/lexicon-stats.txt"
}

if [ -f "$ROOT/data/lang/$LANG_ID.conf" ]; then
  build_conf
elif [ -f "$ROOT/data/langs/$LANG_ID/lang.json" ]; then
  build_lang_config
else
  echo "build-pack-lexicons.sh: no data/lang/$LANG_ID.conf or data/langs/$LANG_ID/lang.json" >&2
  exit 1
fi
