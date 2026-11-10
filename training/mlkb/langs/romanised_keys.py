"""The romanised lookup keys of a pack language: `skeleton<TAB>word<TAB>count` rows, which
`lexbuild-native --romanised-keys` packs into `romanised.fst`, as `manglish.fst` is keyed by
`romanize.rs` for Malayalam.

    uv run python -m mlkb.langs.romanised_keys --lang as --words data/raw/bundle-as/words.tsv \\
        --out data/raw/bundle-as/romanised-keys.tsv

Each word is keyed by the skeleton of its walk under the pack's format-1 `romanisation.json`
(`mlkb.langs.format1.Format1.key`): the walk and skeleton the engine will run, so the build and
run time agree. One key per word, as for Malayalam.
"""

import argparse
from pathlib import Path

from mlkb.langs.format1 import Format1


def main(argv: list[str] | None = None) -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--lang", default="as")
    p.add_argument("--tables", type=Path, help="format-1 romanisation.json (default data/langs/<lang>/)")
    p.add_argument("--words", type=Path, required=True, help="word<TAB>count rows (lexbuild-native's words.tsv)")
    p.add_argument("--out", type=Path, required=True)
    args = p.parse_args(argv)
    f = Format1.load(args.tables or args.lang)
    n = words = 0
    keyless = []
    with args.out.open("w", encoding="utf-8") as out:
        for line in args.words.read_text(encoding="utf-8").splitlines():
            word, count = line.split("\t")
            words += 1
            key = f.key(word)
            if key:
                out.write(f"{key}\t{word}\t{count}\n")
                n += 1
            else:
                keyless.append(word)
    print(f"{args.out}: {n:,} keys for {words:,} words")
    if keyless:
        # Their skeleton is empty (every letter folds away, or the tables have no spelling for
        # one), so Latin typing can't reach them; the native keyboard still can.
        print(
            f"  {len(keyless):,} word{'s' if len(keyless) > 1 else ''} with no key, so not reachable by Latin "
            f"typing: {' '.join(keyless[:20])}{' …' if len(keyless) > 20 else ''}"
        )


if __name__ == "__main__":
    main()
