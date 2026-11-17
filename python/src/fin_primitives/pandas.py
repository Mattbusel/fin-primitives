"""``df.fp``: a pandas accessor with pandas-ta's method names, defaults and column names.

Code written for pandas-ta's ``df.ta`` moves over by changing ``ta`` to ``fp`` after one
import::

    >>> import fin_primitives.pandas  # registers df.fp and series.fp
    >>> df.fp.rsi()                    # was df.ta.rsi(): Series "RSI_14"
    >>> df.fp.macd(append=True)        # adds MACD_12_26_9, MACDh_12_26_9, MACDs_12_26_9
    >>> df.fp.bbands(length=20, std=2) # BBL_20_2.0, BBM_20_2.0, BBU_20_2.0, BBB_20_2.0, BBP_20_2.0
    >>> df["close"].fp.ema(50)         # on a Series too

The columns are found the way pandas-ta finds them: ``open``, ``high``, ``low``, ``close``,
``volume`` in any case, or set them with ``close=...`` and friends. Every method computes
with :mod:`fin_primitives.fast` (one Rust pass over float64, GIL released) and gives
pandas-ta-classic 0.9.0's numbers: the same warm-up NaNs and seeds, values equal to within a
few units in the last place (``tests/test_accessors.py`` checks every method against it).

Methods (pandas-ta defaults): ``sma(10)``, ``ema(10)``, ``rma(10)``, ``rsi(14)``,
``atr(14)``, ``true_range()``, ``macd(12, 26, 9)``, ``bbands(5, 2.0)``, ``stdev(30)``,
``variance(30)``. Any of the 743 decimal-exact indicators runs through
``df.fp.compute("kind", *params)``.

Arguments pandas-ta takes that change the formula (``talib=``, ``mamode=``, ``drift=``,
``offset=``, ``scalar=``) are not supported and raise ``TypeError`` rather than being ignored.
"""
from __future__ import annotations

from typing import Any, Dict, Optional

import pandas as pd

from . import compute as _compute
from . import fast

__all__ = ["FinPrimitivesAccessor", "FinPrimitivesSeriesAccessor"]

_COLUMNS = ("open", "high", "low", "close", "volume")
_UNSUPPORTED = ("talib", "mamode", "drift", "offset", "scalar", "asc")


def _check(kwargs: Dict[str, Any]) -> None:
    bad = [k for k in kwargs if k in _UNSUPPORTED or k not in _COLUMNS]
    if bad:
        raise TypeError(f"unsupported argument(s) {', '.join(sorted(bad))}; fin_primitives.pandas computes the default formula only")


