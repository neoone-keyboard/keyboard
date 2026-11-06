"""Trains the transliterator on the pair files from `mlkb.data.pairs`.

    uv run --group train python -m mlkb.translit.train \\
        --pairs training/build/pairs --out training/build/translit.pt

AdamW (lr 5e-4, 2k warmup steps, cosine decay to 10% over `--steps`), batch 512, label
smoothing 0.1, gradient clipping at 1.0, on MPS when available.

Every `--eval-every` steps it measures exact-match top-1 (greedy decoding) on a fixed sample
of `valid.tsv` (synthetic) and on all of `valid-human.tsv` (Dakshina dev), and scores the
checkpoint by their mean. The best checkpoint goes to `--out` (model weights and config
only). Training stops when the score hasn't improved for `--patience` evaluations.

Resumable: each evaluation also writes `<out stem>-last.pt` (model, optimiser, schedule,
step, early-stopping state). A run started again with the same `--out` continues from it;
pass `--fresh` to start over. Progress goes to stdout; run it as
`nohup … > training/build/translit.log 2>&1 &`.
"""

import argparse
import math
import random
import sys
import time
from pathlib import Path

import numpy as np
import torch
import torch.nn.functional as F

from mlkb.translit.decode import greedy_tokens
from mlkb.translit.model import Config, Translit, param_count
from mlkb.translit.vocab import LATIN, MALAYALAM, Vocab, decode_ml, encode_latin, encode_ml, fits, native

ROOT = Path(__file__).resolve().parents[3]
PAIRS = ROOT / "training/build/pairs"
OUT = ROOT / "training/build/translit.pt"


def log(msg: str) -> None:
    print(f"{time.strftime('%H:%M:%S')} {msg}", flush=True)


def read_pairs(path: Path, max_len: int, vocab: Vocab = MALAYALAM) -> tuple[list[tuple[str, str]], int]:
    """The `latin<TAB>native` rows of `path` that fit the model, and how many didn't."""
    rows, dropped = [], 0
    with path.open(encoding="utf-8") as f:
        for line in f:
            latin, ml = line.rstrip("\n").split("\t")
            if fits(latin, ml, max_len, vocab):
                rows.append((latin, ml))
            else:
                dropped += 1
    return rows, dropped


def to_arrays(rows: list[tuple[str, str]], max_len: int, vocab: Vocab = MALAYALAM) -> tuple[np.ndarray, np.ndarray]:
    """Padded token arrays `[n, max_len]` (uint8) for the Latin and Malayalam sides."""
    src = np.zeros((len(rows), max_len), dtype=np.uint8)
    tgt = np.zeros((len(rows), max_len), dtype=np.uint8)
    for i, (latin, ml) in enumerate(rows):
        s, t = encode_latin(latin), encode_ml(ml, vocab)
        src[i, : len(s)] = s
        tgt[i, : len(t)] = t
    return src, tgt


def load_train(path: Path, max_len: int, vocab: Vocab = MALAYALAM) -> tuple[np.ndarray, np.ndarray, int]:
    """Train arrays, cached next to the TSV as `.npz` until the TSV changes."""
    cache = path.with_suffix(f".{max_len}.npz")
    if cache.exists() and cache.stat().st_mtime > path.stat().st_mtime:
        z = np.load(cache)
        return z["src"], z["tgt"], int(z["dropped"])
    rows, dropped = read_pairs(path, max_len, vocab)
    src, tgt = to_arrays(rows, max_len, vocab)
    np.savez(cache, src=src, tgt=tgt, dropped=dropped)
    return src, tgt, dropped


def schedule(step: int, warmup: int, total: int, floor: float = 0.1) -> float:
    """Learning-rate multiplier: linear warmup, then cosine decay to `floor`."""
    if step < warmup:
        return (step + 1) / warmup
    progress = min(1.0, (step - warmup) / max(1, total - warmup))
    return floor + (1 - floor) * 0.5 * (1 + math.cos(math.pi * progress))


def batch(src: np.ndarray, tgt: np.ndarray, idx: np.ndarray, device) -> tuple[torch.Tensor, torch.Tensor]:
    """A batch trimmed to its longest row, as long tensors on `device`."""
    s, t = src[idx], tgt[idx]
    s = s[:, : int((s != LATIN.pad).sum(1).max())]
    t = t[:, : int((t != MALAYALAM.pad).sum(1).max())]
    return torch.from_numpy(s.astype(np.int64)).to(device), torch.from_numpy(t.astype(np.int64)).to(device)


def accuracy(
    model: Translit, rows: list[tuple[str, str]], device, batch_size: int = 2048, vocab: Vocab = MALAYALAM
) -> float:
    """Exact-match top-1 of greedy decoding over `rows`."""
    model.eval()
    hits = 0
    max_len = model.config.max_len
    for start in range(0, len(rows), batch_size):
        chunk = rows[start : start + batch_size]
        src, _ = to_arrays(chunk, max_len, vocab)
        s = torch.from_numpy(src[:, : int((src != 0).sum(1).max())].astype(np.int64)).to(device)
        out = greedy_tokens(model, s).cpu().tolist()
        hits += sum(decode_ml(o, vocab) == ml for o, (_, ml) in zip(out, chunk, strict=True))
    model.train()
    return hits / max(len(rows), 1)


