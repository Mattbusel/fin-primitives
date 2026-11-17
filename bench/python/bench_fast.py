"""fin_primitives.fast against TA-Lib, kand, pandas-ta-classic, talipp and polars-talib.

    pip install fin-primitives numpy pandas polars TA-Lib kand talipp polars-talib "pandas-ta-classic==0.9.0"
    python bench/python/bench_fast.py            # prints the Markdown tables in docs/benchmarks.md

Input: 100,000 bars of a seeded random walk (prices rounded to cents), the same float64 numpy
arrays for every library. Each cell is the best of several runs of one whole-series call
(talipp, which updates one value at a time, is fed the whole list once). The agreement table
is the largest absolute difference from TA-Lib over bars 1,000 onward.
"""
import importlib.metadata as md
import platform
import sys
import time

import numpy as np

N = 100_000
RUNS = 7


def data(n=N, seed=20261116):
    rng = np.random.default_rng(seed)
    close = (100 * np.exp(np.cumsum(rng.normal(0, 0.01, n)))).round(2)
    high = (close * (1 + np.abs(rng.normal(0, 0.003, n)))).round(2)
    low = (close * (1 - np.abs(rng.normal(0, 0.003, n)))).round(2)
    return np.ascontiguousarray(high), np.ascontiguousarray(low), np.ascontiguousarray(close)


H, L, C = data()


def best(fn, runs=RUNS):
    out = None
    t = float("inf")
    for _ in range(runs):
        t0 = time.perf_counter()
        out = fn()
        t = min(t, time.perf_counter() - t0)
    return t * 1000, out


def first(x):
    """The main output of an indicator call as a float64 array."""
    if isinstance(x, (tuple, list)) and len(x) and not np.isscalar(x[0]):
        x = x[0]
    if hasattr(x, "to_numpy"):
        x = x.to_numpy()
    return np.asarray(x, dtype=float)


def libs():
    import talib
    from fin_primitives import fast

    out = {
        "fin_primitives.fast": {
            "SMA(20)": lambda: fast.sma(C, 20),
            "EMA(20)": lambda: fast.ema(C, 20),
            "RSI(14)": lambda: fast.rsi(C, 14),
            "ATR(14)": lambda: fast.atr(H, L, C, 14),
            "MACD(12,26,9)": lambda: fast.macd(C, 12, 26, 9),
            "BBANDS(20,2)": lambda: fast.bbands(C, 20, 2.0)[1],
        },
        "TA-Lib": {
            "SMA(20)": lambda: talib.SMA(C, 20),
            "EMA(20)": lambda: talib.EMA(C, 20),
            "RSI(14)": lambda: talib.RSI(C, 14),
            "ATR(14)": lambda: talib.ATR(H, L, C, 14),
            "MACD(12,26,9)": lambda: talib.MACD(C, 12, 26, 9),
            "BBANDS(20,2)": lambda: talib.BBANDS(C, 20, 2.0, 2.0, 0)[1],
        },
    }
    try:
        import kand

        out["kand"] = {
            "SMA(20)": lambda: kand.sma(C, 20),
            "EMA(20)": lambda: kand.ema(C, 20),
            "RSI(14)": lambda: kand.rsi(C, 14),
            "ATR(14)": lambda: kand.atr(H, L, C, 14),
            "MACD(12,26,9)": lambda: kand.macd(C, 12, 26, 9),
            "BBANDS(20,2)": lambda: kand.bbands(C, 20, 2.0, 2.0)[1],
        }
    except ImportError:
        pass
    try:
        import pandas as pd

        if not hasattr(np, "NaN"):
            np.NaN = np.nan
        import pandas_ta_classic as pta

        hs, ls, cs = pd.Series(H), pd.Series(L), pd.Series(C)
        out["pandas-ta-classic"] = {
            "SMA(20)": lambda: pta.sma(cs, 20, talib=False),
            "EMA(20)": lambda: pta.ema(cs, 20, talib=False),
            "RSI(14)": lambda: pta.rsi(cs, 14, talib=False),
            "ATR(14)": lambda: pta.atr(hs, ls, cs, 14, talib=False),
            "MACD(12,26,9)": lambda: pta.macd(cs, 12, 26, 9, talib=False).iloc[:, 0],
            "BBANDS(20,2)": lambda: pta.bbands(cs, 20, 2.0, talib=False).iloc[:, 1],
        }
    except ImportError:
        pass
    try:
        import polars as pl
        import polars_talib  # noqa: F401  registers the .ta namespace

        df = pl.DataFrame({"high": H, "low": L, "close": C})
        out["polars-talib"] = {
            "SMA(20)": lambda: df.select(pl.col("close").ta.sma(20))[:, 0],
            "EMA(20)": lambda: df.select(pl.col("close").ta.ema(20))[:, 0],
            "RSI(14)": lambda: df.select(pl.col("close").ta.rsi(14))[:, 0],
            "ATR(14)": lambda: df.select(pl.col("close").ta.atr(pl.col("high"), pl.col("low"), 14))[:, 0],
            "MACD(12,26,9)": lambda: df.select(pl.col("close").ta.macd(12, 26, 9).struct.field("macd"))[:, 0],
            "BBANDS(20,2)": lambda: df.select(pl.col("close").ta.bbands(20, 2.0, 2.0).struct.field("middleband"))[:, 0],
        }
    except ImportError:
        pass
    try:
        from talipp.indicators import ATR, BB, EMA, MACD, RSI, SMA
        from talipp.ohlcv import OHLCV

        cl = C.tolist()
        bars = [OHLCV(c, h, lo, c, 0.0) for h, lo, c in zip(H.tolist(), L.tolist(), cl)]

        def nn(xs):
            return [np.nan if x is None else x for x in xs]

        out["talipp"] = {
            "SMA(20)": lambda: nn(SMA(20, cl)),
            "EMA(20)": lambda: nn(EMA(20, cl)),
            "RSI(14)": lambda: nn(RSI(14, cl)),
            "ATR(14)": lambda: nn(ATR(14, bars)),
            "MACD(12,26,9)": lambda: nn([None if v is None else v.macd for v in MACD(12, 26, 9, cl)]),
            "BBANDS(20,2)": lambda: nn([None if v is None else v.cb for v in BB(20, 2.0, cl)]),
        }
    except ImportError:
        pass
    return out


