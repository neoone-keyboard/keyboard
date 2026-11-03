import numpy as np

from mlkb.lm.memorise import WINDOW, WindowIndex, continuation_run, corpus_run, histogram


def test_continuation_run_counts_the_matching_start():
    assert continuation_run([5, 6, 7, 9], [5, 6, 7, 8]) == 3
    assert continuation_run([1], [2]) == 0


def test_corpus_run_finds_a_copied_span():
    rng = np.random.default_rng(0)
    corpus = rng.integers(4, 16000, 5000).astype(np.uint16)
    index = WindowIndex.build([corpus])
    copied = list(corpus[1000:1000 + 60])
    generated = [7] * 5 + copied + [9] * 5
    assert corpus_run(generated, index) == 60
    assert corpus_run(list(rng.integers(4, 16000, 80)), index) < WINDOW


def test_the_index_spans_several_arrays_but_no_window_crosses_two():
    a, b = np.arange(4, 40, dtype=np.uint16), np.arange(100, 140, dtype=np.uint16)
    index = WindowIndex.build([a, b])
    assert len(index) == (len(a) - WINDOW + 1) + (len(b) - WINDOW + 1)
    assert corpus_run(list(a[-10:]) + list(b[:10]), index) == 0


def test_histogram_buckets_by_tens():
    assert histogram([0, 9, 10, 55, 70]) == {"0-9": 2, "10-19": 1, "20-29": 0, "30-39": 0, "40-49": 0, ">=50": 2}