class _Base:
    _obj: Any

    def _col(self, name: str, kwargs: Dict[str, Any]) -> pd.Series:
        given = kwargs.get(name)
        if given is not None:
            return self._obj[given] if isinstance(given, str) else given
        if isinstance(self._obj, pd.Series):
            if name == "close":
                return self._obj
            raise ValueError(f"{name} is needed; call this on a DataFrame or pass {name}=")
        for c in self._obj.columns:
            if isinstance(c, str) and c.lower() == name:
                return self._obj[c]
        raise KeyError(f"no '{name}' column; pass {name}='your column'")

    def _out(self, result: Any, append: bool) -> Any:
        if append:
            if not isinstance(self._obj, pd.DataFrame):
                raise ValueError("append=True needs a DataFrame")
            if isinstance(result, pd.DataFrame):
                for c in result.columns:
                    self._obj[c] = result[c]
            else:
                self._obj[result.name] = result
        return result

    # -- moving averages -------------------------------------------------------------------

    def sma(self, length: Optional[int] = None, append: bool = False, **kwargs: Any) -> pd.Series:
        """Simple moving average, ``SMA_{length}`` (default 10)."""
        _check(kwargs)
        n = int(length or 10)
        return self._out(fast.sma(self._col("close", kwargs), n).rename(f"SMA_{n}"), append)

    def ema(self, length: Optional[int] = None, append: bool = False, **kwargs: Any) -> pd.Series:
        """Exponential moving average with an SMA seed, ``EMA_{length}`` (default 10)."""
        _check(kwargs)
        n = int(length or 10)
        return self._out(fast.ema(self._col("close", kwargs), n).rename(f"EMA_{n}"), append)

    def rma(self, length: Optional[int] = None, append: bool = False, **kwargs: Any) -> pd.Series:
        """Wilder's moving average, ``RMA_{length}`` (default 10)."""
        _check(kwargs)
        n = int(length or 10)
        return self._out(fast.rma(self._col("close", kwargs), n).rename(f"RMA_{n}"), append)

    # -- momentum and volatility -----------------------------------------------------------

    def rsi(self, length: Optional[int] = None, append: bool = False, **kwargs: Any) -> pd.Series:
        """Relative Strength Index, Wilder smoothing, ``RSI_{length}`` (default 14)."""
        _check(kwargs)
        n = int(length or 14)
        return self._out(fast.rsi(self._col("close", kwargs), n).rename(f"RSI_{n}"), append)

    def true_range(self, append: bool = False, **kwargs: Any) -> pd.Series:
        """True range, ``TRUERANGE_1`` (NaN on the first bar, as pandas-ta)."""
        _check(kwargs)
        tr = fast.true_range(self._col("high", kwargs), self._col("low", kwargs), self._col("close", kwargs))
        return self._out(tr.rename("TRUERANGE_1"), append)

    def atr(self, length: Optional[int] = None, append: bool = False, **kwargs: Any) -> pd.Series:
        """Average True Range, Wilder smoothing (pandas-ta's default ``mamode="rma"``),
        ``ATRr_{length}`` (default 14)."""
        _check(kwargs)
        n = int(length or 14)
        out = fast.atr(self._col("high", kwargs), self._col("low", kwargs), self._col("close", kwargs), n)
        return self._out(out.rename(f"ATRr_{n}"), append)

    def macd(
        self, fast: Optional[int] = None, slow: Optional[int] = None, signal: Optional[int] = None, append: bool = False, **kwargs: Any
    ) -> pd.DataFrame:
        """MACD line, histogram and signal: ``MACD_f_s_g``, ``MACDh_f_s_g``, ``MACDs_f_s_g``
        (defaults 12, 26, 9), in pandas-ta's column order."""
        _check(kwargs)
        f, s, g = int(fast or 12), int(slow or 26), int(signal or 9)
        if f > s:
            f, s = s, f  # pandas-ta swaps them too
        line, sig, hist = _fast_macd(self._col("close", kwargs), f, s, g)
        p = f"_{f}_{s}_{g}"
        df = pd.DataFrame({"MACD" + p: line, "MACDh" + p: hist, "MACDs" + p: sig})
        df.name = "MACD" + p  # type: ignore[attr-defined]
        return self._out(df, append)

    def bbands(
        self, length: Optional[int] = None, std: Optional[float] = None, ddof: int = 0, append: bool = False, **kwargs: Any
    ) -> pd.DataFrame:
        """Bollinger Bands with pandas-ta's five columns: ``BBL``, ``BBM``, ``BBU``, ``BBB``
        (bandwidth, percent of the middle band) and ``BBP`` (%B), suffixed
        ``_{length}_{std}`` (defaults 5 and 2.0)."""
        _check(kwargs)
        n, k = int(length or 5), float(std or 2.0)
        close = self._col("close", kwargs)
        lower, mid, upper = fast.bbands(close, n, k, ddof)
        p = f"_{n}_{k}"
        width = 100 * (upper - lower) / mid
        percent = (close - lower) / (upper - lower)
        df = pd.DataFrame({"BBL" + p: lower, "BBM" + p: mid, "BBU" + p: upper, "BBB" + p: width, "BBP" + p: percent})
        df.name = "BBANDS" + p  # type: ignore[attr-defined]
        return self._out(df, append)

    def stdev(self, length: Optional[int] = None, ddof: int = 0, append: bool = False, **kwargs: Any) -> pd.Series:
        """Rolling standard deviation, ``STDEV_{length}`` (default 30, ``ddof=0`` as pandas-ta)."""
        _check(kwargs)
        n = int(length or 30)
        return self._out(fast.stdev(self._col("close", kwargs), n, ddof).rename(f"STDEV_{n}"), append)

    def variance(self, length: Optional[int] = None, ddof: int = 0, append: bool = False, **kwargs: Any) -> pd.Series:
        """Rolling variance, ``VAR_{length}`` (default 30, ``ddof=0`` as pandas-ta)."""
        _check(kwargs)
        n = int(length or 30)
        return self._out(fast.variance(self._col("close", kwargs), n, ddof).rename(f"VAR_{n}"), append)

    # -- everything else -------------------------------------------------------------------

    def compute(self, kind: str, *params: Any, append: bool = False, **kwargs: Any) -> Any:
        """Any of the 743 decimal-exact indicators (``fin_primitives.catalogue()``), fed from
        this frame's columns. The result is named ``KIND_param_...``."""
        cols = {c: self._col(c, kwargs) for c in ("high", "low", "open", "volume") if self._has(c, kwargs)}
        out = _compute(kind, self._col("close", kwargs), *params, **cols)
        name = "_".join([kind.upper(), *[str(p) for p in params]])
        if isinstance(out, pd.Series):
            out = out.rename(name)
        return self._out(out, append)

    def _has(self, name: str, kwargs: Dict[str, Any]) -> bool:
        try:
            self._col(name, kwargs)
            return True
        except (KeyError, ValueError):
            return False


def _fast_macd(close: Any, f: int, s: int, g: int) -> Any:
    return fast.macd(close, f, s, g)


@pd.api.extensions.register_dataframe_accessor("fp")
class FinPrimitivesAccessor(_Base):
    """``df.fp``: see the module docs."""

    def __init__(self, df: pd.DataFrame) -> None:
        self._obj = df


@pd.api.extensions.register_series_accessor("fp")
class FinPrimitivesSeriesAccessor(_Base):
    """``series.fp``: the close-only methods on a Series."""

    def __init__(self, s: pd.Series) -> None:
        self._obj = s
