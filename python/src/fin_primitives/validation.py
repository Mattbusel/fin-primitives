"""Combinatorial purged cross-validation (CPCV) for backtests.

Pure Python on top of numpy (``pip install "fin-primitives[numpy]"``). Nothing here touches
the compiled extension, and numpy is imported only when this module is.

Standard k-fold leaks on time series: a training sample whose label window overlaps the
test window has seen the test period. CPCV (Lopez de Prado, *Advances in Financial Machine
Learning*, 2018, ch. 12) fixes this and, instead of one out-of-sample path, gives many:

* the samples are cut into ``n_groups`` contiguous groups, in order;
* every choice of ``n_test_groups`` of them is one split, so there are
  ``C(n_groups, n_test_groups)`` splits;
* **purge**: the ``purge`` training samples right before each test block and right after it
  are dropped, because their labels overlap the test block (set ``purge`` to the label
  horizon in bars);
* **embargo**: a further ``embargo`` training samples after each test block are dropped,
  for serial correlation that outlives the label horizon;
* the test groups of the splits recombine into ``C(n_groups - 1, n_test_groups - 1)`` full
  out-of-sample paths, each covering every sample exactly once.

``CombinatorialPurgedKFold`` follows scikit-learn's cross-validator protocol (``split`` and
``get_n_splits``), so it can be passed as ``cv=`` to ``cross_val_score``,
``GridSearchCV`` and friends without scikit-learn being a dependency here.
``walk_forward_paths`` runs a strategy over every split and stitches the out-of-sample
results into the backtest paths.

    >>> import numpy as np
    >>> from fin_primitives.validation import CombinatorialPurgedKFold
    >>> cv = CombinatorialPurgedKFold(n_groups=6, n_test_groups=2, purge=2, embargo=1)
    >>> cv.get_n_splits(), cv.n_paths
    (15, 5)
    >>> train, test = next(cv.split(np.zeros(60)))
    >>> int(test[0]), int(test[-1]), int(train[0])
    (0, 19, 23)

Adapted from srfm-lab's ``lib/validation/cpcv_walkforward.py`` (MIT). Changes from the lab
version: the sklearn split protocol; the embargo now follows each test block, as in the book
(the lab version placed it before the block, where the purge already covers it); purge and
embargo are applied per contiguous test block instead of per test index; the backtest-path
assembly is new; and there is no ``min_train_pct`` filter (CPCV keeps every split, otherwise
the paths are incomplete). The lab's simplified "deflated Sharpe" was dropped:
it was not the Bailey and Lopez de Prado statistic.
"""
from __future__ import annotations

import itertools
import math
from dataclasses import dataclass
from typing import Any, Callable, Iterator, List, Optional, Sequence, Tuple

import numpy as np

__all__ = [
    "CombinatorialPurgedKFold",
    "CPCVPaths",
    "walk_forward_paths",
]


def _n_samples(X: Any) -> int:
    if isinstance(X, (int, np.integer)):
        return int(X)
    shape = getattr(X, "shape", None)
    if shape is not None and len(shape) > 0:
        return int(shape[0])
    return len(X)


