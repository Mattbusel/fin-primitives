"""Python bindings for fin-primitives: exact decimal trading indicators from Rust.

Every indicator in the Rust crate (743 of them) is available through one entry point::

    >>> from fin_primitives import Indicator
    >>> rsi = Indicator("rsi", 14)
    >>> rsi.update("101.25")          # str, int, float or decimal.Decimal
    >>> rsi.update(101.5)             # None while warming up, then a decimal.Decimal

``mode="float"`` takes and returns floats instead. The arithmetic inside is the same exact
decimal code either way; float mode skips building ``decimal.Decimal`` objects and its batch
path runs over raw float64 buffers with the GIL released.

Batch helpers accept a list, a numpy array, a pandas Series or a polars Series and return the
same type (numpy, pandas and polars are optional; nothing imports them unless you pass one)::

    >>> import fin_primitives as fp
    >>> fp.rsi(df["close"], 14)                       # pandas Series in, pandas Series out
    >>> fp.compute("atr", df["close"], 14, high=df["high"], low=df["low"])

``catalogue()`` lists every indicator key with its parameters and a one-line description.
"""
from __future__ import annotations

import sys
from array import array
from decimal import Decimal
from typing import Any, Dict, List, Optional, Sequence, Tuple, Union

from . import _fin_primitives as _native
from ._fin_primitives import (
    PyEMA,
    PyOHLCV,
    PyOrderBook,
    PyPrice,
    PyQuantity,
    PyRSI,
    PySMA,
    aliases,
    resolve,
)

__all__ = [
    "Indicator",
    "catalogue",
    "aliases",
    "resolve",
    "compute",
    "sma",
    "ema",
    "rsi",
    "macd",
    "bollinger_b",
    "atr",
    "DEFAULTS",
    "Price",
    "Quantity",
    "OrderBook",
    "PyPrice",
    "PyQuantity",
    "PyOHLCV",
    "PyOrderBook",
    "PySMA",
    "PyEMA",
    "PyRSI",
]

__version__: str = _native.__version__

Number = Union[str, int, float, Decimal]

#: Validated price (strictly positive). Accepts str, int, float or decimal.Decimal.
Price = PyPrice
#: Validated quantity (non-negative). Accepts str, int, float or decimal.Decimal.
Quantity = PyQuantity
#: Level-2 order book that rejects crossed books and out-of-order sequence numbers.
OrderBook = PyOrderBook

_CATALOGUE: Dict[str, Dict[str, Any]] = {e["key"]: e for e in _native.catalogue()}

#: Conventional parameters used when an indicator is built with no parameters at all,
#: e.g. ``Indicator("macd")`` is ``Indicator("macd", 12, 26, 9)``.
DEFAULTS: Dict[str, Tuple[Any, ...]] = {
    "sma": (20,),
    "ema": (20,),
    "rsi": (14,),
    "macd": (12, 26, 9),
    "bollinger_b": (20, 2),
    "bollinger_width": (20, 2),
    "atr": (14,),
    "adx": (14,),
    "cci": (20,),
    "stochastic_k": (14,),
    "stochastic_d": (14, 3),
    "williams_r": (14,),
    "mfi": (14,),
    "roc": (12,),
    "momentum": (10,),
    "keltner_channel": (20, 2),
    "parabolic_sar": ("0.02", "0.2"),
    "super_trend": (10, 3),
    "aroon": (25,),
    "cmf": (20,),
    "cmo": (14,),
    "ichimoku": (9, 26, 52),
    "ultimate_oscillator": (7, 14, 28),
    "tsi": (25, 13),
    "ppo": (12, 26),
    "apo": (12, 26),
}

_MODES = ("exact", "float")


def catalogue() -> List[Dict[str, Any]]:
    """Every indicator: ``{"key", "type", "params": [(name, type), ...], "doc"}``.

    ``type`` is the Rust type in ``fin_primitives::signals::indicators``. Parameter types are
    ``"int"``, ``"decimal"`` (str/int/float/Decimal), ``"float"``, ``"float|None"``,
    ``"bool"``, ``"str"`` or ``"int[4]"``.

    The result is a fresh copy each call; changing it does not affect the module.
    """
    return [{**e, "params": [tuple(p) for p in e["params"]]} for e in _CATALOGUE.values()]


