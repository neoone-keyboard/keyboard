#!/usr/bin/env bash
# The packs bundled in the keyboard extension, for Debug builds only (M6 spec, "Where packs
# live"). Run by Xcode as the extension's last build phase. A Debug build bundles
# packs/ml.neopack, plus packs/<id>.neopack for each id in ML_DEBUG_PACKS (space-separated,
# "hi as"). A Release build bundles none: its languages are downloaded or imported in the app.
# Any pack left in the bundle by an earlier build that isn't wanted now is removed.
#
#   make twopacks   (ML_DEBUG_PACKS="hi as" ML_ACTIVE_LANGUAGES=ml,hi, its own DerivedData)
set -euo pipefail

dest="${TARGET_BUILD_DIR:?}/${UNLOCALIZED_RESOURCES_FOLDER_PATH:?}/packs"
src="${SRCROOT:?}/../../packs"
if [ "${CONFIGURATION:-}" = "Debug" ]; then
  wanted=" ml ${ML_DEBUG_PACKS:-} "
else
  wanted=" "
fi

if [ -z "${wanted// /}" ]; then
  rm -rf "$dest"
  exit 0
fi

mkdir -p "$dest"
for pack in "$dest"/*.neopack; do
  [ -e "$pack" ] || continue
  id="$(basename "$pack" .neopack)"
  case "$wanted" in
    *" $id "*) ;;
    *) echo "debug-packs: removing $id.neopack"; rm -rf "$pack" ;;
  esac
done
for id in $wanted; do
  if [ ! -f "$src/$id.neopack/manifest.json" ]; then
    echo "error: debug-packs: no pack $src/$id.neopack" >&2
    exit 1
  fi
  echo "debug-packs: bundling $id.neopack"
  # Copies only what changed, and drops files the pack no longer has.
  rsync -a --delete "$src/$id.neopack/" "$dest/$id.neopack/"
done
