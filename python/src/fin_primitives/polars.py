"""``pl.col("close").fp.rsi(14)``: a Polars expression namespace, and ``df.fp`` for frames.

    >>> import polars as pl
    >>> import fin_primitives.polars  # registers the namespaces
    >>> df.with_columns(
    ...     pl.col("close").fp.rsi(14).alias("rsi"),
    ...     pl.col("close").fp.ema(50).over("symbol").alias("ema50"),
    ... )
    >>> df.fp.atr(14)          # needs high, low, close: a DataFrame namespace
    >>> df.fp.macd()           # MACD_12_26_9, MACDh_12_26_9, MACDs_12_26_9
    >>> df.fp.bbands(20, 2.0)  # BBL, BBM, BBU, BBB, BBP

The expressions run :mod:`fin_primitives.fast` (one Rust pass over float64) on each column
or group, so ``.over("symbol")`` computes per symbol. Values and warm-up NaNs are
pandas-ta-classic 0.9.0's, with the same defaults as :mod:`fin_primitives.pandas`; nulls
are read as NaN. Needs polars 0.20 or newer and numpy.
"""
from __future__ import annotations

from typing import Any, Callable, Optional

import numpy as np
import polars as pl

import inspect

from . import _pta
from . import fast

__all__ = ["FinPrimitivesExpr", "FinPrimitivesFrame", "EXPR_METHODS"]


def _np(s: pl.Series) -> np.ndarray:
    return s.cast(pl.Float64).fill_null(float("nan")).to_numpy()


def _series(name: str, values: np.ndarray) -> pl.Series:
    return pl.Series(name, values, dtype=pl.Float64)


@pl.api.register_expr_namespace("fp")
class FinPrimitivesExpr:
    """Single-column indicators as expressions: ``pl.col("close").fp.<name>(...)``."""

    def __init__(self, expr: pl.Expr) -> None:
        self._expr = expr

    def _map(self, f: Callable[[np.ndarray], np.ndarray]) -> pl.Expr:
        return self._expr.map_batches(lambda s: _series(s.name, f(_np(s))), return_dtype=pl.Float64)

    def sma(self, length: int = 10) -> pl.Expr:
        """Simple moving average (pandas-ta ``sma``, default 10)."""
        return self._map(lambda a: fast.sma(a, int(length)))

    def ema(self, length: int = 10) -> pl.Expr:
        """Exponential moving average with an SMA seed (pandas-ta ``ema``, default 10)."""
        return self._map(lambda a: fast.ema(a, int(length)))

    def rma(self, length: int = 10) -> pl.Expr:
        """Wilder's moving average (pandas-ta ``rma``, default 10)."""
        return self._map(lambda a: fast.rma(a, int(length)))

    def rsi(self, length: int = 14) -> pl.Expr:
        """Relative Strength Index, Wilder smoothing (pandas-ta ``rsi``, default 14)."""
        return self._map(lambda a: fast.rsi(a, int(length)))

    def stdev(self, length: int = 30, ddof: int = 0) -> pl.Expr:
        """Rolling standard deviation (pandas-ta ``stdev``, default 30, ``ddof=0``)."""
        return self._map(lambda a: fast.stdev(a, int(length), int(ddof)))

    def variance(self, length: int = 30, ddof: int = 0) -> pl.Expr:
        """Rolling variance (pandas-ta ``variance``, default 30, ``ddof=0``)."""
        return self._map(lambda a: fast.variance(a, int(length), int(ddof)))

    def macd_line(self, fast_: int = 12, slow: int = 26, signal: int = 9) -> pl.Expr:
        """The MACD line alone, as an expression (``df.fp.macd()`` gives all three)."""
        return self._map(lambda a: fast.macd(a, int(fast_), int(slow), int(signal))[0])