def _bind_params(key: str, params: Tuple[Any, ...], kwargs: Dict[str, Any]) -> List[Any]:
    spec = _CATALOGUE[key]["params"]
    names = [n for n, _ in spec]
    if not params and not kwargs and key in DEFAULTS:
        return list(DEFAULTS[key])
    if len(params) > len(names):
        raise TypeError(f"{key}() takes {len(names)} parameter(s) {names}, got {len(params)}")
    args = list(params)
    for name in names[len(params):]:
        if name not in kwargs:
            raise TypeError(f"{key}() missing parameter {name!r}; parameters are {names}")
        args.append(kwargs.pop(name))
    if kwargs:
        dup = [k for k in kwargs if k in names]
        if dup:
            raise TypeError(f"{key}() got multiple values for {dup}")
        raise TypeError(f"{key}() got unexpected keyword(s) {sorted(kwargs)}; parameters are {names}")
    return args


class Indicator:
    """A streaming indicator from the catalogue, chosen by key.

    ``Indicator("rsi", 14)``, ``Indicator("macd", fast_period=12, slow_period=26,
    signal_period=9)`` and ``Indicator("macd")`` (``DEFAULTS``) all work. Keys are the
    snake_case Rust type names (``"bollinger_b"`` for ``BollingerB``); module names such as
    ``"bollinger"`` or ``"psar"`` are accepted as aliases.

    ``mode="exact"`` (default): inputs may be str, int, float or decimal.Decimal and values
    come back as ``decimal.Decimal``. ``mode="float"``: floats in, floats out.

    Values are ``None`` (``NaN`` in numpy/pandas batches, null in polars) until the
    indicator has seen enough bars.

    Inputs must fit ``rust_decimal`` (28 decimal places, about 7.9e28 in magnitude). Extra
    decimal places are rounded, but a non-zero input that would round to zero (``1e-30``)
    raises ``ValueError`` instead of turning into ``0``.

    An ``Indicator`` holds native Rust state, so it cannot be pickled or copied with
    ``copy.copy`` / ``copy.deepcopy`` (``TypeError``). To get a second one, build it again
    with ``Indicator(ind.key, *ind.params, mode=ind.mode)`` and feed it the same data.
    """

    __slots__ = ("_inner", "_mode", "_args")

    def __init__(self, kind: str, *params: Any, mode: str = "exact", **kwargs: Any) -> None:
        if mode not in _MODES:
            raise ValueError(f"mode must be 'exact' or 'float', got {mode!r}")
        key = resolve(kind)
        args = _bind_params(key, params, kwargs)
        self._inner = _native._Indicator(key, *args)
        self._mode = mode
        self._args = tuple(args)

    # -- introspection -------------------------------------------------------------------
    @property
    def key(self) -> str:
        """Catalogue key, e.g. ``"rsi"``."""
        return self._inner.key

    @property
    def type_name(self) -> str:
        """Rust type name, e.g. ``"Rsi"``."""
        return self._inner.type_name

    @property
    def params(self) -> Tuple[Any, ...]:
        """Constructor parameters, in catalogue order."""
        return self._args

    @property
    def mode(self) -> str:
        """``"exact"`` or ``"float"``."""
        return self._mode

    @property
    def period(self) -> int:
        """Look-back period reported by the Rust indicator."""
        return self._inner.period

    @property
    def is_ready(self) -> bool:
        """True once the indicator produces values."""
        return self._inner.is_ready

    @property
    def label(self) -> str:
        """``key`` plus parameters, e.g. ``"macd_12_26_9"``; used to name output Series."""
        return "_".join([self.key] + [str(a) for a in self._args])

    def reset(self) -> None:
        """Clear all state, as if newly constructed."""
        self._inner.reset()

    # -- streaming -----------------------------------------------------------------------
    def update(self, value: Number) -> Optional[Union[Decimal, float]]:
        """Feed one close price; returns the value or ``None`` while warming up."""
        if self._mode == "float":
            return self._inner.update_f64(value)
        return self._inner.update(value)

    def update_bar(
        self,
        close: Number,
        high: Optional[Number] = None,
        low: Optional[Number] = None,
        open: Optional[Number] = None,
        volume: Optional[Number] = None,
    ) -> Optional[Union[Decimal, float]]:
        """Feed one OHLCV bar. ``high``/``low``/``open`` default to ``close``, ``volume`` to 0.

        Range and volume indicators (ATR, ADX, MFI, OBV, VWAP, ...) need the full bar.
        """
        if self._mode == "float":
            return self._inner.update_bar_f64(close, high, low, open, volume)
        return self._inner.update_bar(close, high, low, open, volume)

    # -- batch ---------------------------------------------------------------------------
    def batch(self, close: Any, high: Any = None, low: Any = None, open: Any = None, volume: Any = None) -> Any:
        """Feed a whole series and return one value per input, as the same container type.

        ``close`` (and the optional columns, same length) may be a list/tuple/iterable, a
        1-D numpy array, a pandas Series or a polars Series. Exact mode returns ``Decimal``
        objects (object-dtype numpy/pandas; Decimal polars, or an Object column when the
        values need more than 38 digits); float mode returns float64. State carries over
        between calls, so a stream can be fed in chunks.

        Columns are lined up by position. pandas Series must share the same index (a
        ``ValueError`` otherwise; use ``df[...]`` columns of one frame, or ``.align()`` /
        ``.reset_index(drop=True)`` first). A numpy array with more than one dimension raises
        ``ValueError``.
        """
        return _batch(self, close, high, low, open, volume)

    def __reduce__(self) -> Any:
        raise TypeError(
            f"{self!r} holds native Rust state and cannot be pickled or copied; build a new one "
            "with Indicator(ind.key, *ind.params, mode=ind.mode) and feed it the same data"
        )

    def __copy__(self) -> "Indicator":
        self.__reduce__()
        raise AssertionError  # unreachable

    def __deepcopy__(self, memo: Any) -> "Indicator":
        self.__reduce__()
        raise AssertionError  # unreachable

    def __repr__(self) -> str:
        args = ", ".join(repr(a) for a in self._args)
        sep = ", " if args else ""
        mode = "" if self._mode == "exact" else f", mode={self._mode!r}"
        return f"Indicator({self.key!r}{sep}{args}{mode})"


