import numpy as np

from mlkb.lm.shards import LOOP_PERIODS, MAX_LOOP, cut_loops


def test_a_long_loop_keeps_its_first_whole_periods():
    zeros = np.array([7, 1] + [0] * 40 + [9], np.uint16)
    out, removed = cut_loops(zeros)
    assert out.tolist() == [7, 1] + [0] * MAX_LOOP + [9]
    assert removed == 40 - MAX_LOOP
    commas = np.array([5] + [3, 4] * 20 + [8], np.uint16)
    out, _ = cut_loops(commas)
    assert out.tolist() == [5] + [3, 4] * (MAX_LOOP // 2) + [8]
    sevens = np.array([1] + list(range(10, 17)) * 5 + [2], np.uint16)
    out, _ = cut_loops(sevens)
    assert out.tolist() == [1] + list(range(10, 17)) + [2]  # one whole period fits in MAX_LOOP


def test_no_window_of_a_loop_is_left():
    rng = np.random.default_rng(0)
    parts = []
    for p in range(1, LOOP_PERIODS + 1):
        parts += [rng.integers(100, 200, 5), np.tile(rng.integers(200, 300, p), 64 // p)]
    tokens = np.concatenate(parts).astype(np.uint16)
    out, _ = cut_loops(tokens)
    for p in range(1, LOOP_PERIODS + 1):
        eq = np.concatenate(([False], out[p:] == out[:-p], [False]))
        d = np.diff(eq.astype(np.int8))
        spans = np.flatnonzero(d == -1) - np.flatnonzero(d == 1) + p
        assert spans.max(initial=0) <= MAX_LOOP


def test_text_without_loops_is_unchanged():
    tokens = np.arange(1000, dtype=np.uint16) % 37
    tokens[::5] = 2
    out, removed = cut_loops(tokens[:30])
    assert removed == 0 and out.tolist() == tokens[:30].tolist()
