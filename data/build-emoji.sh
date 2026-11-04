#!/usr/bin/env bash
# Rebuilds the emoji panel and emoji suggestion files in apps/ios/KeyboardExtension/Emoji/
# from Unicode's emoji-test.txt and the CLDR annotations (data/manifests/unicode-emoji.json
# and cldr-annotations.json; about 1.5 MB to download).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RAW="$ROOT/data/raw"

"$ROOT/data/fetch.sh" --only unicode-emoji --only cldr-annotations
uv run --quiet --project "$ROOT/data/pipeline" python -I "$ROOT/data/pipeline/emoji_data.py" \
  --emoji-test "$RAW/unicode-emoji/emoji-test.txt" \
  --annotations "$RAW/cldr-annotations/en.xml" --annotations "$RAW/cldr-annotations/ml.xml" \
  --annotations "$RAW/cldr-annotations/derived-en.xml" --annotations "$RAW/cldr-annotations/derived-ml.xml" \
  --out "$ROOT/apps/ios/KeyboardExtension/Emoji"
