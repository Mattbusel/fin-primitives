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
with Rust kernels over float64 (:mod:`fin_primitives.fast`, GIL released) and gives
pandas-ta-classic 0.9.0's numbers: the same warm-up NaNs and seeds, values equal to within a
few units in the last place (``tests/test_accessors.py`` checks every method against it).

Methods (pandas-ta defaults): ``sma(10)``, ``ema(10)``, ``rma(10)``, ``rsi(14)``,
``atr(14)``, ``true_range()``, ``macd(12, 26, 9)``, ``bbands(5, 2.0)``, ``stdev(30)``,
``variance(30)``, and from :mod:`fin_primitives._pta`: ``wma``, ``hma``, ``dema``, ``tema``,
``t3``, ``trima``, ``zlma``, ``vwma``, ``vwap``, ``midpoint``, ``midprice``, ``linreg``,
``kama``, ``hl2``, ``hlc3``, ``ohlc4``, ``supertrend``, ``ichimoku``, ``mom``, ``eri``, ``kdj``,
``fisher``, ``bias``, ``slope``, ``roc``, ``cmo``,
``stoch``, ``stochrsi``, ``willr``, ``cci``, ``ao``, ``apo``, ``ppo``, ``trix``, ``tsi``,
``uo``, ``bop``, ``qqe``, ``adx``, ``aroon``, ``psar``, ``vortex``, ``chop``, ``dpo``,
``natr``, ``kc``, ``donchian``, ``mad``, ``zscore``, ``log_return``, ``percent_return``,
``obv``, ``mfi``, ``ad``, ``adosc``, ``cmf``, ``efi``, ``pvt``. Any of the 743
decimal-exact indicators runs through ``df.fp.compute("kind", *params)``.

Several at once, as pandas-ta's ``df.ta.strategy``: ``df.fp.strategy()`` runs them all,
``df.fp.strategy("momentum")`` one category, and ``df.fp.strategy(Strategy(...))`` your own
list (:class:`Strategy`, :data:`CommonStrategy`, :data:`AllStrategy`), appending every column.

