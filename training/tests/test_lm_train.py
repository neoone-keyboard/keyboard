import numpy as np
from mlkb.lm.train import BUCKETS, Windows, schedule


def test_schedule_warms_up_then_decays_to_the_floor():
    assert schedule(0, 2000, 90000) == 0.0
    assert abs(schedule(2000, 2000, 90000) - 1.0) < 1e-6
    assert abs(schedule(90000, 2000, 90000) - 0.1) < 1e-6


def test_windows_start_at_a_word_and_take_bos_at_a_paragraph_start(tmp_path):
    # ids: 2 = bos; even ids >= 10 are word starts in this fake vocabulary.
    ids = np.array([2, 10, 11, 12, 13, 2, 14, 15, 16, 17], dtype="<u2")
    path = tmp_path / "s.bin"
    ids.tofile(path)
    w = Windows(path, length=4, word_start=lambda t: t >= 10 and t % 2 == 0, seed=0)
    for _ in range(50):
        x, mask = w.sample()
        first = x[0] if x[0] != 2 else x[1]
        assert first >= 10 and first % 2 == 0
        if 2 in x[1:]:  # a window never runs into the next paragraph
            assert not mask[list(x).index(2, 1):].any()


def test_bucket_weights_sum_to_one():
    assert abs(sum(BUCKETS.values()) - 1.0) < 1e-9


def test_sentence_windows_start_after_a_sentence_end(tmp_path):
    # 5 = ".", a sentence end; word starts are even ids >= 10.
    ids = np.array([2, 10, 12, 14, 5, 16, 11, 18, 20, 5, 22, 24, 26], dtype="<u2")
    path = tmp_path / "s.bin"
    ids.tofile(path)
    w = Windows(path, length=3, word_start=lambda t: t >= 10 and t % 2 == 0, seed=1,
                sentence_end=lambda t: t == 5, p_sentence=1.0)
    for _ in range(50):
        x, _ = w.sample()
        assert x[0] in (2, 16, 22)


def test_hindi_buckets_keep_the_weights_under_their_own_names():
    from mlkb.lm import lang
    from mlkb.lm.train import GROUPS, buckets_for, groups_for, score

    hi = buckets_for(lang.load("hi"))
    assert list(hi.values()) == list(BUCKETS.values())
    assert {"hi", "hinglish"} <= set(hi) and not {"ml", "manglish"} & set(hi)
    assert buckets_for() == BUCKETS and groups_for() == GROUPS
    losses = {b: 1.0 for b in hi} | {"hi": 3.0}
    assert score(losses, groups_for(lang.load("hi"))) == (1 + 3 + 1 + 1) / 4
