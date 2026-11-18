"""pandas-ta's function API on fin-primitives' Rust kernels: change one import.

The same function names, input order, parameters, defaults and column names as pandas-ta.
If pandas-ta-classic is installed, the script also runs it and prints the largest
difference between the two.

    pip install "fin-primitives[pandas]"
    python pandas_ta_dropin.py
"""
import time

import numpy as np
import pandas as pd

from fin_primitives import ta  # was: import pandas_ta as ta

rng = np.random.default_rng(7)
n = 100_000
close = 100 * np.exp(np.cumsum(rng.normal(0, 0.01, n)))
spread = np.abs(rng.normal(0, 0.005, n)) * close
df = pd.DataFrame({
    "high": close + spread,
    "low": close - spread,
    "close": close,
    "volume": rng.lognormal(8, 1, n).round(),
})

start = time.perf_counter()
out = pd.concat(
    [
        ta.rsi(df["close"], length=14),
        ta.macd(df["close"], fast=12, slow=26, signal=9),
        ta.bbands(df["close"], length=20, std=2),
        ta.atr(df["high"], df["low"], df["close"], length=14),
        ta.adx(df["high"], df["low"], df["close"]),
        ta.mfi(df["high"], df["low"], df["close"], df["volume"]),
    ],
    axis=1,
)
ours = time.perf_counter() - start
print(f"fin_primitives.ta: {len(out.columns)} columns over {n:,} bars in {ours * 1000:.1f} ms")
print(out.tail(3).round(4).to_string())

try:
    if not hasattr(np, "NaN"):
        np.NaN = np.nan  # pandas-ta-classic 0.9.0 still reads np.NaN
    import pandas_ta_classic as pta
except ImportError:
    pta = None

if pta is not None:
    start = time.perf_counter()
    ref = pd.concat(
        [
            pta.rsi(df["close"], length=14, talib=False),
            pta.macd(df["close"], fast=12, slow=26, signal=9, talib=False),
            pta.bbands(df["close"], length=20, std=2, talib=False),
            pta.atr(df["high"], df["low"], df["close"], length=14, talib=False),
            pta.adx(df["high"], df["low"], df["close"], talib=False),
            pta.mfi(df["high"], df["low"], df["close"], df["volume"], talib=False),
        ],
        axis=1,
    )
    theirs = time.perf_counter() - start
    assert list(ref.columns) == list(out.columns)
    rel = ((out - ref).abs() / ref.abs().clip(lower=1.0)).max().max()
    print(f"pandas-ta-classic: {theirs * 1000:.1f} ms; largest relative difference {rel:.1e}")