@pl.api.register_dataframe_namespace("fp")
class FinPrimitivesFrame:
    """Indicators that need several columns, or return several: ``df.fp.<name>(...)``.
    Columns are found by name in any case (``high``, ``low``, ``close``); pass ``close=``
    and friends to use others."""

    def __init__(self, df: pl.DataFrame) -> None:
        self._df = df

    def _col(self, name: str, given: Optional[str]) -> np.ndarray:
        if given is not None:
            return _np(self._df[given])
        for c in self._df.columns:
            if c.lower() == name:
                return _np(self._df[c])
        raise KeyError(f"no '{name}' column; pass {name}='your column'")

    def true_range(self, *, high: Optional[str] = None, low: Optional[str] = None, close: Optional[str] = None) -> pl.Series:
        """True range, ``TRUERANGE_1``."""
        return _series("TRUERANGE_1", fast.true_range(self._col("high", high), self._col("low", low), self._col("close", close)))

    def atr(self, length: int = 14, *, high: Optional[str] = None, low: Optional[str] = None, close: Optional[str] = None) -> pl.Series:
        """Average True Range, Wilder smoothing, ``ATRr_{length}``."""
        n = int(length)
        return _series(f"ATRr_{n}", fast.atr(self._col("high", high), self._col("low", low), self._col("close", close), n))

    def macd(self, fast_: int = 12, slow: int = 26, signal: int = 9, *, close: Optional[str] = None) -> pl.DataFrame:
        """``MACD_f_s_g``, ``MACDh_f_s_g``, ``MACDs_f_s_g``."""
        f, s, g = int(fast_), int(slow), int(signal)
        if f > s:
            f, s = s, f
        line, sig, hist = fast.macd(self._col("close", close), f, s, g)
        p = f"_{f}_{s}_{g}"
        return pl.DataFrame([_series("MACD" + p, line), _series("MACDh" + p, hist), _series("MACDs" + p, sig)])

    def bbands(self, length: int = 5, std: float = 2.0, ddof: int = 0, *, close: Optional[str] = None) -> pl.DataFrame:
        """``BBL``, ``BBM``, ``BBU``, ``BBB`` (bandwidth %) and ``BBP`` (%B), ``_{length}_{std}``."""
        n, k = int(length), float(std)
        c = self._col("close", close)
        lower, mid, upper = fast.bbands(c, n, k, int(ddof))
        with np.errstate(divide="ignore", invalid="ignore"):
            width = 100 * (upper - lower) / mid
            percent = (c - lower) / (upper - lower)
        p = f"_{n}_{k}"
        return pl.DataFrame([_series("BBL" + p, lower), _series("BBM" + p, mid), _series("BBU" + p, upper), _series("BBB" + p, width), _series("BBP" + p, percent)])

    def with_indicators(self, *frames: Any) -> pl.DataFrame:
        """``df`` with the given Series or DataFrames added as columns:
        ``df.fp.with_indicators(df.fp.atr(), df.fp.macd())``."""
        out = self._df
        for f in frames:
            out = out.with_columns(f) if isinstance(f, pl.Series) else out.hstack(f)
        return out


# -- the pandas-ta indicators in fin_primitives._pta -------------------------------------

#: Close-only, single-column indicators, available as ``pl.col("close").fp.<name>(...)``.
EXPR_METHODS = tuple(
    m for m, (inputs, _) in _pta.SPEC.items() if inputs == ("close",) and m not in _pta.MULTI
)


def _param_names(name: str) -> list:
    inputs, fn = _pta.SPEC[name]
    return [p for p in inspect.signature(fn).parameters if p not in inputs and p != "open_"]


def _bind(name: str, args: tuple, kwargs: dict) -> dict:
    params = _param_names(name)
    if len(args) > len(params):
        raise TypeError(f"{name}() takes at most {len(params)} positional arguments")
    given = dict(zip(params, args))
    for k, v in kwargs.items():
        if k not in params or k in given:
            raise TypeError(f"{name}() got an unexpected or repeated argument {k!r}")
        given[k] = v
    return given


def _make_expr(name: str) -> Any:
    def method(self: FinPrimitivesExpr, *args: Any, **kwargs: Any) -> pl.Expr:
        given = _bind(name, args, kwargs)
        return self._map(lambda a: _pta.SPEC[name][1](np.ascontiguousarray(a), **given)[0][1])

    method.__name__ = name
    method.__doc__ = f"pandas-ta's ``{name}`` on this column (same defaults and values)."
    return method


def _make_frame(name: str) -> Any:
    inputs, fn = _pta.SPEC[name]

    def method(self: FinPrimitivesFrame, *args: Any, **kwargs: Any) -> Any:
        cols = {c: kwargs.pop(c, None) for c in ("open", "high", "low", "close", "volume")}
        given = _bind(name, args, kwargs)
        arrays = [np.ascontiguousarray(self._col(c, cols[c])) for c in inputs]
        out = fn(*arrays, **given)
        if name in _pta.MULTI:
            return pl.DataFrame([_series(k, v) for k, v in out])
        return _series(*out[0])

    method.__name__ = name
    method.__doc__ = (
        f"pandas-ta's ``{name}`` (needs {', '.join(inputs)}): a Series, or a DataFrame for "
        "indicators with several columns, named as pandas-ta names them."
    )
    return method


for _name in EXPR_METHODS:
    if not hasattr(FinPrimitivesExpr, _name):
        setattr(FinPrimitivesExpr, _name, _make_expr(_name))
for _name in _pta.SPEC:
    if not hasattr(FinPrimitivesFrame, _name):
        setattr(FinPrimitivesFrame, _name, _make_frame(_name))
