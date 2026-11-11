#!/usr/bin/env bash
# Rebuilds the emoji panel and emoji suggestion files in apps/ios/KeyboardExtension/Emoji/
# from Unicode's emoji-test.txt and the CLDR annotations (data/manifests/unicode-emoji.json
# and cldr-annotations.json; about 1.5 MB to download): English into the app's files, and a
# pack language's words into data/langs/<id>/ (the pack's emojiWords and emojiSearch roles,
# which scripts/pack.py --write copies into the pack). Malayalam's also go to
# Emoji/packs/ml/, the app's copy for a Malayalam pack published before it carried them.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RAW="$ROOT/data/raw"

"$ROOT/data/fetch.sh" --only unicode-emoji --only cldr-annotations
# Every annotation file of the manifest but English's is a pack language's: <id>.xml and
# derived-<id>.xml (CLDR's locale code is the pack id). Its words go where its other sources
# are, data/langs/<id>/; add a language by listing its two files in
# data/manifests/cldr-annotations.json.
packs=()
for xml in "$RAW"/cldr-annotations/*.xml; do
  id="$(basename "$xml" .xml)"
  case "$id" in en | derived-*) continue ;; esac
  packs+=(--pack-annotations "$id=$xml")
  if [ -f "$RAW/cldr-annotations/derived-$id.xml" ]; then
    packs+=(--pack-annotations "$id=$RAW/cldr-annotations/derived-$id.xml")
  fi
  packs+=(--pack-out "$id=$ROOT/data/langs/$id")
  # The app's own copy, for a pack of the language published before packs carried the words.
  if [ -d "$ROOT/apps/ios/KeyboardExtension/Emoji/packs/$id" ]; then
    packs+=(--pack-out "$id=$ROOT/apps/ios/KeyboardExtension/Emoji/packs/$id")
  fi
done
uv run --quiet --project "$ROOT/data/pipeline" python -I "$ROOT/data/pipeline/emoji_data.py" \
  --emoji-test "$RAW/unicode-emoji/emoji-test.txt" \
  --annotations "$RAW/cldr-annotations/en.xml" --annotations "$RAW/cldr-annotations/derived-en.xml" \
  --out "$ROOT/apps/ios/KeyboardExtension/Emoji" "${packs[@]}"
