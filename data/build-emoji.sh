#!/usr/bin/env bash
# Rebuilds the emoji panel and emoji suggestion files from Unicode's emoji-test.txt and the
# CLDR annotations (data/manifests/unicode-emoji.json and cldr-annotations.json; about 13 MB
# to download): English into the app's files in apps/ios/KeyboardExtension/Emoji/, and a pack
# language's words into data/langs/<id>/ (the pack's emojiWords and emojiSearch roles, which
# scripts/pack.py --write copies into the pack). Malayalam's also go to Emoji/packs/ml/, the
# app's copy for a Malayalam pack published before it carried them.
#
#   data/build-emoji.sh [PACK_ID ...]
#
# The pack languages are the "packs" of data/manifests/cldr-annotations.json: pack id -> CLDR
# locale, whose files the manifest lists as <locale>.xml and derived-<locale>.xml. Built are
# those that already have their two files in data/langs/<id>/ and those named here, so a
# language's first build names it (make emoji EMOJI_PACKS="as ta") and later ones need not.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RAW="$ROOT/data/raw"
MANIFEST="$ROOT/data/manifests/cldr-annotations.json"

# "id locale" per line, for every pack language of the manifest.
known="$(python3 -I -c '
import json, sys
for pack, locale in sorted(json.load(open(sys.argv[1], encoding="utf-8"))["packs"].items()):
    print(pack, locale)' "$MANIFEST")"
for id in "$@"; do
  if ! grep -q "^$id " <<<"$known"; then
    echo "build-emoji: no pack \"$id\" in the \"packs\" of $MANIFEST." >&2
    echo "  Add it there with its CLDR locale, and the locale's two files to \"files\"" >&2
    echo "  (docs/make-a-language.md, step 8b). It has: $(cut -d' ' -f1 <<<"$known" | tr '\n' ' ')" >&2
    exit 1
  fi
done

"$ROOT/data/fetch.sh" --only unicode-emoji --only cldr-annotations
packs=()
while read -r id locale; do
  named=no
  for arg in "$@"; do
    if [ "$arg" = "$id" ]; then named=yes; fi
  done
  if [ "$named" = no ] && [ ! -f "$ROOT/data/langs/$id/emoji-words.txt" ]; then continue; fi
  packs+=(--pack-annotations "$id=$RAW/cldr-annotations/$locale.xml")
  packs+=(--pack-annotations "$id=$RAW/cldr-annotations/derived-$locale.xml")
  packs+=(--pack-out "$id=$ROOT/data/langs/$id")
  # The app's own copy, for a pack of the language published before packs carried the words.
  if [ -d "$ROOT/apps/ios/KeyboardExtension/Emoji/packs/$id" ]; then
    packs+=(--pack-out "$id=$ROOT/apps/ios/KeyboardExtension/Emoji/packs/$id")
  fi
done <<<"$known"
uv run --quiet --project "$ROOT/data/pipeline" python -I "$ROOT/data/pipeline/emoji_data.py" \
  --emoji-test "$RAW/unicode-emoji/emoji-test.txt" \
  --annotations "$RAW/cldr-annotations/en.xml" --annotations "$RAW/cldr-annotations/derived-en.xml" \
  --out "$ROOT/apps/ios/KeyboardExtension/Emoji" "${packs[@]}"
