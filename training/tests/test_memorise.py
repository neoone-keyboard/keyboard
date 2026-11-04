import numpy as np

from mlkb.lm.memorise import (LOOP_DISTINCT, WINDOW, WindowIndex, continuation_run, corpus_run, corpus_runs,
                              histogram, is_loop)


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


def test_every_run_is_found_with_its_span():
    rng = np.random.default_rng(1)
    corpus = rng.integers(4, 16000, 5000).astype(np.uint16)
    index = WindowIndex.build([corpus])
    generated = [7] * 3 + list(corpus[100:130]) + [9] * 3 + list(corpus[2000:2020])
    assert corpus_runs(generated, index) == [(3, 30), (36, 20)]
    assert corpus_runs([1] * 10, index) == []


def test_a_run_over_a_few_tokens_is_a_loop():
    assert is_loop([0] * 50)
    assert is_loop([11, 12, 3] + [0] * 47)  # "to be 1" then zeros
    assert is_loop(list(range(LOOP_DISTINCT)) * 10)
    assert not is_loop(list(range(LOOP_DISTINCT + 1)) * 10)


def test_a_copied_span_is_not_a_loop_but_a_looping_copy_is():
    rng = np.random.default_rng(2)
    text = rng.integers(4, 16000, 3000).astype(np.uint16)
    loop = np.full(40, 5, np.uint16)
    index = WindowIndex.build([text, loop])
    copied = list(text[500:560])
    (start, n), = corpus_runs(copied, index)
    assert n == 60 and not is_loop(copied[start:start + n])
    zeros = [5] * 64
    (start, n), = corpus_runs(zeros, index)
    assert n == 64 and is_loop(zeros[start:start + n])
