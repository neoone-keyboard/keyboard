#!/usr/bin/env bash
# The keyboard extension's debug-only extra packs (M6 phase 2, task 12). Run by Xcode as the
# extension's last build phase. The bundle always has packs/ml.neopack (project.yml); with
# ML_DEBUG_PACKS set (space-separated pack ids, "hi") in a Debug build, packs/<id>.neopack of
# each is copied in beside it. Otherwise any other pack left in the bundle by an earlier build
# is removed, so the normal build is English and Malayalam only.
#
#   make twopacks   (ML_DEBUG_PACKS=hi ML_ACTIVE_LANGUAGES=ml,hi, its own DerivedData)
set -euo pipefail

dest="${TARGET_BUILD_DIR:?}/${UNLOCALIZED_RESOURCES_FOLDER_PATH:?}/packs"
src="${SRCROOT:?}/../../packs"
wanted=" ml ${ML_DEBUG_PACKS:-} "
if [ "${CONFIGURATION:-}" != "Debug" ]; then
  wanted=" ml "
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
  [ "$id" = ml ] && continue
  if [ ! -f "$src/$id.neopack/manifest.json" ]; then
    echo "error: debug-packs: no pack $src/$id.neopack" >&2
    exit 1
  fi
  echo "debug-packs: bundling $id.neopack"
  rm -rf "${dest:?}/$id.neopack"
  cp -R "$src/$id.neopack" "$dest/"
done