class CombinatorialPurgedKFold:
    """Combinatorial purged K-fold splitter with scikit-learn cross-validator semantics.

    Parameters
    ----------
    n_groups:
        Number of contiguous groups the samples are cut into (``N`` in the book). At least 2.
        Groups are sized like ``numpy.array_split``: the first ``n % n_groups`` groups hold
        one extra sample.
    n_test_groups:
        Groups per test set (``k``). ``1 <= n_test_groups < n_groups``. With ``1`` this is
        purged K-fold with a single path.
    purge:
        Training samples dropped on each side of every contiguous test block. Use the label
        horizon: a label at bar ``i`` that looks ``h`` bars ahead needs ``purge = h``.
    embargo:
        Extra training samples dropped after every test block (after the purge), to cover
        serial correlation beyond the label horizon. Lopez de Prado suggests about 1% of the
        sample count.

    Samples are assumed to be in time order. ``y`` and ``groups`` are accepted for
    scikit-learn compatibility and ignored.
    """

    def __init__(self, n_groups: int = 6, n_test_groups: int = 2, purge: int = 0, embargo: int = 0):
        for name, value in (("n_groups", n_groups), ("n_test_groups", n_test_groups),
                            ("purge", purge), ("embargo", embargo)):
            if isinstance(value, bool) or not isinstance(value, (int, np.integer)):
                raise TypeError(f"{name} must be an int, got {type(value).__name__}")
        if n_groups < 2:
            raise ValueError(f"n_groups must be at least 2, got {n_groups}")
        if not 1 <= n_test_groups < n_groups:
            raise ValueError(
                f"n_test_groups must be in [1, n_groups), got {n_test_groups} with n_groups={n_groups}"
            )
        if purge < 0 or embargo < 0:
            raise ValueError("purge and embargo must be non-negative")
        self.n_groups = int(n_groups)
        self.n_test_groups = int(n_test_groups)
        self.purge = int(purge)
        self.embargo = int(embargo)

    def __repr__(self) -> str:
        return (f"CombinatorialPurgedKFold(n_groups={self.n_groups}, n_test_groups={self.n_test_groups}, "
                f"purge={self.purge}, embargo={self.embargo})")

    # -- scikit-learn protocol -------------------------------------------------------------

    def get_n_splits(self, X: Any = None, y: Any = None, groups: Any = None) -> int:
        """Number of splits, ``C(n_groups, n_test_groups)``. Does not depend on ``X``."""
        return math.comb(self.n_groups, self.n_test_groups)

    def split(self, X: Any, y: Any = None, groups: Any = None) -> Iterator[Tuple[np.ndarray, np.ndarray]]:
        """Yield ``(train_indices, test_indices)`` for every split, as sorted int64 arrays.

        ``X`` is anything with a length (array, DataFrame, list) or the sample count as an
        int. Splits come in lexicographic order of their test groups:
        ``(0, 1), (0, 2), ..., (N-2, N-1)``; ``test_groups()`` lists them.
        """
        n = _n_samples(X)
        bounds = self.group_bounds(n)
        for combo in self.test_groups():
            yield self._split_one(n, bounds, combo)

    # -- CPCV extras ------------------------------------------------------------------------

    @property
    def n_paths(self) -> int:
        """Number of full out-of-sample paths, ``C(n_groups - 1, n_test_groups - 1)``."""
        return math.comb(self.n_groups - 1, self.n_test_groups - 1)

    def test_groups(self) -> List[Tuple[int, ...]]:
        """The test groups of each split, in the order ``split`` yields them."""
        return list(itertools.combinations(range(self.n_groups), self.n_test_groups))

    def group_bounds(self, n_samples: int) -> List[Tuple[int, int]]:
        """``[start, stop)`` sample bounds of each group (``numpy.array_split`` sizing)."""
        n = int(n_samples)
        if n < self.n_groups:
            raise ValueError(f"cannot cut {n} samples into n_groups={self.n_groups} groups")
        base, extra = divmod(n, self.n_groups)
        out, start = [], 0
        for g in range(self.n_groups):
            stop = start + base + (1 if g < extra else 0)
            out.append((start, stop))
            start = stop
        return out

    def path_assignment(self) -> np.ndarray:
        """``(n_splits, n_groups)`` int array: the path each split's test group belongs to.

        Entry ``[s, g]`` is the path id that split ``s`` fills for group ``g``, or ``-1`` when
        ``g`` is not a test group of split ``s``. Group ``g`` is tested in exactly
        ``n_paths`` splits; the ``j``-th of them, in split order, fills path ``j``.
        """
        combos = self.test_groups()
        out = np.full((len(combos), self.n_groups), -1, dtype=np.int64)
        seen = [0] * self.n_groups
        for s, combo in enumerate(combos):
            for g in combo:
                out[s, g] = seen[g]
                seen[g] += 1
        return out

    def _split_one(self, n: int, bounds: Sequence[Tuple[int, int]], combo: Sequence[int]):
        test_mask = np.zeros(n, dtype=bool)
        for g in combo:
            a, b = bounds[g]
            test_mask[a:b] = True
        train_mask = ~test_mask
        # Merge adjacent test groups into contiguous blocks, then purge and embargo each.
        for a, b in self._blocks(bounds, combo):
            train_mask[max(0, a - self.purge):a] = False
            train_mask[b:min(n, b + self.purge + self.embargo)] = False
        return np.flatnonzero(train_mask).astype(np.int64), np.flatnonzero(test_mask).astype(np.int64)

    @staticmethod
    def _blocks(bounds: Sequence[Tuple[int, int]], combo: Sequence[int]) -> List[Tuple[int, int]]:
        blocks: List[Tuple[int, int]] = []
        for g in sorted(combo):
            a, b = bounds[g]
            if blocks and blocks[-1][1] == a:
                blocks[-1] = (blocks[-1][0], b)
            else:
                blocks.append((a, b))
        return blocks