# -- container handling ------------------------------------------------------------------

def _container(x: Any) -> str:
    """'pandas', 'polars', 'numpy' or 'list', detected without importing anything."""
    pd = sys.modules.get("pandas")
    if pd is not None and isinstance(x, pd.Series):
        return "pandas"
    pl = sys.modules.get("polars")
    if pl is not None and isinstance(x, pl.Series):
        return "polars"
    np = sys.modules.get("numpy")
    if np is not None and isinstance(x, np.ndarray):
        return "numpy"
    return "list"


def _is_float_like(x: Any) -> bool:
    kind = _container(x)
    if kind == "numpy":
        return x.dtype.kind in "fiu"
    if kind == "pandas":
        return x.dtype.kind in "fiu"
    if kind == "polars":
        return x.dtype.is_numeric() and not str(x.dtype).startswith("Decimal")
    return False


def _check_1d(x: Any, kind: str, what: str) -> None:
    if kind == "numpy" and x.ndim != 1:
        raise ValueError(f"{what}: expected a 1-D array, got shape {x.shape}")


def _to_f64_bytes(x: Any, what: str) -> bytes:
    kind = _container(x)
    _check_1d(x, kind, what)
    if kind in ("numpy", "pandas"):
        np = sys.modules["numpy"]
        if kind == "pandas":
            x = x.to_numpy(dtype="float64", na_value=np.nan)
        return np.ascontiguousarray(x, dtype=np.float64).tobytes()
    if kind == "polars":
        pl = sys.modules["polars"]
        values = x.cast(pl.Float64).to_list()
    else:
        values = list(x)
    try:
        return array("d", (float("nan") if v is None else float(v) for v in values)).tobytes()
    except (TypeError, ValueError) as e:
        raise TypeError(f"{what}: {e}") from None


def _to_objects(x: Any, what: str) -> List[Any]:
    kind = _container(x)
    _check_1d(x, kind, what)
    if kind == "numpy":
        return x.tolist()
    if kind == "pandas":
        return x.tolist()
    if kind == "polars":
        return x.to_list()
    return x if isinstance(x, list) else list(x)


def _wrap(values: Any, like: Any, name: str, exact: bool) -> Any:
    """Return ``values`` (a list, or float64 bytes in float mode) shaped like ``like``."""
    kind = _container(like)
    if kind in ("numpy", "pandas"):
        np = sys.modules["numpy"]
        if exact:
            arr = np.empty(len(values), dtype=object)
            arr[:] = values
        else:
            arr = np.frombuffer(values, dtype=np.float64).copy()
        if kind == "numpy":
            return arr
        pd = sys.modules["pandas"]
        return pd.Series(arr, index=like.index, name=name, dtype=object if exact else "float64")
    if not exact:
        floats = array("d")
        floats.frombytes(values)
        values = [None if v != v else v for v in floats]
    if kind == "polars":
        pl = sys.modules["polars"]
        if not exact:
            return pl.Series(name, values, dtype=pl.Float64)
        return _polars_decimals(pl, name, values)
    if isinstance(like, tuple):
        return tuple(values)
    return values


