"""Write a seeded, real-looking OHLCV random walk to data/bars.csv.

Log prices follow a slowly mean-reverting walk (1.2% per-bar vol, reversion
0.1% per bar toward 100) so 100k bars stay in a realistic 30..300 range;
prices are rounded to cents; high/low extend past open/close by a random intrabar
excursion; volume is lognormal and loosely tied to the absolute return.
Every value is written as a short decimal string so that fin-primitives
(exact decimals) and TA-Lib (float64) read the same numbers.
"""
import argparse
import os

import numpy as np


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bars", type=int, default=100_000)
    ap.add_argument("--seed", type=int, default=20260930)
    ap.add_argument("--out", default=os.path.join(os.path.dirname(__file__), "data", "bars.csv"))
    a = ap.parse_args()

    rng = np.random.default_rng(a.seed)
    n = a.bars
    eps = rng.normal(0.0, 0.012, n)
    logp = np.empty(n)
    x = np.log(100.0)
    for i in range(n):
        x = x + 0.001 * (np.log(100.0) - x) + eps[i]
        logp[i] = x
    close = np.exp(logp)
    ret = np.diff(np.concatenate([[np.log(100.0)], logp]))
    open_ = np.empty(n)
    open_[0] = 100.0
    open_[1:] = close[:-1] * np.exp(rng.normal(0.0, 0.002, n - 1))  # small overnight gap
    body_hi = np.maximum(open_, close)
    body_lo = np.minimum(open_, close)
    high = body_hi * (1.0 + np.abs(rng.normal(0.0, 0.004, n)))
    low = body_lo * (1.0 - np.abs(rng.normal(0.0, 0.004, n)))
    vol = np.round(rng.lognormal(13.0, 0.4, n) * (1.0 + 40.0 * np.abs(ret)))

    r2 = lambda x: np.round(x, 2)
    o, h, l, c = r2(open_), r2(high), r2(low), r2(close)
    # Rounding can break the OHLC ordering by a cent; restore it.
    h = np.maximum.reduce([h, o, c])
    l = np.minimum.reduce([l, o, c])

    os.makedirs(os.path.dirname(a.out), exist_ok=True)
    with open(a.out, "w", newline="\n") as f:
        f.write("open,high,low,close,volume\n")
        for i in range(n):
            f.write(f"{o[i]:.2f},{h[i]:.2f},{l[i]:.2f},{c[i]:.2f},{int(vol[i])}\n")
    print(f"wrote {n} bars to {a.out} (seed {a.seed}); close range {c.min():.2f}..{c.max():.2f}")


if __name__ == "__main__":
    main()