def pick_device(name: str) -> torch.device:
    if name != "auto":
        return torch.device(name)
    return torch.device("mps" if torch.backends.mps.is_available() else "cpu")


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--pairs", type=Path, default=PAIRS)
    p.add_argument("--out", type=Path, default=OUT)
    p.add_argument("--steps", type=int, default=100_000, help="cosine schedule length and step cap")
    p.add_argument("--batch", type=int, default=512)
    p.add_argument("--lr", type=float, default=5e-4)
    p.add_argument("--warmup", type=int, default=2000)
    p.add_argument("--eval-every", type=int, default=2000)
    p.add_argument("--patience", type=int, default=5)
    p.add_argument("--valid-rows", type=int, default=5000, help="synthetic valid rows sampled per evaluation")
    p.add_argument("--device", default="auto")
    p.add_argument("--seed", type=int, default=0)
    p.add_argument("--fresh", action="store_true", help="ignore an existing -last.pt checkpoint")
    p.add_argument("--lang", default="ml", help="the target language's vocabulary (mlkb.translit.vocab.NATIVE)")
    args = p.parse_args(argv)

    torch.manual_seed(args.seed)
    device = pick_device(args.device)
    vocab = native(args.lang)
    config = Config(tgt_vocab=len(vocab))
    last_path = args.out.with_name(args.out.stem + "-last.pt")

    t0 = time.monotonic()
    src, tgt, dropped = load_train(args.pairs / "train.tsv", config.max_len, vocab)
    valid_all, _ = read_pairs(args.pairs / "valid.tsv", config.max_len, vocab)
    valid = random.Random(args.seed).sample(valid_all, min(args.valid_rows, len(valid_all)))
    human_path = args.pairs / "valid-human.tsv"
    valid_human = read_pairs(human_path, config.max_len, vocab)[0] if human_path.exists() else []
    log(
        f"train {len(src):,} rows ({dropped:,} too long or unencodable dropped); valid {len(valid):,} "
        f"of {len(valid_all):,}; valid-human {len(valid_human):,}; loaded in {time.monotonic() - t0:.0f}s"
    )

    model = Translit(config).to(device)
    opt = torch.optim.AdamW(model.parameters(), lr=args.lr, betas=(0.9, 0.98), weight_decay=0.01)
    sched = torch.optim.lr_scheduler.LambdaLR(opt, lambda s: schedule(s, args.warmup, args.steps))
    step, best, bad, history = 0, -1.0, 0, []
    if last_path.exists() and not args.fresh:
        state = torch.load(last_path, map_location=device, weights_only=False)
        model.load_state_dict(state["model"])
        opt.load_state_dict(state["opt"])
        sched.load_state_dict(state["sched"])
        step, best, bad, history = state["step"], state["best"], state["bad"], state["history"]
        log(f"resumed from {last_path} at step {step:,} (best {best:.4f}, {bad} evaluations without gain)")
    log(f"device {device}; {param_count(model):,} parameters; config {config.to_dict()}; args {vars(args)}")

    n = len(src)
    per_epoch = max(1, n // args.batch)
    rng = np.random.default_rng(args.seed)
    order_epoch = step // per_epoch
    for _ in range(order_epoch):  # replay the permutation stream so a resumed run sees the same order
        rng.permutation(n)
    order = rng.permutation(n)
    model.train()
    loss_sum, loss_n, tick = 0.0, 0, time.monotonic()
    while step < args.steps:
        if step // per_epoch != order_epoch:
            order_epoch += 1
            order = rng.permutation(n)
        pos = step % per_epoch
        s, t = batch(src, tgt, order[pos * args.batch : (pos + 1) * args.batch], device)
        logits = model(s, t[:, :-1])
        loss = F.cross_entropy(
            logits.transpose(1, 2), t[:, 1:], ignore_index=MALAYALAM.pad, label_smoothing=0.1
        )
        opt.zero_grad(set_to_none=True)
        loss.backward()
        torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
        opt.step()
        sched.step()
        step += 1
        loss_sum += loss.item() if step % 50 == 0 else 0.0
        loss_n += step % 50 == 0
        if step % 500 == 0:
            rate = 500 / (time.monotonic() - tick)
            log(
                f"step {step:,} epoch {step / per_epoch:.2f} loss {loss_sum / max(loss_n, 1):.4f} "
                f"lr {sched.get_last_lr()[0]:.2e} {rate:.1f} steps/s"
            )
            loss_sum, loss_n, tick = 0.0, 0, time.monotonic()
        if step % args.eval_every == 0 or step == args.steps:
            ev = time.monotonic()
            acc = accuracy(model, valid, device, vocab=vocab)
            acc_h = accuracy(model, valid_human, device, vocab=vocab) if valid_human else acc
            score = (acc + acc_h) / 2
            history.append({"step": step, "valid": acc, "valid_human": acc_h})
            improved = score > best
            if improved:
                best, bad = score, 0
                torch.save({"model": model.state_dict(), "config": config.to_dict(), "step": step, "score": score}, args.out)
            else:
                bad += 1
            torch.save(
                {
                    "model": model.state_dict(),
                    "opt": opt.state_dict(),
                    "sched": sched.state_dict(),
                    "step": step,
                    "best": best,
                    "bad": bad,
                    "history": history,
                    "config": config.to_dict(),
                },
                last_path,
            )
            log(
                f"eval step {step:,}: valid top1 {acc:.4f}  valid-human top1 {acc_h:.4f}  score {score:.4f} "
                f"{'(best, saved)' if improved else f'(no gain {bad}/{args.patience})'}  {time.monotonic() - ev:.0f}s"
            )
            tick += time.monotonic() - ev
            if bad >= args.patience:
                log(f"early stop at step {step:,}; best score {best:.4f} in {args.out}")
                return 0
    log(f"reached {args.steps:,} steps; best score {best:.4f} in {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
