"""Trains the bilingual LM on the token shards from `mlkb.lm.shards`.

    uv run --group train python -m mlkb.lm.train --shards build/lm/shards \\
        --vocab build/lm/pieces.tsv --out build/lm.pt
    uv run --group train python -m mlkb.lm.train --lang hi    # build/lm-hi/shards -> build/lm-hi.pt

`--lang` (`mlkb.lm.lang`) names the native and romanised buckets (`hi`, `hinglish` for Hindi);
the weights are the same.

Each batch is 256 windows of 49 tokens (48 inputs, 48 targets), drawn from the buckets by
`BUCKETS` weight (design §5). A bucket with several pass files (`manglish-0..3`, `mixed-0,1`)
takes its windows from them in turn, so the Malayalam text is seen about 3.5 times in all,
each time with fresh spellings. Windows start at a word boundary, half of them at a sentence
start, and never run past the end of their paragraph.

AdamW (lr 1e-3, betas (0.9, 0.95), weight decay 0.1 on matrices only), 2k warmup steps,
cosine decay to 10% over `--steps`, gradient clipping at 1.0, float32 on MPS.

Every `--eval-every` steps: validation loss per bucket on the `valid-*` shards (the same
`--valid-batches` batches each time). The score is the mean over the four languages (English,
the mean of its four buckets; Malayalam script; Manglish; mixed). The best checkpoint goes to
`--out` (`{"model", "config", "step", "valid"}`); training stops after `--patience`
evaluations without a gain.

Resumable: each evaluation also writes `<out stem>-last.pt` (model, optimiser, schedule, step,
sampler state, early-stopping state). A run started again with the same `--out` continues from
it; `--fresh` starts over. Progress goes to stdout and to `--log`.
"""

import argparse
import json
import math
import re
import sys
import time
from collections.abc import Callable
from pathlib import Path

import numpy as np
import torch
import torch.nn.functional as F

from mlkb.lm import lang as lm_lang
from mlkb.lm.model import LM, Config

BOS = 2
PAD = 0

BUCKETS = {
    "en-wiki": 0.27,
    "en-simple": 0.09,
    "en-news": 0.04,
    "en-gutenberg": 0.10,
    "ml": 0.22,
    "manglish": 0.17,
    "mixed": 0.11,
}
# Validation languages: the score is the mean of these group means.
GROUPS = {
    "en": ("en-wiki", "en-simple", "en-news", "en-gutenberg"),
    "ml": ("ml",),
    "manglish": ("manglish",),
    "mixed": ("mixed",),
}



def buckets_for(lang: lm_lang.LmLang = lm_lang.ML) -> dict[str, float]:
    """`BUCKETS` with the language's native and romanised bucket names, in the same order."""
    names = {"ml": lang.native, "manglish": lang.romanised}
    return {names.get(k, k): w for k, w in BUCKETS.items()}


def groups_for(lang: lm_lang.LmLang = lm_lang.ML) -> dict[str, tuple[str, ...]]:
    """`GROUPS` with the language's bucket names."""
    names = {"ml": lang.native, "manglish": lang.romanised}
    return {names.get(k, k): tuple(names.get(b, b) for b in v) for k, v in GROUPS.items()}


_SEEK = 64  # tokens searched ahead of a random offset for a window start
_SENTENCE_END = re.compile(r"[.?!।][\"'”’)\]]*$")


def schedule(step: int, warmup: int, total: int, floor: float = 0.1) -> float:
    """Learning-rate multiplier: linear warmup from 0, then cosine decay to `floor`."""
    if step < warmup:
        return step / warmup
    progress = min(1.0, (step - warmup) / max(1, total - warmup))
    return floor + (1 - floor) * 0.5 * (1 + math.cos(math.pi * progress))


def _table(pred: Callable[[int], bool] | np.ndarray | None) -> np.ndarray | None:
    if pred is None or isinstance(pred, np.ndarray):
        return pred
    return np.array([bool(pred(t)) for t in range(1 << 16)])