def version(pkg):
    try:
        return md.version(pkg)
    except md.PackageNotFoundError:
        return "?"


def main():
    table = libs()
    names = list(table)
    inds = list(table["TA-Lib"])
    times, agree = {}, {}
    for lib in names:
        for ind, fn in table[lib].items():
            try:
                runs = 2 if lib == "talipp" else RUNS
                ms, out = best(fn, runs)
                times[lib, ind] = ms
                ref = first(table["TA-Lib"][ind]())
                got = first(out)
                m = ~np.isnan(ref)
                m[:1000] = False
                agree[lib, ind] = float(np.nanmax(np.abs(got[m] - ref[m]))) if got.shape == ref.shape else None
            except Exception as e:  # a library missing an indicator or with another signature
                print(f"{lib} {ind}: {type(e).__name__}: {e}", file=sys.stderr)
    print(f"{N:,} bars; best of {RUNS} runs (talipp: of 2); milliseconds per whole-series call.\n")
    print("| indicator | " + " | ".join(names) + " |")
    print("|---" * (len(names) + 1) + "|")
    for ind in inds:
        cells = [f"{times[lib, ind]:.2f}" if (lib, ind) in times else "n/a" for lib in names]
        print(f"| {ind} | " + " | ".join(cells) + " |")
    print("\nLargest absolute difference from TA-Lib, bars 1,000 onward:\n")
    print("| indicator | " + " | ".join(n for n in names if n != "TA-Lib") + " |")
    print("|---" * len(names) + "|")
    for ind in inds:
        cells = []
        for lib in names:
            if lib == "TA-Lib":
                continue
            v = agree.get((lib, ind))
            cells.append("n/a" if v is None else f"{v:.1e}")
        print(f"| {ind} | " + " | ".join(cells) + " |")
    print("\nVersions: " + ", ".join(f"{p} {version(p)}" for p in ["fin-primitives", "TA-Lib", "kand", "pandas-ta-classic", "talipp", "polars-talib", "numpy", "pandas", "polars"]))
    print(f"Python {platform.python_version()}, {platform.platform()}, {platform.processor()}")


if __name__ == "__main__":
    main()
