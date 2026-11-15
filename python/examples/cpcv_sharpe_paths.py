"""Distribution of a strategy's out-of-sample Sharpe ratio with CPCV.

A walk-forward backtest gives one Sharpe ratio. Combinatorial purged cross-validation gives
one per backtest path (here C(5, 1) = 5 paths from C(6, 2) = 15 splits), so you can see how
much of the number is luck. The "strategy" is a moving-average sign rule whose lookback is
chosen on the training bars only.

    pip install "fin-primitives[numpy]"
    python cpcv_sharpe_paths.py
"""
import numpy as np

from fin_primitives.validation import CombinatorialPurgedKFold, walk_forward_paths

rng = np.random.default_rng(7)
n = 3000
returns = 0.0002 + 0.01 * rng.standard_t(df=4, size=n) / np.sqrt(2.0)  # fat-tailed bars

LOOKBACKS = (5, 10, 20, 50, 100)


def signal(lookback: int) -> np.ndarray:
    """+1/-1 from the sign of the trailing mean, known at the close of bar i-1."""
    c = np.concatenate([[0.0], np.cumsum(returns)])
    mean = np.full(n, np.nan)
    mean[lookback:] = (c[lookback:-1] - c[: n - lookback]) / lookback
    return np.where(np.isnan(mean), 0.0, np.sign(mean))


SIGNALS = {lb: signal(lb) for lb in LOOKBACKS}


def sharpe(x: np.ndarray) -> float:
    s = x.std()
    return float(np.sqrt(252 * 24) * x.mean() / s) if s > 0 else 0.0


def fit_predict(train_idx: np.ndarray, test_idx: np.ndarray) -> np.ndarray:
    # Pick the lookback with the best in-sample Sharpe, then trade it on the test bars.
    best = max(LOOKBACKS, key=lambda lb: sharpe(SIGNALS[lb][train_idx] * returns[train_idx]))
    return SIGNALS[best][test_idx] * returns[test_idx]


# Each signal uses bars up to i-1 and the label is bar i's return: purge 1 bar,
# embargo 1% of the sample.
cv = CombinatorialPurgedKFold(n_groups=6, n_test_groups=2, purge=1, embargo=n // 100)
res = walk_forward_paths(cv, returns, fit_predict, metric=sharpe)

print(f"{cv.get_n_splits()} splits, {res.n_paths} out-of-sample paths")
for p, s in enumerate(res.metrics):
    print(f"  path {p}: Sharpe {s:+.2f}")
print(f"mean {res.metrics.mean():+.2f}, min {res.metrics.min():+.2f}, max {res.metrics.max():+.2f}")
