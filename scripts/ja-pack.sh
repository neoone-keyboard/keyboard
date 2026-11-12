#!/bin/sh
# Builds the Japanese pack's conversion files (packs/ja.neopack, engine 9) from the Mozc OSS
# dictionary at a fixed commit: downloads the files into build/ja/mozc (once), checks each
# against its sha256, builds conversion.dict and connection.bin with lexbuild-ja, and fills in
# the manifest's checksums (scripts/pack.py --write, which also copies kana.json and
# dictionary-notices.txt from data/langs/ja). `make ja-pack`.
#
# Licence (docs/next-languages.md, "Data that can ship"): the dictionary files are under the
# IPAdic licence with ICOT's conditions, which data/langs/ja/dictionary-notices.txt carries
# verbatim (Mozc's LICENSE at this commit, unchanged) and which travels in the pack.
set -eu
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

COMMIT=a4541df29db443b49f81cf54665873b68fde9efa
BASE="https://raw.githubusercontent.com/google/mozc/$COMMIT"
OUT=build/ja/mozc
mkdir -p "$OUT"

while read -r sum name; do
  if [ ! -f "$OUT/$name" ]; then
    case "$name" in
      LICENSE) url="$BASE/LICENSE" ;;
      *) url="$BASE/src/data/dictionary_oss/$name" ;;
    esac
    curl -sSfL -o "$OUT/$name.part" "$url"
    mv "$OUT/$name.part" "$OUT/$name"
  fi
  echo "$sum  $OUT/$name" | shasum -a 256 -c --quiet -
done <<'EOF'
9e07ce292932c4fff327ed8cb06274c09acca556bd8bdf7c7ef1f6173eae3b61 dictionary00.txt
836fd5fd8399510de0d8ef7b33be913cca6bbaee7b712cc8045fba03f4c75a89 dictionary01.txt
93943858d0cfd092b849a78def6901eaf228177e1ac5663544e865a3a1e45a9e dictionary02.txt
cb9f1b0b4a7550d682d86244a021cd17e0b15366328819e69135b5f183a1a24b dictionary03.txt
9fdc4e3948d03463de13ab5782adffbc11dd06e934cb80138c5920029cd3d2f6 dictionary04.txt
199e3b0f050b691823ae7ba943090d437f51af6436b27c5f3dc69cafa12000e4 dictionary05.txt
b95b14f7770ae2a824bc8e9fad8b03c1bc45c504f93a36554b8b56daa0368d2a dictionary06.txt
7f53649b529d4c8c495d3a8dd9c4d489dfe7696080f7423107ff01331174be48 dictionary07.txt
50a31bb5fc1646721b00be371a5695dffab1292bda143e1e59cc70cbbdd8643f dictionary08.txt
4e5fcd4cbfbf11395166a37b506f547fc54d20edc0884f5184ecd3fc705f0d9a dictionary09.txt
07a05a268c3783b7e02b36e4aa591555a2dd19a1f3c93891576740761c154f2d id.def
68823a1f6617372ccb89afdbcb334d7a0bcfa82c9c5dda03fc1f80f1cab5c48e connection_single_column.txt
44cdd923b91ea9199293abecc2762c70c87dbf1e581c027a94c416368d1a648c LICENSE
EOF

# The notices in the pack hold Mozc's LICENSE unchanged.
tail -c "$(wc -c < "$OUT/LICENSE")" data/langs/ja/dictionary-notices.txt | cmp -s - "$OUT/LICENSE" \
  || { echo "data/langs/ja/dictionary-notices.txt doesn't end with Mozc's LICENSE at $COMMIT" >&2; exit 1; }

(cd engine && cargo run -q --release -p lexbuild --bin lexbuild-ja -- \
  "../$OUT" ../packs/ja.neopack/conversion.dict ../packs/ja.neopack/connection.bin)
python3 -I scripts/pack.py packs/ja.neopack --write
