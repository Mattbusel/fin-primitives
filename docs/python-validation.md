# Backtest validation: combinatorial purged cross-validation

`fin_primitives.validation` is a pure-Python module (numpy only) that ships in the
`fin-primitives` wheel next to the compiled indicators.

```bash
pip install "fin-primitives[numpy]"
```

## Why

A walk-forward backtest produces one equity curve and one Sharpe ratio. Plain k-fold
cross-validation on bars leaks: a training bar whose label looks into the test window has
already seen it. Combinatorial purged cross-validation (Lopez de Prado, *Advances in
Financial Machine Learning*, 2018, chapter 12) removes the leak and turns one backtest into
many:

| step | what it does |
|---|---|
| groups | the `n` bars are cut into `n_groups` contiguous groups, in time order |
| splits | every choice of `n_test_groups` groups is a test set: `C(n_groups, n_test_groups)` splits |
| purge | the `purge` training bars before and after each contiguous test block are dropped (set it to the label horizon) |
| embargo | a further `embargo` training bars after each test block are dropped (serial correlation) |
| paths | the test groups recombine into `C(n_groups - 1, n_test_groups - 1)` complete out-of-sample paths |

## Splitter

```python
import numpy as np
from fin_primitives.validation import CombinatorialPurgedKFold

cv = CombinatorialPurgedKFold(n_groups=6, n_test_groups=2, purge=2, embargo=1)
cv.get_n_splits()   # 15
cv.n_paths          # 5
for train_idx, test_idx in cv.split(X):    # X: array, DataFrame, list or the sample count
    ...
```

It follows scikit-learn's cross-validator protocol (`split(X, y=None, groups=None)` yields
sorted int64 `(train, test)` index arrays, `get_n_splits()`), so it works as
`cv=` in `cross_val_score`, `GridSearchCV` and the rest. scikit-learn is not a dependency.

Splits come in lexicographic order of their test groups, `(0, 1), (0, 2), ..., (4, 5)`;
`cv.test_groups()` lists them. Group sizes follow `numpy.array_split` (the first
`n % n_groups` groups hold one extra bar), like scikit-learn's `KFold`.

Worked example, 60 bars, `purge=2, embargo=1`, split `(1, 3)`:

- test: bars 10-19 and 30-39
- dropped from training: 8, 9 (purge before 10), 20, 21, 22 (purge + embargo after 19),
  28, 29 and 40, 41, 42
- train: 0-7, 23-27, 43-59

Adjacent test groups form one block: split `(2, 3)` tests 20-39 and drops only 18, 19 and
40-42.

## Backtest paths

```python
from fin_primitives.validation import walk_forward_paths

def fit_predict(train_idx, test_idx):
    model = fit(features[train_idx], labels[train_idx])
    return strategy_returns(model, test_idx)      # one value per test bar

res = walk_forward_paths(cv, X, fit_predict, metric=sharpe)
res.paths      # (n_paths, n_bars): each row a full out-of-sample return series
res.metrics    # sharpe of every path: a distribution, not one number
res.apply(max_drawdown)
```

`cv.path_assignment()` gives the mapping used: entry `[s, g]` is the path that split `s`
fills for group `g` (`-1` if `g` is not tested in `s`). Group `g` is tested in exactly
`n_paths` splits, and the `j`-th of them fills path `j`.

`python/examples/cpcv_sharpe_paths.py` runs a moving-average rule whose lookback is picked
in-sample and prints the Sharpe ratio of each of the 5 paths.

## Tests

`python/tests/test_validation.py` checks the counts (`C(6,2) = 15` splits, 5 paths), the
train and test indices of seven splits enumerated by hand (purge and embargo boundaries
checked bar by bar), the path assignment by hand, a brute-force restatement of the purge
rule over several sizes, and that a model that returns the series itself reconstructs it
on every path. When scikit-learn or skfolio are installed it also checks that
`cross_val_score` accepts the splitter and that the splits and path assignment equal
skfolio's `CombinatorialPurgedCV` index for index (on sizes where both size the groups the
same way; skfolio puts the remainder in the last group).

## Credits

Adapted from `lib/validation/cpcv_walkforward.py` in srfm-lab (MIT, same author). The lab
version put the embargo before each test block, filtered splits by a minimum train share
(which leaves the paths incomplete) and reported a simplified "deflated Sharpe" that was
not the Bailey and Lopez de Prado statistic; those parts were changed or dropped here.
