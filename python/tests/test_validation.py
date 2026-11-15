"""Tests for fin_primitives.validation (CPCV).

Expected indices are enumerated by hand for 60 samples in 6 groups of 10 (C(6,2) = 15
splits, C(5,1) = 5 paths), not taken from the code's output.
"""
import itertools

import pytest

np = pytest.importorskip("numpy")

from fin_primitives.validation import CPCVPaths, CombinatorialPurgedKFold, walk_forward_paths  # noqa: E402


def r(a, b):
    """Inclusive integer range [a, b] as a list."""
    return list(range(a, b + 1))


def test_counts():
    cv = CombinatorialPurgedKFold(n_groups=6, n_test_groups=2)
    assert cv.get_n_splits() == 15
    assert cv.get_n_splits(np.zeros(60), None, None) == 15
    assert cv.n_paths == 5
    assert len(list(cv.split(np.zeros(60)))) == 15
    assert CombinatorialPurgedKFold(10, 3).get_n_splits() == 120
    assert CombinatorialPurgedKFold(10, 3).n_paths == 36  # C(9, 2)
    assert CombinatorialPurgedKFold(5, 1).n_paths == 1


def test_split_order_is_lexicographic():
    cv = CombinatorialPurgedKFold(6, 2)
    assert cv.test_groups() == [
        (0, 1), (0, 2), (0, 3), (0, 4), (0, 5),
        (1, 2), (1, 3), (1, 4), (1, 5),
        (2, 3), (2, 4), (2, 5),
        (3, 4), (3, 5),
        (4, 5),
    ]


def test_no_purge_is_plain_combinatorial_kfold():
    cv = CombinatorialPurgedKFold(6, 2)
    for (train, test), combo in zip(cv.split(np.zeros(60)), cv.test_groups()):
        expected_test = [i for g in combo for i in r(10 * g, 10 * g + 9)]
        assert test.tolist() == expected_test
        assert train.tolist() == [i for i in range(60) if i not in expected_test]
        assert train.dtype == np.int64 and test.dtype == np.int64


# purge=2, embargo=1: two samples dropped before each test block, 2 + 1 = 3 after it.
HAND = {
    # test groups: (expected test, expected train)
    (0, 1): (r(0, 19), r(23, 59)),
    (0, 2): (r(0, 9) + r(20, 29), r(13, 17) + r(33, 59)),
    (0, 5): (r(0, 9) + r(50, 59), r(13, 47)),
    (1, 3): (r(10, 19) + r(30, 39), r(0, 7) + r(23, 27) + r(43, 59)),
    (2, 3): (r(20, 39), r(0, 17) + r(43, 59)),
    (2, 4): (r(20, 29) + r(40, 49), r(0, 17) + r(33, 37) + r(53, 59)),
    (4, 5): (r(40, 59), r(0, 37)),
}


@pytest.mark.parametrize("combo", sorted(HAND))
def test_purge_and_embargo_by_index(combo):
    cv = CombinatorialPurgedKFold(n_groups=6, n_test_groups=2, purge=2, embargo=1)
    splits = dict(zip(cv.test_groups(), cv.split(np.zeros(60))))
    train, test = splits[combo]
    exp_test, exp_train = HAND[combo]
    assert test.tolist() == exp_test
    assert train.tolist() == exp_train


def test_purge_boundaries_split_1_3():
    cv = CombinatorialPurgedKFold(6, 2, purge=2, embargo=1)
    train, _ = dict(zip(cv.test_groups(), cv.split(60)))[(1, 3)]
    t = set(train.tolist())
    # last kept before block [10, 19], first kept after it
    assert 7 in t and 8 not in t and 9 not in t
    assert 22 not in t and 23 in t
    # block [30, 39]
    assert 27 in t and 28 not in t and 29 not in t
    assert 40 not in t and 41 not in t and 42 not in t and 43 in t


