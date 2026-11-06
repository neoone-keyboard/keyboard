"""Writes the fixture that keeps lexbuild's romaniser and `table_romanise` in step.

    uv run python -m mlkb.data.romanisation_fixture --lang hi

Rows `native<TAB>canonical<TAB>skeleton<TAB>typed<TAB>typed skeleton`, from a seeded sample of
the Dakshina train lexicon (CC BY-SA 4.0) plus hand-picked hard cases. Both
`engine/lexbuild/tests/romanisation.rs` and `training/tests/test_table_romanise.py` check every row
against `packs/<lang>.neopack/romanisation.json`. Regenerate it after changing the table.
"""

import argparse
import random

from mlkb.data.table_romanise import ROOT, Table, dakshina_rows

CASES = {
    "hi": [
        ("भारत", "bhaarat"), ("कमरा", "kamraa"), ("समझना", "samjhna"), ("बचपन", "bachpan"),
        ("मित्र", "mitra"), ("दर्द", "dard"), ("क्या", "kya"), ("है", "hai"), ("में", "mein"),
        ("नहीं", "nahi"), ("मैं", "main"), ("संभव", "sambhav"), ("ज़िंदगी", "zindagi"),
        ("लड़का", "ladka"), ("ज्ञान", "gyan"), ("क्षमा", "kshama"), ("पढ़ाई", "padhai"),
        ("गए", "gaye"), ("करें", "karein"), ("कार्य", "karya"), ("न", "na"), ("हूँ", "hoon"),
        ("दुःख", "dukh"), ("फ़िल्म", "film"), ("क़िस्मत", "kismat"), ("श्रीमान", "shriman"),
    ],
}  # fmt: skip


def main(argv: list[str] | None = None) -> None:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--lang", default="hi")
    p.add_argument("--rows", type=int, default=1000)
    args = p.parse_args(argv)
    t = Table.load(ROOT / f"packs/{args.lang}.neopack/romanisation.json")
    lexicons = ROOT / f"data/raw/dakshina/dakshina_dataset_v1.0/{args.lang}/lexicons"
    rows = dakshina_rows(lexicons / f"{args.lang}.translit.sampled.train.tsv")
    picked = [(n, latin) for n, latin, _ in random.Random(20261106).sample(rows, args.rows)]
    out = ROOT / f"engine/lexbuild/tests/fixtures/{args.lang}-romanisation.tsv"
    with out.open("w", encoding="utf-8") as f:
        for native, typed in CASES.get(args.lang, []) + picked:
            canon = t.canonical(native)
            f.write(f"{native}\t{canon}\t{t.skeleton(canon)}\t{typed}\t{t.skeleton(typed)}\n")
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
