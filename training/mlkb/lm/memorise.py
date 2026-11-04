"""Memorisation check (design §10): does the LM reproduce its share-alike training text?

    uv run --group train python -m mlkb.lm.memorise --model build/lm.mlkbnn \\
        --shards build/lm/shards --out ../data/eval/memorisation.json

Prompts: `PROMPTS` random `PROMPT`-token windows that start at a word start and stay inside one
paragraph, from each of English Wikipedia (`en-wiki`), Simple English Wikipedia (`en-simple`),
Malayalam Wikipedia and Malayalam Wikisource (`ml`, told apart by `build/lm/text/sources.tsv`).
Each prompt is continued for `GENERATED` tokens through the production Rust decoder
(`mlengine.Lm.generate`, which slides its cache as the phone does): greedily, then sampled at
temperature 0.7 with seeds 1 and 2.

Measures, per generation:
- `continuation`: how many generated tokens match the source's own continuation from the first;
- `corpus`: the longest run of generated tokens found anywhere in the share-alike training text
  (`en-wiki`, `en-simple`, `ml`, `manglish-*`, `mixed-*`): `WINDOW - 1` plus the longest run of
  consecutive `WINDOW`-token windows that are in it (a hash index of every window), 0 for none.

A corpus run whose tokens hold `LOOP_DISTINCT` or fewer distinct ones is a loop (`0000…`,
`, , , ,`): a model stuck on one or two tokens. Every window of a one-token loop is the same
window, so one such run in the corpus matches a loop of any length. Loops count like any other
run; the runs that aren't loops are also summarised apart (`corpus_not_loops`), for diagnosis.

Writes the max, p99 and a histogram of each, the prompt count and the model's SHA-256; `pass` is
whether every run, by both measures, stays under `LIMIT` tokens. Exits 1 on a fail. The longest runs and the longest loops are listed with their text (`--dump`
saves every generation for a closer look).

`sources.tsv` (`bucket<TAB>source<TAB>first paragraph`) is derived here when missing:
`data/raw/text/ml.txt` is Malayalam Wikipedia followed by Wikisource (data/build-lexicons.sh),
and `corpus.py` keeps its order, so Wikisource begins at the first `ml.txt` paragraph that is
one of the first paragraphs of the Wikisource dump (`--wikisource`).
"""
import argparse
import bz2
import hashlib
import json
import sys
from pathlib import Path

import mlengine
import numpy as np

from mlkb.lm.shards import read_pieces

REPO = Path(__file__).resolve().parents[3]
WIKISOURCE_DUMP = next(iter(sorted((REPO / "data/raw/mlwikisource").glob("*.xml.bz2"))), None)

WINDOW = 16
BASE = np.uint64(1_000_003)
LIMIT = 50
PROMPT = 32
GENERATED = 64
PROMPTS = 500
SEED = 20261104
SHARE_ALIKE = ("en-wiki", "en-simple", "ml", "manglish-0", "manglish-1", "manglish-2", "manglish-3",
               "mixed-0", "mixed-1")
RUNS = ((0.0, 0), (0.7, 1), (0.7, 2))  # (temperature, seed)
# A matched run with this many distinct tokens or fewer is a loop (`0000…`, `, , , ,`,
# `to be 1000…`): listed apart in the output, for diagnosis only (design §10).
LOOP_DISTINCT = 4
BOS = 2
# Wikisource paragraphs read from the dump to find where it begins in `ml.txt`.
WIKISOURCE_PARAGRAPHS = 200


def window_hashes(tokens: np.ndarray, out: np.ndarray | None = None) -> np.ndarray:
    """The polynomial hash (base `BASE`, wrapping in uint64) of every `WINDOW`-token window."""
    n = len(tokens) - WINDOW + 1
    if n <= 0:
        return np.empty(0, np.uint64)
    h = np.empty(n, np.uint64) if out is None else out
    h[:] = tokens[:n]
    for j in range(1, WINDOW):
        h *= BASE
        h += tokens[j:j + n]
    return h


class WindowIndex:
    """Every `WINDOW`-token window of some token arrays, as sorted hashes. No window spans two
    arrays."""

    def __init__(self, hashes: np.ndarray):
        self.hashes = hashes

    @classmethod
    def build(cls, arrays) -> "WindowIndex":
        sizes = [max(len(a) - WINDOW + 1, 0) for a in arrays]
        hashes = np.empty(sum(sizes), np.uint64)
        at = 0
        for a, n in zip(arrays, sizes):
            window_hashes(a, hashes[at:at + n])
            at += n
        hashes.sort()
        return cls(hashes)

    def __len__(self) -> int:
        return len(self.hashes)

    def contains(self, hashes: np.ndarray) -> np.ndarray:
        i = np.searchsorted(self.hashes, hashes).clip(0, max(len(self.hashes) - 1, 0))
        return self.hashes[i] == hashes if len(self.hashes) else np.zeros(len(hashes), bool)


