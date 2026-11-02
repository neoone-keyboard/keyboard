"""SCOWL tarball -> one English word per line (sizes 10-70, US/UK/CA spellings).

Sizes above 70 and the Australian lists carry extra copyright notices, so
they are left out (see data/manifests/scowl.json)."""

import argparse
import re
import sys
import tarfile

LIST = re.compile(r"/final/(english|american|british|canadian)-(words|upper|contractions|proper-names)\.(\d+)$")
MAX_SIZE = 70


def words(tar_path: str) -> set[str]:
    found: set[str] = set()
    with tarfile.open(tar_path, "r:gz") as tar:
        for member in tar.getmembers():
            match = LIST.search(member.name)
            if not match or int(match.group(3)) > MAX_SIZE:
                continue
            data = tar.extractfile(member)
            if data is None:
                continue
            for line in data.read().decode("latin-1").splitlines():
                word = line.strip()
                if word and word.isascii():
                    found.add(word)
    return found


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tarball")
    parser.add_argument("--out", required=True)
    args = parser.parse_args(argv)
    found = sorted(words(args.tarball))
    with open(args.out, "w", encoding="utf-8") as out:
        out.write("\n".join(found) + "\n")
    print(f"{args.out}: {len(found):,} words", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