@dataclass
class CPCVPaths:
    """Out-of-sample backtest paths assembled by ``walk_forward_paths``.

    Attributes
    ----------
    paths:
        ``(n_paths, n_samples)`` float array. Row ``p`` is one full out-of-sample path: for
        every group, the values produced by the split assigned to path ``p`` for that group.
    path_assignment:
        ``(n_splits, n_groups)`` path ids, as ``CombinatorialPurgedKFold.path_assignment``.
    test_groups:
        Test groups of each split, in split order.
    split_results:
        Whatever ``fit_predict`` returned for each split (the out-of-sample arrays).
    metrics:
        ``metric(path)`` for every path when a metric was given, else ``None``.
    """

    paths: np.ndarray
    path_assignment: np.ndarray
    test_groups: List[Tuple[int, ...]]
    split_results: List[np.ndarray]
    metrics: Optional[np.ndarray] = None

    @property
    def n_paths(self) -> int:
        return int(self.paths.shape[0])

    def apply(self, metric: Callable[[np.ndarray], float]) -> np.ndarray:
        """Evaluate ``metric`` on every path; returns a ``(n_paths,)`` float array."""
        return np.array([float(metric(p)) for p in self.paths], dtype=float)


def walk_forward_paths(
    cv: CombinatorialPurgedKFold,
    X: Any,
    fit_predict: Callable[[np.ndarray, np.ndarray], Any],
    metric: Optional[Callable[[np.ndarray], float]] = None,
) -> CPCVPaths:
    """Run ``fit_predict`` on every CPCV split and stitch the results into backtest paths.

    ``fit_predict(train_idx, test_idx)`` fits on ``train_idx`` and returns one
    out-of-sample value per test index, in the order of ``test_idx`` (for a strategy:
    its per-bar returns on the test bars). The values for each test group are placed into
    the path ``cv.path_assignment()`` gives, so every row of ``paths`` is a complete
    out-of-sample series over all samples. With ``metric`` (for example a Sharpe ratio),
    ``metrics`` holds ``metric`` of each path, so you get a distribution of the statistic
    instead of the single number a walk-forward backtest gives.
    """
    n = _n_samples(X)
    bounds = cv.group_bounds(n)
    assignment = cv.path_assignment()
    combos = cv.test_groups()
    paths = np.full((cv.n_paths, n), np.nan, dtype=float)
    results: List[np.ndarray] = []
    for s, (train_idx, test_idx) in enumerate(cv.split(n)):
        out = np.asarray(fit_predict(train_idx, test_idx), dtype=float).reshape(-1)
        if out.shape[0] != test_idx.shape[0]:
            raise ValueError(
                f"fit_predict returned {out.shape[0]} values for split {s} with {test_idx.shape[0]} test samples"
            )
        results.append(out)
        offset = 0
        for g in combos[s]:
            a, b = bounds[g]
            paths[assignment[s, g], a:b] = out[offset:offset + (b - a)]
            offset += b - a
    result = CPCVPaths(paths=paths, path_assignment=assignment, test_groups=combos, split_results=results)
    if metric is not None:
        result.metrics = result.apply(metric)
    return result