Arguments pandas-ta takes that change the formula (``talib=``, ``mamode=``, ``drift=``,
``offset=``, ``scalar=``) are not supported and raise ``TypeError`` rather than being ignored.
"""
from __future__ import annotations

import inspect
import logging
import time
import warnings
from dataclasses import dataclass, field
from datetime import datetime, timezone
from typing import Any, Dict, List, Optional, Sequence, Tuple

import numpy as np
import pandas as pd

from . import _pta
from . import compute as _compute
from . import fast

__all__ = ["AllStrategy", "CATEGORIES", "CommonStrategy", "FinPrimitivesAccessor", "FinPrimitivesSeriesAccessor", "Strategy"]

_log = logging.getLogger(__name__)

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


# -- the pandas-ta indicators in fin_primitives._pta, as df.fp methods -------------------


def _params(fn: Any) -> list:
    sig = inspect.signature(fn)
    inputs = _pta.SPEC[fn.__name__][0]
    return [p for p in sig.parameters if p not in inputs and p not in ("open_",) and p not in _COLUMNS]


def _make(name: str) -> Any:
    inputs, fn = _pta.SPEC[name]
    params = _params(fn)
    optional_close = name == "psar"

    def method(self: "_Base", *args: Any, append: bool = False, **kwargs: Any) -> Any:
        if len(args) > len(params):
            raise TypeError(f"{name}() takes at most {len(params)} positional arguments")
        given = dict(zip(params, args))
        for k in list(kwargs):
            if k in params:
                if k in given:
                    raise TypeError(f"{name}() got two values for {k!r}")
                given[k] = kwargs.pop(k)
        _check(kwargs)
        cols = {c: self._col(c, kwargs) for c in inputs if not (optional_close and c == "close")}
        if optional_close and self._has("close", kwargs):
            cols["close"] = self._col("close", kwargs)
        ref = next(iter(cols.values()))
        arrays = {c: np.ascontiguousarray(v.to_numpy(dtype=np.float64, na_value=np.nan)) for c, v in cols.items()}
        out = _pta.SPEC[name][1](*[arrays.get(c) for c in inputs], **given) if not optional_close else _pta.psar(
            arrays["high"], arrays["low"], arrays.get("close"), **given
        )
        index = ref.index
        if name in _pta.MULTI:
            df = pd.DataFrame({k: v for k, v in out}, index=index)
            df.name = out[0][0]  # type: ignore[attr-defined]
            return self._out(df, append)
        col, values = out[0]
        return self._out(pd.Series(values, index=index, name=col), append)

    method.__name__ = name
    method.__qualname__ = f"_Base.{name}"
    shown = ", ".join(f"{p}=None" for p in params)
    method.__doc__ = (
        f"pandas-ta's ``{name}({shown})``: same defaults, same column names, same values "
        f"(needs {', '.join(inputs)}).\n\n" + (inspect.getdoc(fn) or "")
    )
    return method


for _name in _pta.SPEC:
    if not hasattr(_Base, _name):
        setattr(_Base, _name, _make(_name))


def _vwap(self: "_Base", anchor: Optional[str] = None, append: bool = False, **kwargs: Any) -> pd.Series:
    """Volume-weighted average price reset every ``anchor`` period (a pandas period alias,
    default ``"D"``), ``VWAP_{anchor}``. Needs a sorted DatetimeIndex, as pandas-ta."""
    _check(kwargs)
    a = (anchor or "D").upper()
    high, low, close, volume = (self._col(c, kwargs) for c in ("high", "low", "close", "volume"))
    if not isinstance(close.index, pd.DatetimeIndex):
        raise TypeError(f"vwap() needs a DatetimeIndex to anchor by {a!r}, got {type(close.index).__name__}")
    if not close.index.is_monotonic_increasing:
        raise ValueError("vwap() index must be in ascending time order; sort it first (df.sort_index())")
    wp = (high + low + close) / 3.0 * volume
    clock = wp.index.tz_localize(None) if wp.index.tz is not None else wp.index
    periods = clock.to_period(a)
    out = wp.groupby(periods, observed=True).cumsum()
    out /= volume.groupby(periods, observed=True).cumsum()
    return self._out(out.rename(f"VWAP_{a}"), append)


_Base.vwap = _vwap  # type: ignore[attr-defined]


# -- strategy(): pandas-ta's df.ta.strategy over the df.fp methods ---------------------------


@dataclass
class Strategy:
    """A named list of indicator calls, as pandas-ta's ``Strategy``: each entry of ``ta`` is a
    dict with ``kind`` (the method name) and its keyword arguments, plus optional ``params``
    (a tuple of positional arguments), ``prefix``, ``suffix``, ``delimiter`` (default ``_``)
    and ``col_names`` (new names for the result's columns). ``ta=None`` runs every
    indicator, like ``df.fp.strategy("all")``.

    >>> MyStrategy = Strategy(name="Momo", ta=[{"kind": "rsi"}, {"kind": "macd", "fast": 8, "slow": 21}])
    """

    name: str
    ta: Optional[List[Dict[str, Any]]] = field(default_factory=list)
    description: str = "TA Description"
    created: Optional[datetime] = field(default_factory=lambda: datetime.now(timezone.utc))

    def __post_init__(self) -> None:
        if not isinstance(self.name, str):
            raise TypeError("Strategy name must be a string, for example 'My TA'")
        if self.ta is not None:
            if not isinstance(self.ta, list) or not all(isinstance(t, dict) and isinstance(t.get("kind"), str) for t in self.ta):
                raise TypeError("Strategy ta must be a list of dicts with a 'kind', for example [{'kind': 'sma', 'length': 10}]")


AllStrategy = Strategy(name="All", description="All the indicators with their default settings.", ta=None)

CommonStrategy = Strategy(
    name="Common Price and Volume SMAs",
    description="Common Price SMAs: 10, 20, 50, 200 and Volume SMA: 20.",
    ta=[
        {"kind": "sma", "length": 10},
        {"kind": "sma", "length": 20},
        {"kind": "sma", "length": 50},
        {"kind": "sma", "length": 200},
        {"kind": "sma", "close": "volume", "length": 20, "prefix": "VOL"},
    ],
)

#: pandas-ta's categories, restricted to the indicators df.fp has.
CATEGORIES: Dict[str, List[str]] = {
    "momentum": [
        "ao", "apo", "bias", "bop", "cci", "cmo", "eri", "fisher", "kdj", "macd", "mom", "ppo", "qqe", "roc", "rsi", "slope",
        "stoch", "stochrsi", "trix", "tsi", "uo", "willr",
    ],
    "overlap": [
        "dema", "ema", "hl2", "hlc3", "hma", "ichimoku", "kama", "linreg", "midpoint", "midprice", "ohlc4", "rma", "sma",
        "supertrend", "t3", "tema", "trima", "vwap", "vwma", "wma", "zlma",
    ],
    "performance": ["log_return", "percent_return"],
    "statistics": ["mad", "stdev", "variance", "zscore"],
    "trend": ["adx", "aroon", "chop", "dpo", "psar", "vortex"],
    "volatility": ["atr", "bbands", "donchian", "kc", "natr", "true_range"],
    "volume": ["ad", "adosc", "cmf", "efi", "mfi", "obv", "pvt"],
}

_BUILTIN_INPUTS = {"atr": ("high", "low", "close"), "true_range": ("high", "low", "close"), "vwap": ("high", "low", "close", "volume")}
_ENTRY_KEYS = ("kind", "params", "prefix", "suffix", "delimiter", "col_names")


def _inputs(kind: str) -> Tuple[str, ...]:
    if kind in _pta.SPEC:
        return tuple("open" if c == "open_" else c for c in _pta.SPEC[kind][0])
    return _BUILTIN_INPUTS.get(kind, ("close",))


def _rename(result: Any, entry: Dict[str, Any]) -> Any:
    delimiter = entry.get("delimiter", "_")
    prefix = f"{entry['prefix']}{delimiter}" if "prefix" in entry else ""
    suffix = f"{delimiter}{entry['suffix']}" if "suffix" in entry else ""
    names = entry.get("col_names")
    if names is not None and not isinstance(names, tuple):
        names = (names,)
    if isinstance(result, pd.DataFrame):
        result = result.rename(columns=lambda c: prefix + c + suffix)
        if names is not None:
            if len(names) != len(result.columns):
                raise ValueError(f"col_names has {len(names)} name(s) for {len(result.columns)} column(s): {list(result.columns)}")
            result.columns = list(names)
        return result
    result = result.rename(prefix + str(result.name) + suffix)
    if names is not None:
        if len(names) != 1:
            raise ValueError(f"col_names has {len(names)} names for one column ({result.name})")
        result = result.rename(names[0])
    return result


def _indicators(self: "_Base", as_list: bool = True, exclude: Optional[Sequence[str]] = None) -> Any:
    """The indicator names ``strategy("all")`` runs, sorted, without ``exclude``."""
    names = sorted({n for names in CATEGORIES.values() for n in names} - set(exclude or ()))
    return names if as_list else ", ".join(names)


def _run(self: "_Base", entries: List[Dict[str, Any]], kwargs: Dict[str, Any], name: str, verbose: bool) -> None:
    for e in entries:
        call = {k: v for k, v in e.items() if k not in _ENTRY_KEYS}
        params = e.get("params", ())
        if not isinstance(params, tuple):
            raise TypeError(f"Strategy entry {e['kind']!r}: params must be a tuple, got {type(params).__name__} {params!r}")
        for k, v in kwargs.items():
            call.setdefault(k, v)
        result = getattr(self, e["kind"])(*params, **call)
        self._out(_rename(result, e), True)
        if verbose:
            _log.info("strategy %s: %s", name, e["kind"])


def _strategy(self: "_Base", *args: Any, **kwargs: Any) -> Any:
    """pandas-ta's ``df.ta.strategy``: runs ``"all"`` (the default), a category
    (``"momentum"``, ``"overlap"``, ``"performance"``, ``"statistics"``, ``"trend"``,
    ``"volatility"``, ``"volume"``) or a :class:`Strategy`, appending every result to the
    DataFrame under pandas-ta's column names.

    Keywords: ``exclude`` (indicator names to leave out of ``"all"`` or a category),
    ``returns`` (return the DataFrame), ``timed`` and ``verbose`` (log progress), ``cores``
    (accepted for compatibility; the Rust kernels run serially, which is faster than
    pandas-ta's process pool at any frame size), and column names such as ``close="adj"``,
    passed to every indicator. ``vwap`` is left out without a DatetimeIndex, and indicators
    needing a column the frame lacks (``volume``) are skipped, as in pandas-ta.

    >>> df.fp.strategy(CommonStrategy)          # SMA_10, SMA_20, SMA_50, SMA_200, VOL_SMA_20
    >>> df.fp.strategy("momentum", exclude=["qqe"])
    """
    if not isinstance(self._obj, pd.DataFrame):
        raise ValueError("strategy() needs a DataFrame")
    returns = bool(kwargs.pop("returns", False))
    timed = bool(kwargs.pop("timed", False))
    verbose = bool(kwargs.pop("verbose", False))
    cores = kwargs.pop("cores", None)
    if cores is not None and (not isinstance(cores, int) or cores < 0):
        raise TypeError(f"strategy() cores must be a non-negative int, got {cores!r}")
    kwargs.pop("append", None)
    exclude = kwargs.pop("exclude", None)
    _check(kwargs)
    if len(args) > 1:
        raise TypeError("strategy() takes one strategy: 'all', a category name or a Strategy")
    arg = args[0] if args else "all"
    known = set(_indicators(self))
    custom = False
    if isinstance(arg, Strategy):
        if arg.ta is None:
            arg = "all"
        else:
            custom = True
    if custom:
        if exclude is not None:
            raise ValueError(f"strategy() exclude does not apply to the custom Strategy {arg.name!r}; leave the entries out of its 'ta' list instead")
        entries = [dict(e) for e in arg.ta]
        for position, e in enumerate(entries):
            if not hasattr(_Base, e["kind"]) or e["kind"] in ("compute", "strategy", "study", "indicators"):
                raise ValueError(f"Strategy {arg.name!r} entry {position}: {e['kind']!r} is not an indicator df.fp has")
            if not isinstance(e.get("params", ()), tuple):
                raise TypeError(f"Strategy entry {e['kind']!r}: params must be a tuple, got {type(e['params']).__name__} {e['params']!r}")
        name = arg.name
    elif isinstance(arg, str):
        key = arg.lower()
        if key == "all":
            kinds = _indicators(self)
        elif key in CATEGORIES:
            kinds = list(CATEGORIES[key])
        else:
            raise ValueError(f"strategy() got {arg!r}, which is neither 'all' nor a category ({', '.join(sorted(CATEGORIES))}). Pass a Strategy to run a named one.")
        if exclude is not None:
            if not isinstance(exclude, (list, tuple, set)) or not all(isinstance(x, str) for x in exclude):
                raise TypeError(f"strategy() exclude must be a list of indicator names, got {exclude!r}")
            unknown = sorted(set(exclude) - known)
            if unknown:
                raise ValueError(f"strategy() exclude has unknown indicator name(s): {unknown}")
            kinds = [k for k in kinds if k not in set(exclude)]
        if not isinstance(self._obj.index, pd.DatetimeIndex):
            kinds = [k for k in kinds if k != "vwap"]
        kinds = [k for k in kinds if all(self._has(c, kwargs) for c in _inputs(k) if not (k == "psar" and c == "close"))]
        entries = [{"kind": k} for k in kinds]
        name = arg
    else:
        raise TypeError(f"strategy() expected a category name or a Strategy, got {type(arg).__name__}")

    start = time.perf_counter()
    before = len(self._obj.columns)
    # Appending column by column fragments the frame; pandas warns past 100 blocks.
    with warnings.catch_warnings():
        warnings.simplefilter("ignore", pd.errors.PerformanceWarning)
        _run(self, entries, kwargs, name, verbose)
    if verbose:
        _log.info("strategy %s: %d indicators, %d columns added", name, len(entries), len(self._obj.columns) - before)
    if timed:
        _log.info("strategy %s: %.1f ms", name, (time.perf_counter() - start) * 1000)
    return self._obj if returns else None


_Base.strategy = _strategy  # type: ignore[attr-defined]
_Base.study = _strategy  # type: ignore[attr-defined]  # pandas-ta's older name
_Base.indicators = _indicators  # type: ignore[attr-defined]
_Base.categories = property(lambda self: sorted(CATEGORIES))  # type: ignore[attr-defined]
_Base.cores = 0  # type: ignore[attr-defined]  # df.fp.cores = n is accepted, as df.ta.cores


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