def test_train_never_within_purge_of_test():
    cv = CombinatorialPurgedKFold(7, 3, purge=4, embargo=3)
    n = 103
    for train, test in cv.split(np.zeros((n, 2))):
        assert np.intersect1d(train, test).size == 0
        test_set = set(test.tolist())
        for i in train.tolist():
            # nothing in the 4 samples after a train sample is a test sample (purge before)
            assert not any(i + d in test_set for d in range(1, 5))
            # nothing in the 7 samples before it is a test sample (purge + embargo after)
            assert not any(i - d in test_set for d in range(1, 8))
        assert sorted(set(train.tolist()) | test_set | set(range(n))) == list(range(n))


def test_group_bounds_follow_array_split():
    cv = CombinatorialPurgedKFold(4, 1)
    assert cv.group_bounds(10) == [(0, 3), (3, 6), (6, 8), (8, 10)]
    for n in (8, 9, 10, 11, 37):
        expected = [(int(a[0]), int(a[-1]) + 1) for a in np.array_split(np.arange(n), 4)]
        assert cv.group_bounds(n) == expected


def test_accepts_len_shape_or_int():
    cv = CombinatorialPurgedKFold(3, 1)
    a = [t.tolist() for _, t in cv.split(list(range(9)))]
    b = [t.tolist() for _, t in cv.split(np.zeros((9, 4)))]
    c = [t.tolist() for _, t in cv.split(9)]
    assert a == b == c == [r(0, 2), r(3, 5), r(6, 8)]


def test_path_assignment_by_hand():
    a = CombinatorialPurgedKFold(6, 2).path_assignment()
    combos = CombinatorialPurgedKFold(6, 2).test_groups()
    rows = {c: a[s].tolist() for s, c in enumerate(combos)}
    #                group: 0   1   2   3   4   5
    assert rows[(0, 1)] == [0, 0, -1, -1, -1, -1]
    assert rows[(0, 2)] == [1, -1, 0, -1, -1, -1]
    assert rows[(1, 2)] == [-1, 1, 1, -1, -1, -1]
    assert rows[(1, 3)] == [-1, 2, -1, 1, -1, -1]
    assert rows[(4, 5)] == [-1, -1, -1, -1, 4, 4]
    # every group appears in every path exactly once
    for g in range(6):
        assert sorted(a[a[:, g] >= 0, g].tolist()) == [0, 1, 2, 3, 4]


def test_walk_forward_paths_cover_every_sample_once():
    cv = CombinatorialPurgedKFold(6, 2, purge=2, embargo=1)
    n = 60
    combos = cv.test_groups()
    calls = []

    def fit_predict(train, test):
        s = len(calls)
        calls.append((train, test))
        return np.full(test.shape[0], float(s))  # value = split id

    res = walk_forward_paths(cv, np.zeros(n), fit_predict, metric=np.mean)
    assert isinstance(res, CPCVPaths)
    assert res.paths.shape == (5, 60) and res.n_paths == 5
    assert not np.isnan(res.paths).any()
    # path 0: the first split testing each group, (0,1) for groups 0 and 1, then (0,2) ... (0,5)
    split_id = {c: s for s, c in enumerate(combos)}
    expected_path0 = [split_id[(0, 1)]] * 20 + [split_id[(0, 2)]] * 10 + [split_id[(0, 3)]] * 10 \
        + [split_id[(0, 4)]] * 10 + [split_id[(0, 5)]] * 10
    assert res.paths[0].tolist() == [float(v) for v in expected_path0]
    # last path: the last split testing each group
    expected_path4 = [split_id[(0, 5)]] * 10 + [split_id[(1, 5)]] * 10 + [split_id[(2, 5)]] * 10 \
        + [split_id[(3, 5)]] * 10 + [split_id[(4, 5)]] * 20
    assert res.paths[4].tolist() == [float(v) for v in expected_path4]
    assert res.metrics.tolist() == [float(np.mean(p)) for p in res.paths]
    assert len(calls) == 15 and len(res.split_results) == 15


def test_walk_forward_paths_identity_reconstructs_series():
    # A model that "predicts" the series itself must give n_paths copies of it.
    x = np.sin(np.arange(57) / 3.0)
    cv = CombinatorialPurgedKFold(5, 2, purge=1)
    res = walk_forward_paths(cv, x, lambda tr, te: x[te])
    assert res.paths.shape == (cv.n_paths, 57)
    for p in res.paths:
        assert np.array_equal(p, x)