def _polars_decimals(pl: Any, name: str, values: List[Any]) -> Any:
    """A polars Series of ``Decimal`` values (or ``None``) that loses nothing.

    polars infers one ``Decimal(38, scale)`` type for the whole column, with ``scale`` the
    largest number of decimal places it sees. A value that then needs more than 38 digits
    (28 decimal places in one row, 11 integer digits in another) becomes null without an
    error. So the column is checked against the input and rebuilt as an ``Object`` column
    (plain ``decimal.Decimal`` objects) when anything changed.
    """
    try:
        s = pl.Series(name, values)
    except Exception:  # polars versions without Decimal inference
        s = None
    if s is not None and s.null_count() == sum(v is None for v in values) and s.to_list() == values:
        return s
    return pl.Series(name, values, dtype=pl.Object)


def _check_index(close: Any, cols: Dict[str, Any]) -> None:
    """pandas columns are paired by position, so their indexes must be equal."""
    if _container(close) != "pandas":
        return
    for k, v in cols.items():
        if v is not None and _container(v) == "pandas" and not close.index.equals(v.index):
            raise ValueError(
                f"{k} and close have different pandas indexes; columns are paired by position, "
                "so align them first (e.g. close, high = close.align(high, join='inner'))"
            )


def _batch(ind: Indicator, close: Any, high: Any, low: Any, open: Any, volume: Any) -> Any:
    cols = {"high": high, "low": low, "open": open, "volume": volume}
    _check_index(close, cols)
    if ind.mode == "float":
        raw = {k: (None if v is None else _to_f64_bytes(v, k)) for k, v in cols.items()}
        out = ind._inner.batch_f64(_to_f64_bytes(close, "close"), **raw)
        return _wrap(out, close, ind.label, exact=False)
    objs = {k: (None if v is None else _to_objects(v, k)) for k, v in cols.items()}
    out = ind._inner.batch(_to_objects(close, "close"), **objs)
    return _wrap(out, close, ind.label, exact=True)


# -- one-shot helpers --------------------------------------------------------------------

def compute(
    kind: str,
    close: Any,
    *params: Any,
    high: Any = None,
    low: Any = None,
    open: Any = None,
    volume: Any = None,
    mode: str = "auto",
    **kwargs: Any,
) -> Any:
    """Run a fresh ``Indicator(kind, *params)`` over a whole series.

    Returns the same container type as ``close``. ``mode="auto"`` (default) uses float mode
    for numeric numpy arrays and pandas/polars Series and exact mode for everything else
    (lists of str/Decimal/float, object-dtype columns of Decimal).
    """
    if mode == "auto":
        mode = "float" if _is_float_like(close) else "exact"
    ind = Indicator(kind, *params, mode=mode, **kwargs)
    return ind.batch(close, high=high, low=low, open=open, volume=volume)


def sma(close: Any, period: int = 20, *, mode: str = "auto") -> Any:
    """Simple moving average of ``close``."""
    return compute("sma", close, period, mode=mode)


def ema(close: Any, period: int = 20, *, mode: str = "auto") -> Any:
    """Exponential moving average of ``close`` (seeded with the SMA of the first ``period``)."""
    return compute("ema", close, period, mode=mode)


def rsi(close: Any, period: int = 14, *, mode: str = "auto") -> Any:
    """Wilder's relative strength index of ``close`` (0 to 100)."""
    return compute("rsi", close, period, mode=mode)


def macd(close: Any, fast: int = 12, slow: int = 26, signal: int = 9, *, mode: str = "auto") -> Any:
    """MACD histogram: ``(EMA(fast) - EMA(slow)) - EMA(signal) of that line``."""
    return compute("macd", close, fast, slow, signal, mode=mode)


def bollinger_b(close: Any, period: int = 20, k: Number = 2, *, mode: str = "auto") -> Any:
    """Bollinger %B: where ``close`` sits between the bands (0 = lower, 1 = upper)."""
    return compute("bollinger_b", close, period, k, mode=mode)


def atr(high: Any, low: Any, close: Any, period: int = 14, *, mode: str = "auto") -> Any:
    """Average true range (simple average of the last ``period`` true ranges)."""
    return compute("atr", close, period, high=high, low=low, mode=mode)