def corpus_runs(generated, index: WindowIndex) -> list[tuple[int, int]]:
    """(start, length in tokens) of every run of consecutive windows of `generated` in `index`:
    `WINDOW - 1` plus the windows."""
    hits = index.contains(window_hashes(np.asarray(generated, np.uint64)))
    d = np.diff(np.concatenate(([0], hits.astype(np.int8), [0])))
    starts, ends = np.flatnonzero(d == 1), np.flatnonzero(d == -1)
    return [(int(s), int(e - s) + WINDOW - 1) for s, e in zip(starts, ends)]


def corpus_run(generated, index: WindowIndex) -> int:
    """The longest of `corpus_runs`, or 0 when there is none."""
    return max((n for _, n in corpus_runs(generated, index)), default=0)


def is_loop(span) -> bool:
    """Whether a matched span is a loop: `LOOP_DISTINCT` or fewer distinct tokens."""
    return len(set(span)) <= LOOP_DISTINCT


def continuation_run(generated, source) -> int:
    """How many tokens of `generated` match `source` from the start."""
    n = 0
    for g, s in zip(generated, source):
        if g != s:
            break
        n += 1
    return n


def histogram(values) -> dict[str, int]:
    """Counts in buckets of ten tokens, the last `>=50`."""
    names = [f"{lo}-{lo + 9}" for lo in range(0, 50, 10)] + [">=50"]
    counts = dict.fromkeys(names, 0)
    for v in values:
        counts[names[min(v // 10, 5)]] += 1
    return counts


def summary(values: list[int]) -> dict:
    return {"max": int(max(values)), "p99": float(np.percentile(values, 99)), "histogram": histogram(values)}


def paragraph_starts(tokens: np.ndarray) -> np.ndarray:
    return np.flatnonzero(tokens == BOS)


def wikisource_start(ml_text: Path, dump: Path) -> int:
    """The index of the first paragraph of `ml_text` that is one of the first paragraphs of the
    Wikisource dump (see the module doc). The texts are compared without punctuation, which
    `corpus.py` drops from some paragraphs."""
    sys.path.insert(0, str(REPO / "data/pipeline"))
    import extract_wiki  # stdlib only

    from mlkb.lm.corpus import strip_punct

    wanted = set()
    with bz2.open(dump, "rb") as stream:
        for text in extract_wiki.article_texts(stream, ["104", "Page", "താൾ"]):
            for para in extract_wiki.paragraphs(text):
                if len(para.split()) >= 10:
                    wanted.add(strip_punct(mlengine.lm_normalize(para)))
            if len(wanted) >= WIKISOURCE_PARAGRAPHS:
                break
    k = 0
    with open(ml_text, encoding="utf-8") as f:
        for line in f:
            if not line.strip():
                continue
            if strip_punct(line.strip()) in wanted:
                return k
            k += 1
    raise ValueError(f"no Wikisource paragraph found in {ml_text}")


def read_sources(path: Path) -> dict[str, list[tuple[str, int]]]:
    out: dict[str, list[tuple[str, int]]] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        bucket, source, first = line.split("\t")
        out.setdefault(bucket, []).append((source, int(first)))
    return out


def prompts(tokens: np.ndarray, lo: int, hi: int, word_start: np.ndarray, k: int, rng) -> list[int]:
    """`k` random positions in `tokens[lo:hi]` where a `PROMPT`-token window starts at a word start
    and holds no paragraph start."""
    found: list[int] = []
    tries = 0
    while len(found) < k:
        tries += 1
        if tries > 1000 * k:
            raise ValueError("too few prompt windows")
        p = int(rng.integers(lo, hi - PROMPT - GENERATED))
        window = tokens[p:p + PROMPT]
        if word_start[window[0]] and not (window == BOS).any():
            found.append(p)
    return found


def entry(x) -> dict:
    run, distinct, loop, source, temperature, prompt, generated = x
    return {"run": run, "distinct_tokens_in_run": distinct, "loop": loop, "source": source,
            "temperature": temperature, "prompt": prompt, "generated": generated}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="python -m mlkb.lm.memorise", description=__doc__.splitlines()[0])
    parser.add_argument("--model", type=Path, default=Path("build/lm.mlkbnn"))
    parser.add_argument("--shards", type=Path, default=Path("build/lm/shards"))
    parser.add_argument("--text", type=Path, default=Path("build/lm/text"))
    parser.add_argument("--vocab", type=Path, default=Path("build/lm/pieces.tsv"))
    parser.add_argument("--wikisource", type=Path, default=WIKISOURCE_DUMP, help="Wikisource dump")
    parser.add_argument("--prompts", type=int, default=PROMPTS, help="per source")
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--dump", type=Path, help="also save every prompt and generation (.npz)")
    args = parser.parse_args(argv)

    sources_path = args.text / "sources.tsv"
    if not sources_path.exists():
        start = wikisource_start(args.text / "ml.txt", args.wikisource)
        sources_path.write_text(f"ml\tmlwiki\t0\nml\tmlwikisource\t{start}\n", encoding="utf-8")
        print(f"wrote {sources_path}: Wikisource from paragraph {start:,}", file=sys.stderr)
    sources = read_sources(sources_path)

    pieces, _ = read_pieces(args.vocab)
    word_start = np.array([p.startswith("▁") for p in pieces])
    shards = {name: np.memmap(args.shards / f"{name}.bin", np.uint16, "r") for name in SHARE_ALIKE}
    rng = np.random.default_rng(SEED)
    jobs = []  # (source, shard, position)
    for name in ("en-wiki", "en-simple"):
        for p in prompts(shards[name], 0, len(shards[name]), word_start, args.prompts, rng):
            jobs.append((name, name, p))
    starts = paragraph_starts(shards["ml"])
    split = dict(sources["ml"])
    bounds = {"mlwiki": (0, int(starts[split["mlwikisource"]])),
              "mlwikisource": (int(starts[split["mlwikisource"]]), len(shards["ml"]))}
    for source, (lo, hi) in bounds.items():
        for p in prompts(shards["ml"], lo, hi, word_start, args.prompts, rng):
            jobs.append((source, "ml", p))

    print(f"indexing {', '.join(SHARE_ALIKE)}", file=sys.stderr)
    index = WindowIndex.build([shards[name] for name in SHARE_ALIKE])
    print(f"{len(index):,} windows", file=sys.stderr)

    lm = mlengine.Lm(str(args.model))
    text = lambda ids: "".join(pieces[t] for t in ids).replace("▁", " ").strip()  # noqa: E731
    cont, corp, prose = [], [], []
    worst: dict[str, int] = {}
    longest = []  # (run, distinct tokens in the run, loop, source, temperature, prompt text, generated text)
    dumped = []
    for i, (source, shard, p) in enumerate(jobs):
        tokens = shards[shard]
        prompt = [int(t) for t in tokens[p:p + PROMPT]]
        after = tokens[p + PROMPT:p + PROMPT + GENERATED]
        bos = np.flatnonzero(after == BOS)
        after = [int(t) for t in (after[:bos[0]] if len(bos) else after)]
        for temperature, seed in RUNS:
            generated = lm.generate(prompt, GENERATED, temperature, seed)
            c = continuation_run(generated, after)
            spans = [generated[s:s + n] for s, n in corpus_runs(generated, index)]
            r = max((len(x) for x in spans), default=0)
            cont.append(c)
            corp.append(r)
            prose.append(max((len(x) for x in spans if not is_loop(x)), default=0))
            worst[source] = max(worst.get(source, 0), c, r)
            for n, span, is_l in [(c, generated[:c], False)] + [(len(x), x, is_loop(x)) for x in spans]:
                if n >= LIMIT // 2:
                    longest.append((n, len(set(span)), is_l, source, temperature, text(prompt), text(generated)))
            dumped.append((source, temperature, prompt, generated))
        if (i + 1) % 200 == 0:
            print(f"{i + 1}/{len(jobs)} prompts, worst so far {max(worst.values())}", file=sys.stderr)
    longest.sort(key=lambda x: -x[0])
    if args.dump:
        np.savez(args.dump, source=np.array([d[0] for d in dumped]), temperature=np.array([d[1] for d in dumped]),
                 prompt=np.array([d[2] for d in dumped], np.int32),
                 generated=np.array([d[3] + [0] * (GENERATED - len(d[3])) for d in dumped], np.int32))

    passed = max(cont) < LIMIT and max(corp) < LIMIT
    result = {
        "model": args.model.name,
        "model_sha256": hashlib.sha256(args.model.read_bytes()).hexdigest(),
        "prompts": len(jobs),
        "prompts_per_source": {s: sum(1 for j in jobs if j[0] == s) for s in worst},
        "generations": len(cont),
        "prompt_tokens": PROMPT,
        "generated_tokens": GENERATED,
        "runs": [{"temperature": t, "seed": s} for t, s in RUNS],
        "window": WINDOW,
        "indexed_windows": len(index),
        "limit": LIMIT,
        "continuation": summary(cont),
        "corpus": summary(corp),
        "loop_distinct": LOOP_DISTINCT,
        "corpus_not_loops": summary(prose),
        "worst_by_source": worst,
        "runs_over_half_the_limit": sum(not x[2] for x in longest),
        "loops_over_half_the_limit": sum(x[2] for x in longest),
        "quoted_text_licence": "The prompts and generations below quote CC BY-SA 4.0 text from "
                               "English and Malayalam Wikipedia and Malayalam Wikisource "
                               "(Wikipedia and Wikisource contributors).",
        "longest": [entry(x) for x in longest if not x[2]][:10],
        "longest_loops": [entry(x) for x in longest if x[2]][:5],
        "pass": passed,
    }
    args.out.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(result, indent=2))
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