class Windows:
    """Random training windows from one memory-mapped `uint16` shard.

    `word_start` and `sentence_end` say which token ids begin a word and end a sentence, as a
    function of the id or a boolean array indexed by id. With probability `p_sentence` (when
    `sentence_end` is given) a window starts at a word that follows a sentence end."""

    def __init__(self, path: Path, length: int, word_start, seed: int, sentence_end=None,
                 p_sentence: float = 0.5):
        self.path = Path(path)
        self.ids = np.memmap(path, dtype="<u2", mode="r")
        self.length = length
        self.word_start = _table(word_start)
        self.sentence_end = _table(sentence_end)
        self.p_sentence = p_sentence if sentence_end is not None else 0.0
        self.rng = np.random.default_rng(seed)

    def _start(self) -> int | None:
        off = int(self.rng.integers(0, len(self.ids)))
        chunk = np.asarray(self.ids[off : off + _SEEK])
        ok = self.word_start[chunk]
        if self.p_sentence and self.rng.random() < self.p_sentence:
            before = int(self.ids[off - 1]) if off > 0 else BOS
            prev = np.concatenate([[before], chunk[:-1]])
            ok &= self.sentence_end[prev] | (prev == BOS)
        hits = np.flatnonzero(ok)
        if not len(hits):
            return None
        start = off + int(hits[0])
        if start > 0 and self.ids[start - 1] == BOS:
            start -= 1
        return start

    def sample(self) -> tuple[np.ndarray, np.ndarray]:
        """`(x, mask)`, `length + 1` tokens each (int64, bool): x is padded with 0 from the next
        paragraph start (or the end of the shard) on, and `mask` is false there."""
        n = self.length + 1
        while True:
            start = self._start()
            if start is None:
                continue
            x = np.zeros(n, dtype=np.int64)
            got = np.asarray(self.ids[start : start + n], dtype=np.int64)
            x[: len(got)] = got
            mask = np.zeros(n, dtype=bool)
            mask[: len(got)] = True
            later = np.flatnonzero(x[1:] == BOS)
            if len(later):
                cut = int(later[0]) + 1
                x[cut:] = PAD
                mask[cut:] = False
            if mask[1:].any():
                return x, mask

    def batch(self, count: int) -> tuple[np.ndarray, np.ndarray]:
        """`count` samples stacked: `[count, length + 1]`."""
        rows = [self.sample() for _ in range(count)]
        return np.stack([r[0] for r in rows]), np.stack([r[1] for r in rows])


class Bucket:
    """The pass files of one bucket, sampled in turn."""

    def __init__(self, files: list[Path], length: int, word_start, sentence_end, seed: int):
        self.windows = [Windows(f, length, word_start, seed + i, sentence_end) for i, f in enumerate(files)]
        self.turn = 0

    def batch(self, count: int) -> tuple[np.ndarray, np.ndarray]:
        xs, ms = [], []
        for _ in range(count):
            x, m = self.windows[self.turn].sample()
            self.turn = (self.turn + 1) % len(self.windows)
            xs.append(x)
            ms.append(m)
        return np.stack(xs), np.stack(ms)

    def state(self) -> dict:
        return {"turn": self.turn, "rngs": [w.rng.bit_generator.state for w in self.windows]}

    def restore(self, state: dict) -> None:
        self.turn = state["turn"]
        for w, s in zip(self.windows, state["rngs"], strict=True):
            w.rng.bit_generator.state = s


def bucket_files(shards: Path, name: str) -> list[Path]:
    """`<name>.bin` or `<name>-<k>.bin`, in pass order."""
    files = sorted(shards.glob(f"{name}-[0-9]*.bin"), key=lambda p: int(p.stem.rsplit("-", 1)[1]))
    single = shards / f"{name}.bin"
    return files or [single]


def piece_tables(vocab: Path) -> tuple[np.ndarray, np.ndarray]:
    """Boolean tables by id: word-start pieces (`▁` prefix) and sentence-end pieces."""
    word, end = np.zeros(1 << 16, bool), np.zeros(1 << 16, bool)
    for i, line in enumerate(vocab.read_text(encoding="utf-8").splitlines()):
        piece = line.rsplit("\t", 1)[0]
        word[i] = piece.startswith("▁")
        end[i] = bool(_SENTENCE_END.search(piece.removeprefix("▁")))
    return word, end


