"""Whole-series float64 indicators: one Rust pass over a numpy array, pandas-ta's numbers.

The rest of ``fin_primitives`` computes in exact decimal arithmetic. This module is the
other trade-off: plain float64, built for backtests that compute an indicator over a whole
price history at once. Each function borrows the input array without copying it, releases
the GIL, runs a single O(n) loop in Rust and returns a new numpy array::

    >>> from fin_primitives import fast
    >>> fast.rsi(close, 14)                        # numpy in, numpy out
    >>> lower, mid, upper = fast.bbands(df["close"], 20, 2.0)   # pandas in, pandas out
    >>> line, signal, hist = fast.macd(close, 12, 26, 9)

The results follow pandas-ta-classic (and so TA-Lib's seeding): the same leading NaNs, the
same SMA seeds, the same Wilder smoothing, and values equal to pandas-ta's to within a few
units in the last place (most are bit for bit identical).

Input can be a numpy array, a pandas Series or anything ``numpy.asarray`` accepts. A pandas
Series comes back as a Series with the same index; everything else comes back as numpy
float64 arrays. numpy is required for this module.
"""
from __future__ import annotations

from typing import Any, Tuple

import numpy as np

from . import _fin_primitives as _native

__all__ = [
    "sma",
    "ema",
    "rma",
    "rsi",
    "true_range",
    "atr",
    "macd",
    "stdev",
    "variance",
    "bbands",
]


def _arr(x: Any, what: str) -> np.ndarray:
    # No copy when x is already a contiguous float64 array (or a Series backed by one).
    a = np.ascontiguousarray(x.to_numpy() if hasattr(x, "to_numpy") else x, dtype=np.float64)
    if a.ndim != 1:
        raise ValueError(f"{what} must be one-dimensional, got shape {a.shape}")
    return a


def _like(values: np.ndarray, like: Any, name: str) -> Any:
    index = getattr(like, "index", None)
    if index is not None and type(like).__module__.startswith("pandas"):
        import pandas as pd

        return pd.Series(values, index=index, name=name, copy=False)
    return values


def sma(close: Any, period: int = 20) -> Any:
    """Simple moving average; the first value is at index ``period - 1``.

    Same as ``pandas_ta.sma`` / ``close.rolling(period).mean()``: a Kahan-compensated
    running sum, so long series do not drift. A window holding a NaN reads NaN.
    """
    return _like(_native.fast_sma(_arr(close, "close"), int(period)), close, f"SMA_{period}")


def ema(close: Any, period: int = 20) -> Any:
    """Exponential moving average, ``alpha = 2 / (period + 1)``, seeded with the SMA of the
    first ``period`` values (``pandas_ta.ema``, TA-Lib ``EMA``)."""
    return _like(_native.fast_ema(_arr(close, "close"), int(period)), close, f"EMA_{period}")


def rma(close: Any, period: int = 14) -> Any:
    """Wilder's moving average, ``alpha = 1 / period``, SMA seed (``pandas_ta.rma``)."""
    return _like(_native.fast_rma(_arr(close, "close"), int(period)), close, f"RMA_{period}")


def rsi(close: Any, period: int = 14) -> Any:
    """Relative Strength Index with Wilder smoothing; the first value is at index
    ``period`` (``pandas_ta.rsi``, TA-Lib ``RSI``)."""
    return _like(_native.fast_rsi(_arr(close, "close"), int(period)), close, f"RSI_{period}")


def true_range(high: Any, low: Any, close: Any, *, seed: str = "talib") -> Any:
    """True range (``pandas_ta.true_range``). ``seed="tradingview"`` gives the first bar
    ``high - low`` instead of NaN."""
    out = _native.fast_true_range(_arr(high, "high"), _arr(low, "low"), _arr(close, "close"), seed)
    return _like(out, close, "TRUERANGE_1")


def atr(high: Any, low: Any, close: Any, period: int = 14, *, seed: str = "talib") -> Any:
    """Average True Range with Wilder smoothing.

    ``seed="talib"`` (default) matches ``pandas_ta.atr`` and TA-Lib ``ATR``: the first value
    is at index ``period``, from the mean true range of bars 1 to ``period``.
    ``seed="tradingview"`` matches TradingView's ``ta.atr``: bar 0's true range is
    ``high - low`` and the first value is at index ``period - 1``.
    """
    out = _native.fast_atr(_arr(high, "high"), _arr(low, "low"), _arr(close, "close"), int(period), seed)
    return _like(out, close, f"ATRr_{period}")


def macd(close: Any, fast: int = 12, slow: int = 26, signal: int = 9) -> Tuple[Any, Any, Any]:
    """MACD as ``(line, signal, histogram)`` (``pandas_ta.macd``, TA-Lib ``MACD``).

    The line starts at index ``slow - 1``, the signal and histogram at
    ``slow + signal - 2``. Rows where close is not finite read NaN and are skipped.
    """
    line, sig, hist = _native.fast_macd(_arr(close, "close"), int(fast), int(slow), int(signal))
    p = f"_{fast}_{slow}_{signal}"
    return _like(line, close, "MACD" + p), _like(sig, close, "MACDs" + p), _like(hist, close, "MACDh" + p)


def stdev(close: Any, period: int = 30, ddof: int = 0) -> Any:
    """Rolling standard deviation in one pass (``pandas_ta.stdev``; ``ddof=0`` is the
    population deviation, ``ddof=1`` pandas' ``rolling().std()``)."""
    return _like(_native.fast_stdev(_arr(close, "close"), int(period), int(ddof)), close, f"STDEV_{period}")


def variance(close: Any, period: int = 30, ddof: int = 0) -> Any:
    """Rolling variance in one pass (``pandas_ta.variance``)."""
    return _like(_native.fast_variance(_arr(close, "close"), int(period), int(ddof)), close, f"VAR_{period}")


def bbands(close: Any, period: int = 20, std: float = 2.0, ddof: int = 0) -> Tuple[Any, Any, Any]:
    """Bollinger Bands as ``(lower, mid, upper)``: SMA middle band, ``std`` rolling standard
    deviations (``ddof=0``, as ``pandas_ta.bbands`` and TA-Lib) either side."""
    lower, mid, upper = _native.fast_bbands(_arr(close, "close"), int(period), float(std), int(ddof))
    p = f"_{period}_{float(std)}"
    return _like(lower, close, "BBL" + p), _like(mid, close, "BBM" + p), _like(upper, close, "BBU" + p)
