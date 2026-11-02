#!/bin/sh
# Lists the third-party Rust crates that mlengine-ffi (the shipped static
# library) depends on, with their licences, as "name<TAB>licence" lines.
#
#   scripts/shipped-crates.sh           print the list
#   scripts/shipped-crates.sh --check   fail if engine/shipped-crates.txt is stale
#
# engine/shipped-crates.txt is checked in; Credits.software must cover every line
# (CreditsTests). When this check fails: run the script > engine/shipped-crates.txt,
# then add the new crates to Credits.swift.
set -eu
cd "$(dirname "$0")/../engine"
export PATH="$HOME/.cargo/bin:$PATH"

list() {
  cargo tree -p mlengine-ffi -e normal --prefix none \
    | grep -v '^mlengine-' | awk '{print $1}' | sort -u > "${TMPDIR:-/tmp}/shipped-names.$$"
  cargo metadata --format-version 1 --locked | python3 -I -c '
import json, sys
names = set(open(sys.argv[1]).read().split())
m = json.load(sys.stdin)
rows = {(p["name"], p["license"] or "UNKNOWN") for p in m["packages"] if p["name"] in names}
for name, lic in sorted(rows):
    print(f"{name}\t{lic}")
' "${TMPDIR:-/tmp}/shipped-names.$$"
  rm -f "${TMPDIR:-/tmp}/shipped-names.$$"
}

if [ "${1:-}" = "--check" ]; then
  list | diff -u shipped-crates.txt - >&2 \
    || { echo "engine/shipped-crates.txt is stale: regenerate it and update Credits.software" >&2; exit 1; }
else
  list
fi