def test_walk_forward_paths_rejects_wrong_length():
    cv = CombinatorialPurgedKFold(3, 1)
    with pytest.raises(ValueError):
        walk_forward_paths(cv, 9, lambda tr, te: np.zeros(1))


def test_sklearn_style_signature_and_repr():
    cv = CombinatorialPurgedKFold(4, 2, purge=1, embargo=2)
    assert repr(cv) == "CombinatorialPurgedKFold(n_groups=4, n_test_groups=2, purge=1, embargo=2)"
    y = np.arange(20)
    assert len(list(cv.split(np.zeros(20), y, groups=y))) == 6


@pytest.mark.parametrize(
    "kwargs",
    [dict(n_groups=1), dict(n_groups=4, n_test_groups=4), dict(n_groups=4, n_test_groups=0),
     dict(purge=-1), dict(embargo=-1)],
)
def test_bad_parameters(kwargs):
    with pytest.raises(ValueError):
        CombinatorialPurgedKFold(**kwargs)


def test_bad_types_and_too_few_samples():
    with pytest.raises(TypeError):
        CombinatorialPurgedKFold(n_groups=6.0)
    with pytest.raises(TypeError):
        CombinatorialPurgedKFold(purge=True)
    with pytest.raises(ValueError):
        list(CombinatorialPurgedKFold(6, 2).split(5))


def test_sklearn_cross_val_score_accepts_it():
    sklearn = pytest.importorskip("sklearn")  # noqa: F841
    from sklearn.linear_model import LinearRegression
    from sklearn.model_selection import cross_val_score

    rng = np.random.default_rng(0)
    X = rng.normal(size=(60, 3))
    y = X @ np.array([1.0, -2.0, 0.5]) + 0.01 * rng.normal(size=60)
    scores = cross_val_score(LinearRegression(), X, y, cv=CombinatorialPurgedKFold(6, 2, purge=1))
    assert scores.shape == (15,)
    assert (scores > 0.99).all()


def test_matches_brute_force_definition():
    # Independent restatement: a train index i survives iff no test index t has
    # t - purge <= i < t (purge before) or t < i <= t + purge + embargo (after).
    for n, N, k, p, e in [(41, 5, 2, 3, 2), (30, 6, 3, 1, 0), (25, 4, 1, 0, 4)]:
        cv = CombinatorialPurgedKFold(N, k, purge=p, embargo=e)
        for train, test in cv.split(n):
            ts = set(test.tolist())
            expected = [
                i for i in range(n)
                if i not in ts
                and not any(i < t <= i + p for t in ts)
                and not any(t < i <= t + p + e for t in ts)
            ]
            assert train.tolist() == expected
        assert sum(1 for _ in itertools.combinations(range(N), k)) == cv.get_n_splits()


def test_matches_skfolio_when_groups_divide_evenly():
    # skfolio sizes groups as n // N with the remainder in the last group, this module as
    # numpy.array_split; with n divisible by N the two must agree index for index.
    pytest.importorskip("skfolio")
    from skfolio.model_selection import CombinatorialPurgedCV

    for n, N, k, p, e in [(120, 6, 2, 2, 1), (240, 8, 3, 3, 0), (360, 10, 2, 0, 4)]:
        sk = CombinatorialPurgedCV(n_folds=N, n_test_folds=k, purged_size=p, embargo_size=e)
        ours = CombinatorialPurgedKFold(N, k, purge=p, embargo=e)
        X = np.zeros((n, 2))
        for (tr1, te1), (tr2, te2) in zip(sk.split(X), ours.split(X)):
            assert np.array_equal(np.sort(tr1), tr2)
            assert np.array_equal(np.sort(np.concatenate(te1)), te2)
        rp = np.asarray(sk.recombined_paths)  # group x path -> split
        a = ours.path_assignment()  # split x group -> path
        assert all(a[rp[g, q], g] == q for g in range(N) for q in range(rp.shape[1]))