def masked_loss(model: LM, x: torch.Tensor, mask: torch.Tensor, reduction: str = "mean") -> torch.Tensor:
    """Cross-entropy of the targets `x[:, 1:]` where `mask[:, 1:]`."""
    logits = model(x[:, :-1])
    target = x[:, 1:].masked_fill(~mask[:, 1:], -100)
    return F.cross_entropy(logits.transpose(1, 2), target, ignore_index=-100, reduction=reduction)


@torch.no_grad()
def validate(model: LM, valid: dict[str, tuple[np.ndarray, np.ndarray]], device, batch: int) -> dict[str, float]:
    """Mean loss per target token for each bucket."""
    model.eval()
    out = {}
    for name, (xs, ms) in valid.items():
        total, count = 0.0, 0
        for i in range(0, len(xs), batch):
            x = torch.from_numpy(xs[i : i + batch]).to(device)
            m = torch.from_numpy(ms[i : i + batch]).to(device)
            total += masked_loss(model, x, m, reduction="sum").item()
            count += int(m[:, 1:].sum().item())
        out[name] = total / max(count, 1)
    model.train()
    return out


def score(losses: dict[str, float], groups: dict[str, tuple[str, ...]] = GROUPS) -> float:
    """The mean over `groups` of each group's mean bucket loss (lower is better)."""
    return sum(sum(losses[b] for b in g) / len(g) for g in groups.values()) / len(groups)


