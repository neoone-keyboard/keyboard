#!/usr/bin/env bash
# Downloads every source in data/manifests/ into data/raw/ (git-ignored) and
# verifies each checksum. About 3 GB. Runs on the Mac only, never in the app.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
uv run --quiet --project "$ROOT/data/pipeline" python -I "$ROOT/data/pipeline/fetch.py" \
  --manifests "$ROOT/data/manifests" --raw "$ROOT/data/raw" "$@"