def pick_device(name: str) -> torch.device:
    if name != "auto":
        return torch.device(name)
    return torch.device("mps" if torch.backends.mps.is_available() else "cpu")


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--lang", default=lm_lang.DEFAULT, choices=sorted(lm_lang.LANGS))
    p.add_argument("--shards", type=Path, default=None, help="default: <the language's build>/shards")
    p.add_argument("--vocab", type=Path, default=None, help="default: <the language's build>/pieces.tsv")
    p.add_argument("--out", type=Path, default=None, help="default: the language's checkpoint")
    p.add_argument("--log", type=Path, default=None, help="default: <out stem>.log next to --out")
    p.add_argument("--steps", type=int, default=120_000,
                   help="cosine schedule length and step cap; 120k steps see about 1.1B tokens (design §5)")
    p.add_argument("--batch", type=int, default=256)
    p.add_argument("--lr", type=float, default=1e-3)
    p.add_argument("--warmup", type=int, default=2000)
    p.add_argument("--eval-every", type=int, default=2000)
    p.add_argument("--valid-batches", type=int, default=200)
    p.add_argument("--patience", type=int, default=5)
    p.add_argument("--device", default="auto")
    p.add_argument("--seed", type=int, default=0)
    p.add_argument("--fresh", action="store_true", help="ignore an existing -last.pt checkpoint")
    args = p.parse_args(argv)
    lang = lm_lang.load(args.lang)
    args.shards = args.shards or Path(lang.build) / "shards"
    args.vocab = args.vocab or Path(lang.build) / "pieces.tsv"
    args.out = args.out or Path(lang.checkpoint)
    weights_by_bucket, groups = buckets_for(lang), groups_for(lang)

    log_path = args.log or args.out.with_suffix(".log")
    log_file = log_path.open("a", encoding="utf-8")

    def log(msg: str) -> None:
        line = f"{time.strftime('%Y-%m-%d %H:%M:%S')} {msg}"
        print(line, flush=True)
        log_file.write(line + "\n")
        log_file.flush()

    torch.manual_seed(args.seed)
    device = pick_device(args.device)
    config = Config()
    length = config.max_len
    word, end = piece_tables(args.vocab)
    buckets = {name: Bucket(bucket_files(args.shards, name), length, word, end, args.seed + 100 * i)
               for i, name in enumerate(weights_by_bucket)}
    for name, b in buckets.items():
        log(f"bucket {name}: weight {weights_by_bucket[name]:.2f}, "
            + ", ".join(f"{w.path.name} {len(w.ids):,} tokens" for w in b.windows))
    valid = {}
    for i, name in enumerate(weights_by_bucket):
        v = Windows(args.shards / f"valid-{name}.bin", length, word, 10_000 + i, end)
        valid[name] = v.batch(args.valid_batches * args.batch)
    weights = np.array(list(weights_by_bucket.values()))
    mix_rng = np.random.default_rng(args.seed + 1)

    model = LM(config).to(device)
    decay = [p_ for p_ in model.parameters() if p_.ndim >= 2]
    no_decay = [p_ for p_ in model.parameters() if p_.ndim < 2]
    opt = torch.optim.AdamW([{"params": decay, "weight_decay": 0.1}, {"params": no_decay, "weight_decay": 0.0}],
                            lr=args.lr, betas=(0.9, 0.95))
    sched = torch.optim.lr_scheduler.LambdaLR(opt, lambda s: schedule(s, args.warmup, args.steps))
    last_path = args.out.with_name(args.out.stem + "-last.pt")
    step, best, bad, history, elapsed = 0, math.inf, 0, [], 0.0
    if last_path.exists() and not args.fresh:
        state = torch.load(last_path, map_location=device, weights_only=False)
        model.load_state_dict(state["model"])
        opt.load_state_dict(state["opt"])
        sched.load_state_dict(state["sched"])
        step, best, bad, history = state["step"], state["best"], state["bad"], state["history"]
        elapsed = state.get("elapsed", 0.0)
        mix_rng.bit_generator.state = state["mix_rng"]
        for name, b in buckets.items():
            b.restore(state["buckets"][name])
        log(f"resumed from {last_path} at step {step:,} (best {best:.4f}, {bad} evaluations without gain)")
    n_params = sum(p_.numel() for p_ in model.parameters())
    log(f"device {device}; {n_params:,} parameters; config {config.to_dict()}; args {vars(args)}")

    model.train()
    loss_sum, loss_n, tick, tokens = 0.0, 0, time.monotonic(), 0
    run_start = time.monotonic() - elapsed
    while step < args.steps:
        counts = mix_rng.multinomial(args.batch, weights)
        parts = [buckets[name].batch(int(c)) for name, c in zip(weights_by_bucket, counts, strict=True) if c]
        xs = np.concatenate([q[0] for q in parts])
        ms = np.concatenate([q[1] for q in parts])
        x = torch.from_numpy(xs).to(device)
        m = torch.from_numpy(ms).to(device)
        loss = masked_loss(model, x, m)
        opt.zero_grad(set_to_none=True)
        loss.backward()
        torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
        opt.step()
        sched.step()
        step += 1
        tokens += int(ms[:, 1:].sum())
        if step % 50 == 0:
            value = loss.item()
            if not math.isfinite(value):
                log(f"non-finite loss {value} at step {step:,}; stopping")
                return 1
            loss_sum += value
            loss_n += 1
        if step % 500 == 0:
            dt = time.monotonic() - tick
            log(f"step {step:,} loss {loss_sum / max(loss_n, 1):.4f} lr {sched.get_last_lr()[0]:.2e} "
                f"{500 / dt:.2f} steps/s {tokens / dt:,.0f} tokens/s")
            loss_sum, loss_n, tick, tokens = 0.0, 0, time.monotonic(), 0
        if step % args.eval_every == 0 or step == args.steps:
            ev = time.monotonic()
            losses = validate(model, valid, device, args.batch)
            s = score(losses, groups)
            history.append({"step": step, "valid": losses, "score": s})
            improved = s < best
            if improved:
                best, bad = s, 0
                torch.save({"model": model.state_dict(), "config": config.to_dict(), "step": step,
                            "valid": losses}, args.out)
            else:
                bad += 1
            torch.save({"model": model.state_dict(), "opt": opt.state_dict(), "sched": sched.state_dict(),
                        "step": step, "best": best, "bad": bad, "history": history,
                        "config": config.to_dict(), "mix_rng": mix_rng.bit_generator.state,
                        "buckets": {k: b.state() for k, b in buckets.items()},
                        "elapsed": time.monotonic() - run_start}, last_path)
            log(f"eval step {step:,}: " + " ".join(f"{k} {v:.4f}" for k, v in losses.items())
                + f" score {s:.4f} (ppl {math.exp(s):.1f}) "
                + ("(best, saved)" if improved else f"(no gain {bad}/{args.patience})")
                + f" {time.monotonic() - ev:.0f}s; {(time.monotonic() - run_start) / 3600:.2f} h")
            log("eval-json " + json.dumps(history[-1]))
            tick += time.monotonic() - ev
            if bad >= args.patience:
                log(f"early stop at step {step:,}; best score {best:.4f} in {args.out}")
                return 0
    log(f"reached {args.steps:,} steps; best score {best:.4f} in {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
