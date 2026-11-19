"""indicator-diff: run one price series through every RSI, ATR and MACD you have and show
where they disagree, by how much, and why.

The same chart can be "oversold" in one library and "neutral" in another. The usual causes
are not bugs in the arithmetic but choices nobody wrote down: Wilder's smoothing against a
simple or an exponential moving average, the bar the average is seeded on, how many warm-up
bars are hidden, and what happens after a missing price. This module makes those choices
visible::

    >>> from fin_primitives import indicator_diff as idiff
    >>> report = idiff.diff(df["close"], df["high"], df["low"])
    >>> print(report.to_text())
    >>> report.disagreements()          # rows that do not match the textbook value

or from a shell::

    $ indicator-diff prices.csv                      # columns close/high/low, any case
    $ indicator-diff --demo                          # the built-in standard series
    $ indicator-diff prices.csv --load mybot/ta.py --expr "rsi=compute_rsi(close, 14)"

The reference ("textbook") values are ``fin_primitives.fast``: Wilder's RSI and ATR as he
defined them in 1978 (the average of the first ``n`` changes, then ``alpha = 1/n``), and
Appel's MACD with SMA-seeded EMAs, which is also what TA-Lib computes. Every other
implementation found on the machine is compared with it:

* ``fin_primitives`` exact-decimal path (``fin_primitives.rsi`` and friends),
* ``talib`` (TA-Lib) if installed,
* ``pandas_ta`` and ``pandas_ta_classic`` if installed. When TA-Lib is also installed both
  are run twice, with their default and with ``talib=False``, because pandas-ta switches
  to TA-Lib's code when it can import it and its own code gives different numbers,
* ``ta`` (bukosabino) if installed,
* the hand-rolled versions that get copied between projects: Cutler's SMA RSI, the
  ``ewm(span=n)`` RSI, the ``rolling(n).mean()`` ATR and the ``ewm(span, adjust=False)``
  MACD,
* anything you pass in ``candidates`` (Python) or ``--load``/``--expr`` (CLI).

For each one the report gives the largest difference, the difference on the last bar (the
one a screener acts on), the first bar with a value, how many bars change verdict (RSI zone
below 30 / above 70, MACD histogram sign), the bar from which it agrees to 0.01, and the
reason: the formula it was identified as, found by comparing it with a catalogue of known
variants, not assumed from its name.

The CLI fails closed: it exits with status 1 when any implementation disagrees with the
textbook value, so it can sit in CI, and it refuses input with missing prices unless
``--allow-nan`` is given (a gap is a decision, and the libraries make different ones).

numpy is required; pandas is required for the third-party libraries and the hand-rolled
pandas variants.
"""
from __future__ import annotations

import argparse
import importlib
import json
import math
import sys
import warnings
from dataclasses import asdict, dataclass, field
from typing import Any, Callable, Dict, Iterable, List, Mapping, Optional, Sequence, Tuple

import numpy as np

from . import fast

__all__ = [
    "Row",
    "Report",
    "diff",
    "standard_series",
    "identify",
    "VARIANTS",
    "main",
]

INDICATORS = ("rsi", "atr", "macd")

# A value matches another when they are within this many units of the reference scale
# (RSI points for RSI, the median ATR for ATR, the median absolute MACD for MACD).
MATCH_REL = 1e-6
# "Agrees from bar k": the first bar after which every difference stays below this many
# indicator units (0.01 RSI points; 0.01 % of the ATR / MACD scale).
CONVERGE_RSI = 0.01
CONVERGE_REL = 1e-4

Arr = np.ndarray
Fn = Callable[..., Any]


# --------------------------------------------------------------------------------------
# The standard series
# --------------------------------------------------------------------------------------


def standard_series(n: int = 500) -> Dict[str, Arr]:
    """A fixed, reproducible OHLC series for comparing implementations.

    Pure Python (a 64-bit linear congruential generator and Box-Muller), so it is the same
    on every machine and numpy version. Three regimes: a rally (bars 0-149, RSI goes
    overbought), a sell-off (150-299, oversold) and a range (300 on). Prices are rounded to
    cents. Returns ``{"open", "high", "low", "close"}`` as float64 arrays.
    """
    state = 1978  # the year Wilder published RSI and ATR

    def uniform() -> float:
        nonlocal state
        state = (6364136223846793005 * state + 1442695040888963407) % (1 << 64)
        return ((state >> 11) + 0.5) / float(1 << 53)

    def normal() -> float:
        return math.sqrt(-2.0 * math.log(uniform())) * math.cos(2.0 * math.pi * uniform())

    close: List[float] = []
    high: List[float] = []
    low: List[float] = []
    opn: List[float] = []
    px = 100.0
    for i in range(n):
        drift = 0.004 if i < 150 else (-0.005 if i < 300 else 0.0)
        prev = px
        px = px * math.exp(drift + 0.015 * normal())
        hi = max(prev, px) * (1.0 + abs(0.006 * normal()))
        lo = min(prev, px) * (1.0 - abs(0.006 * normal()))
        opn.append(round(prev, 2))
        close.append(round(px, 2))
        high.append(round(hi, 2))
        low.append(round(lo, 2))
    return {
        "open": np.array(opn),
        "high": np.array(high),
        "low": np.array(low),
        "close": np.array(close),
    }


# --------------------------------------------------------------------------------------
# Catalogue of known formulas (used to identify what an implementation computes)
# --------------------------------------------------------------------------------------


def _ewm(x: Arr, alpha: float, adjust: bool, min_periods: int) -> Arr:
    """pandas ``ewm(alpha=..., adjust=..., min_periods=...).mean()`` in numpy; leading NaNs
    are skipped, as pandas does."""
    out = np.full(x.shape, np.nan)
    num = 0.0
    den = 0.0
    seen = 0
    started = False
    for i, v in enumerate(x):
        if not np.isfinite(v):
            if started:
                out[i] = num / den if (seen >= min_periods and den) else np.nan
            continue
        if adjust:
            num = num * (1 - alpha) + v
            den = den * (1 - alpha) + 1.0
        else:
            if not started:
                num, den = v, 1.0
            else:
                num = num * (1 - alpha) + alpha * v
        started = True
        seen += 1
        if seen >= min_periods:
            out[i] = num / den
    return out


def _rolling_mean(x: Arr, n: int) -> Arr:
    """pandas ``rolling(n).mean()``: a window holding a NaN reads NaN."""
    out = np.full(x.shape, np.nan)
    if len(x) < n:
        return out
    out[n - 1 :] = np.lib.stride_tricks.sliding_window_view(x, n).mean(axis=1)
    return out


def _sma_seeded(x: Arr, n: int, alpha: float, start: int) -> Arr:
    """Average of ``x[start:start+n]`` at bar ``start+n-1``, then exponential smoothing."""
    out = np.full(x.shape, np.nan)
    if len(x) < start + n:
        return out
    acc = float(np.mean(x[start : start + n]))
    out[start + n - 1] = acc
    for i in range(start + n, len(x)):
        acc = acc + alpha * (x[i] - acc)
        out[i] = acc
    return out


def _changes(close: Arr, first_zero: bool) -> Tuple[Arr, Arr]:
    d = np.diff(close, prepend=np.nan)
    if first_zero:
        d[0] = 0.0
    up = np.where(np.isnan(d), np.nan, np.clip(d, 0, None))
    dn = np.where(np.isnan(d), np.nan, np.clip(-d, 0, None))
    return up, dn


def _rsi_from(up: Arr, dn: Arr) -> Arr:
    with np.errstate(divide="ignore", invalid="ignore"):
        rs = up / dn
        out = 100.0 - 100.0 / (1.0 + rs)
    out = np.where((dn == 0) & (up > 0), 100.0, out)
    out = np.where((dn == 0) & (up == 0), np.nan, out)
    return out


def _true_range(h: Arr, l: Arr, c: Arr, first: str) -> Arr:
    pc = np.roll(c, 1)
    tr: Arr = np.maximum(h - l, np.maximum(np.abs(h - pc), np.abs(l - pc)))
    tr[0] = (h[0] - l[0]) if first == "hl" else np.nan
    return tr


@dataclass(frozen=True)
class Variant:
    """A known way of computing an indicator, with what is wrong with it (if anything)."""

    key: str
    indicator: str
    family: str  # "wilder", "ema" or "sma": the smoothing, which decides the long run
    reason: str
    compute: Callable[[Dict[str, Arr], Dict[str, int]], Arr] = field(repr=False, compare=False)


def _v_rsi_textbook(s: Dict[str, Arr], p: Dict[str, int]) -> Arr:
    return np.asarray(fast.rsi(s["close"], p["length"]))


def _v_rsi_wilder_first(s: Dict[str, Arr], p: Dict[str, int]) -> Arr:
    n = p["length"]
    up, dn = _changes(s["close"], first_zero=False)
    return _rsi_from(_ewm(up, 1 / n, False, n), _ewm(dn, 1 / n, False, n))


def _v_rsi_wilder_zero(s: Dict[str, Arr], p: Dict[str, int]) -> Arr:
    n = p["length"]
    up, dn = _changes(s["close"], first_zero=True)
    return _rsi_from(_ewm(up, 1 / n, False, n), _ewm(dn, 1 / n, False, n))


def _v_rsi_wilder_adjust(s: Dict[str, Arr], p: Dict[str, int]) -> Arr:
    n = p["length"]
    up, dn = _changes(s["close"], first_zero=False)
    return _rsi_from(_ewm(up, 1 / n, True, n), _ewm(dn, 1 / n, True, n))


def _v_rsi_ema(adjust: bool) -> Callable[[Dict[str, Arr], Dict[str, int]], Arr]:
    def f(s: Dict[str, Arr], p: Dict[str, int]) -> Arr:
        n = p["length"]
        up, dn = _changes(s["close"], first_zero=False)
        a = 2 / (n + 1)
        return _rsi_from(_ewm(up, a, adjust, n), _ewm(dn, a, adjust, n))

    return f


def _v_rsi_sma(s: Dict[str, Arr], p: Dict[str, int]) -> Arr:
    n = p["length"]
    up, dn = _changes(s["close"], first_zero=False)
    return _rsi_from(_rolling_mean(up, n), _rolling_mean(dn, n))


def _v_atr_textbook(s: Dict[str, Arr], p: Dict[str, int]) -> Arr:
    return np.asarray(fast.atr(s["high"], s["low"], s["close"], p["length"]))


def _v_atr_tradingview(s: Dict[str, Arr], p: Dict[str, int]) -> Arr:
    return np.asarray(fast.atr(s["high"], s["low"], s["close"], p["length"], seed="tradingview"))


def _v_atr_ewm(alpha_of: Callable[[int], float], adjust: bool, first: str) -> Callable[[Dict[str, Arr], Dict[str, int]], Arr]:
    def f(s: Dict[str, Arr], p: Dict[str, int]) -> Arr:
        n = p["length"]
        tr = _true_range(s["high"], s["low"], s["close"], first)
        return _ewm(tr, alpha_of(n), adjust, n)

    return f


def _v_atr_sma(first: str) -> Callable[[Dict[str, Arr], Dict[str, int]], Arr]:
    def f(s: Dict[str, Arr], p: Dict[str, int]) -> Arr:
        tr = _true_range(s["high"], s["low"], s["close"], first)
        return _rolling_mean(tr, p["length"])

    return f


def _macd_from(fast_ema: Arr, slow_ema: Arr, signal: int, seeded: bool) -> Tuple[Arr, Arr, Arr]:
    line = fast_ema - slow_ema
    if seeded:
        start = int(np.argmax(np.isfinite(line))) if np.isfinite(line).any() else len(line)
        sig = _sma_seeded(line, signal, 2 / (signal + 1), start)
    else:
        sig = _ewm(line, 2 / (signal + 1), False, 1)
    return line, sig, line - sig


def _v_macd_textbook(s: Dict[str, Arr], p: Dict[str, int]) -> Arr:
    line, sig, hist = fast.macd(s["close"], p["fast"], p["slow"], p["signal"])
    return np.vstack([np.asarray(line), np.asarray(sig), np.asarray(hist)])


def _v_macd_ewm(adjust: bool) -> Callable[[Dict[str, Arr], Dict[str, int]], Arr]:
    def f(s: Dict[str, Arr], p: Dict[str, int]) -> Arr:
        c = s["close"]
        fe = _ewm(c, 2 / (p["fast"] + 1), adjust, 1)
        se = _ewm(c, 2 / (p["slow"] + 1), adjust, 1)
        line = fe - se
        sig = _ewm(line, 2 / (p["signal"] + 1), adjust, 1)
        return np.vstack([line, sig, line - sig])

    return f


def _v_macd_ewm_warmup(s: Dict[str, Arr], p: Dict[str, int]) -> Arr:
    c = s["close"]
    fe = _ewm(c, 2 / (p["fast"] + 1), False, p["fast"])
    se = _ewm(c, 2 / (p["slow"] + 1), False, p["slow"])
    line = fe - se
    sig = _ewm(line, 2 / (p["signal"] + 1), False, p["signal"])
    return np.vstack([line, sig, line - sig])


def _v_macd_talib(s: Dict[str, Arr], p: Dict[str, int]) -> Arr:
    c = s["close"]
    f, sl, sg = p["fast"], p["slow"], p["signal"]
    fe = _sma_seeded(c, f, 2 / (f + 1), max(sl - f, 0))
    se = _sma_seeded(c, sl, 2 / (sl + 1), 0)
    line, sig, hist = _macd_from(fe, se, sg, seeded=True)
    line = np.where(np.isfinite(sig), line, np.nan)
    return np.vstack([line, sig, np.where(np.isfinite(sig), hist, np.nan)])


def _v_macd_sma(s: Dict[str, Arr], p: Dict[str, int]) -> Arr:
    c = s["close"]
    line = _rolling_mean(c, p["fast"]) - _rolling_mean(c, p["slow"])
    start = int(np.argmax(np.isfinite(line)))
    sig = np.full(line.shape, np.nan)
    sig[start:] = _rolling_mean(line[start:], p["signal"])
    return np.vstack([line, sig, line - sig])


_W = "Wilder"
VARIANTS: Tuple[Variant, ...] = (
    Variant("rsi.textbook", "rsi", "wilder", "Wilder's RSI as defined (1978) and as TA-Lib computes it: the first average is the mean of the first n changes, then alpha = 1/n.", _v_rsi_textbook),
    Variant("rsi.wilder_seed_first_change", "rsi", "wilder", f"{_W}'s smoothing (alpha = 1/n) seeded with the first change instead of the mean of the first n (pandas ewm(alpha=1/n, adjust=False)). Wrong during warm-up, converges later.", _v_rsi_wilder_first),
    Variant("rsi.wilder_seed_zero", "rsi", "wilder", f"{_W}'s smoothing (alpha = 1/n) seeded with a zero change on bar 0 (the NaN first difference filled with 0). Wrong during warm-up, converges later.", _v_rsi_wilder_zero),
    Variant("rsi.wilder_adjust_true", "rsi", "wilder", f"{_W}'s alpha = 1/n with pandas' default adjust=True weights (ewm(com=n-1)). Wrong during warm-up, converges later.", _v_rsi_wilder_adjust),
    Variant("rsi.ema_span", "rsi", "ema", "Exponential smoothing with alpha = 2/(n+1) (ewm(span=n), adjust=False) instead of Wilder's 1/n. A faster indicator: RSI-14 this way is roughly Wilder's RSI-7.5. Never converges.", _v_rsi_ema(False)),
    Variant("rsi.ema_span_adjust", "rsi", "ema", "Exponential smoothing with alpha = 2/(n+1) and adjust=True (ewm(span=n)) instead of Wilder's 1/n. Never converges.", _v_rsi_ema(True)),
    Variant("rsi.sma_cutler", "rsi", "sma", "Cutler's RSI: a simple n-bar mean of gains and losses (rolling(n).mean()) instead of Wilder's smoothing. Never converges; it jumps when a big bar leaves the window.", _v_rsi_sma),
    Variant("atr.textbook", "atr", "wilder", "Wilder's ATR as TA-Lib computes it: the first value is the mean true range of bars 1..n, then alpha = 1/n.", _v_atr_textbook),
    Variant("atr.tradingview_seed", "atr", "wilder", "Wilder's smoothing with bar 0's true range taken as high - low and the seed at bar n-1 (TradingView's ta.atr). Wrong during warm-up, converges later.", _v_atr_tradingview),
    Variant("atr.wilder_seed_first", "atr", "wilder", "Wilder's alpha = 1/n seeded with the first true range (ewm(alpha=1/n, adjust=False)). Wrong during warm-up, converges later.", _v_atr_ewm(lambda n: 1 / n, False, "nan")),
    Variant("atr.wilder_seed_first_hl", "atr", "wilder", "Wilder's alpha = 1/n seeded with bar 0's high - low (ewm(alpha=1/n, adjust=False)). Wrong during warm-up, converges later.", _v_atr_ewm(lambda n: 1 / n, False, "hl")),
    Variant("atr.wilder_adjust_true", "atr", "wilder", "Wilder's alpha = 1/n with pandas' default adjust=True weights (ewm(com=n-1)). Wrong during warm-up, converges later.", _v_atr_ewm(lambda n: 1 / n, True, "nan")),
    Variant("atr.ema_span", "atr", "ema", "Exponential smoothing with alpha = 2/(n+1) (ewm(span=n)) instead of Wilder's 1/n. Never converges.", _v_atr_ewm(lambda n: 2 / (n + 1), False, "nan")),
    Variant("atr.ema_span_hl", "atr", "ema", "Exponential smoothing with alpha = 2/(n+1) (ewm(span=n)), bar 0's true range as high - low, instead of Wilder's 1/n. Never converges.", _v_atr_ewm(lambda n: 2 / (n + 1), False, "hl")),
    Variant("atr.sma", "atr", "sma", "A simple n-bar mean of the true range (rolling(n).mean()) instead of Wilder's smoothing. Never converges.", _v_atr_sma("nan")),
    Variant("atr.sma_hl", "atr", "sma", "A simple n-bar mean of the true range with bar 0 as high - low, instead of Wilder's smoothing. Never converges.", _v_atr_sma("hl")),
    Variant("macd.textbook", "macd", "ema", "Appel's MACD with every EMA seeded by the mean of its first n values (pandas-ta-classic).", _v_macd_textbook),
    Variant("macd.talib", "macd", "ema", "TA-Lib's seeding: the fast EMA is seeded on the slow EMA's first bar (mean of closes slow-fast .. slow-1, not of the first fast closes) and the line is hidden until the signal has a value. Same EMAs; the signal differs during warm-up, then converges.", _v_macd_talib),
    Variant("macd.ewm_first_value", "macd", "ema", "EMAs seeded with the first close (ewm(span, adjust=False)) and no warm-up: the first ~3x slow bars are wrong, then it converges.", _v_macd_ewm(False)),
    Variant("macd.ewm_first_value_warmup", "macd", "ema", "EMAs seeded with the first close (ewm(span, adjust=False)) with the warm-up hidden by min_periods: the hidden bars do not fix the seed, so the first visible values are still wrong, then it converges.", _v_macd_ewm_warmup),
    Variant("macd.ewm_adjust_true", "macd", "ema", "EMAs with pandas' default adjust=True weights (ewm(span)): wrong during warm-up, converges later.", _v_macd_ewm(True)),
    Variant("macd.sma", "macd", "sma", "Simple moving averages instead of EMAs. Never converges.", _v_macd_sma),
)


# --------------------------------------------------------------------------------------
# Implementations found on this machine
# --------------------------------------------------------------------------------------


def _import(name: str) -> Any:
    """Import an optional library without side effects on the caller's pandas.

    pandas-ta and pandas-ta-classic both register a ``df.ta`` accessor on import, and the
    last one imported wins. Running a comparison must not change which ``df.ta`` the
    caller's own code gets, so any accessor that existed before is put back.
    """
    pd = sys.modules.get("pandas")
    saved: List[Tuple[Any, str, Any]] = []
    if pd is not None:
        for cls in (pd.DataFrame, pd.Series):
            for attr in ("ta",):
                if attr in cls.__dict__:
                    saved.append((cls, attr, cls.__dict__[attr]))
    try:
        with warnings.catch_warnings():
            warnings.simplefilter("ignore")
            return importlib.import_module(name)
    except Exception:  # noqa: BLE001 - a broken optional install is "not installed"
        return None
    finally:
        for cls, attr, value in saved:
            setattr(cls, attr, value)


def _series(s: Dict[str, Arr]) -> Dict[str, Any]:
    import pandas as pd

    return {k: pd.Series(v, name=k) for k, v in s.items()}


def _np(x: Any) -> Arr:
    if hasattr(x, "to_numpy"):
        x = x.to_numpy(dtype=float, na_value=np.nan)
    return np.asarray(x, dtype=float).reshape(-1)


def _macd3(df: Any) -> Arr:
    """A pandas-ta MACD frame (line, histogram, signal) as rows (line, signal, histogram)."""
    cols = list(df.columns)
    line = next(c for c in cols if c.startswith("MACD_"))
    hist = next(c for c in cols if c.startswith("MACDh"))
    sig = next(c for c in cols if c.startswith("MACDs"))
    return np.vstack([_np(df[line]), _np(df[sig]), _np(df[hist])])


Impl = Tuple[str, str, Callable[[Dict[str, Arr], Dict[str, int]], Arr]]


def _library_impls(indicators: Sequence[str]) -> List[Impl]:
    """``(name, indicator, fn)`` for every implementation that can be imported."""
    out: List[Impl] = []
    pd = _import("pandas")

    import fin_primitives as fp

    def fp_rsi(s: Dict[str, Arr], p: Dict[str, int]) -> Arr:
        return _np(fp.rsi(s["close"], p["length"], mode="float"))

    def fp_atr(s: Dict[str, Arr], p: Dict[str, int]) -> Arr:
        return _np(fp.atr(s["high"], s["low"], s["close"], p["length"], mode="float"))

    out += [("fin_primitives (exact decimal)", "rsi", fp_rsi), ("fin_primitives (exact decimal)", "atr", fp_atr)]

    talib = _import("talib")
    if talib is not None:
        out += [
            ("talib", "rsi", lambda s, p: _np(talib.RSI(s["close"], timeperiod=p["length"]))),
            ("talib", "atr", lambda s, p: _np(talib.ATR(s["high"], s["low"], s["close"], timeperiod=p["length"]))),
            ("talib", "macd", lambda s, p: np.vstack([_np(x) for x in talib.MACD(s["close"], p["fast"], p["slow"], p["signal"])])),
        ]

    if pd is not None:
        for modname in ("pandas_ta", "pandas_ta_classic"):
            mod = _import(modname)
            if mod is None:
                continue
            variants: List[Tuple[str, Dict[str, Any]]] = [(modname, {})]
            if talib is not None:
                variants.append((f"{modname} (talib=False)", {"talib": False}))
            for label, kw in variants:
                out += _pandas_ta_impls(label, mod, kw)

        ta = _import("ta")
        if ta is not None and hasattr(ta, "momentum"):
            out += [
                ("ta", "rsi", lambda s, p: _np(ta.momentum.RSIIndicator(_series(s)["close"], window=p["length"]).rsi())),
                ("ta", "atr", lambda s, p: _np(ta.volatility.AverageTrueRange(*(_series(s)[k] for k in ("high", "low", "close")), window=p["length"]).average_true_range())),
                ("ta", "macd", lambda s, p: _ta_macd(ta, s, p)),
            ]

        out += _hand_rolled()
    return [i for i in out if i[1] in indicators]


def _pandas_ta_impls(label: str, m: Any, kw: Dict[str, Any]) -> List[Impl]:
    """RSI, ATR and MACD of pandas-ta or pandas-ta-classic, with ``kw`` (``talib=False``)."""

    def rsi(s: Dict[str, Arr], p: Dict[str, int]) -> Arr:
        return _np(m.rsi(_series(s)["close"], length=p["length"], **kw))

    def atr(s: Dict[str, Arr], p: Dict[str, int]) -> Arr:
        x = _series(s)
        return _np(m.atr(x["high"], x["low"], x["close"], length=p["length"], **kw))

    def macd(s: Dict[str, Arr], p: Dict[str, int]) -> Arr:
        return _macd3(m.macd(_series(s)["close"], fast=p["fast"], slow=p["slow"], signal=p["signal"], **kw))

    return [(label, "rsi", rsi), (label, "atr", atr), (label, "macd", macd)]


def _ta_macd(ta: Any, s: Dict[str, Arr], p: Dict[str, int]) -> Arr:
    m = ta.trend.MACD(_series(s)["close"], window_slow=p["slow"], window_fast=p["fast"], window_sign=p["signal"])
    return np.vstack([_np(m.macd()), _np(m.macd_signal()), _np(m.macd_diff())])


def _hand_rolled() -> List[Impl]:
    """The versions that get pasted between projects, written the way they usually are."""

    def sma_rsi(s: Dict[str, Arr], p: Dict[str, int]) -> Arr:
        c = _series(s)["close"]
        d = c.diff()
        gain = d.clip(lower=0).rolling(p["length"]).mean()
        loss = (-d.clip(upper=0)).rolling(p["length"]).mean()
        return _np(100 - 100 / (1 + gain / loss))

    def ewm_span_rsi(s: Dict[str, Arr], p: Dict[str, int]) -> Arr:
        c = _series(s)["close"]
        d = c.diff()
        gain = d.clip(lower=0).ewm(span=p["length"], adjust=False).mean()
        loss = (-d.clip(upper=0)).ewm(span=p["length"], adjust=False).mean()
        return _np(100 - 100 / (1 + gain / loss))

    def sma_atr(s: Dict[str, Arr], p: Dict[str, int]) -> Arr:
        import pandas as pd

        x = _series(s)
        pc = x["close"].shift()
        tr = pd.concat([x["high"] - x["low"], (x["high"] - pc).abs(), (x["low"] - pc).abs()], axis=1).max(axis=1)
        return _np(tr.rolling(p["length"]).mean())

    def ewm_macd(s: Dict[str, Arr], p: Dict[str, int]) -> Arr:
        c = _series(s)["close"]
        line = c.ewm(span=p["fast"], adjust=False).mean() - c.ewm(span=p["slow"], adjust=False).mean()
        sig = line.ewm(span=p["signal"], adjust=False).mean()
        return np.vstack([_np(line), _np(sig), _np(line - sig)])

    return [
        ("hand-rolled: rolling-mean RSI", "rsi", sma_rsi),
        ("hand-rolled: ewm(span=n) RSI", "rsi", ewm_span_rsi),
        ("hand-rolled: rolling-mean ATR", "atr", sma_atr),
        ("hand-rolled: ewm(span, adjust=False) MACD", "macd", ewm_macd),
    ]


# --------------------------------------------------------------------------------------
# Comparison
# --------------------------------------------------------------------------------------


@dataclass
class Row:
    """One implementation of one indicator output, compared with the textbook value."""

    implementation: str
    indicator: str  # "rsi", "atr", "macd", "macd_signal" or "macd_hist"
    matches: bool
    max_abs_diff: Optional[float]
    last_diff: Optional[float]
    last_value: Optional[float]
    textbook_last: Optional[float]
    first_value_bar: Optional[int]
    textbook_first_bar: Optional[int]
    agrees_from_bar: Optional[int]
    verdict_flips: int
    verdict_rule: str
    warmup_fill: Optional[float]
    identified_as: Optional[str]
    reason: str
    nan_after_gap: Optional[str] = None
    error: Optional[str] = None

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)


@dataclass
class Report:
    """The result of :func:`diff`. ``rows`` holds one :class:`Row` per implementation and
    output; :meth:`to_text`, :meth:`to_json` and :meth:`disagreements` summarise it."""

    n_bars: int
    params: Dict[str, int]
    rows: List[Row]
    notes: List[str]

    def disagreements(self) -> List[Row]:
        """Rows whose values do not match the textbook value (errors included)."""
        return [r for r in self.rows if not r.matches]

    def to_dict(self) -> Dict[str, Any]:
        return {"n_bars": self.n_bars, "params": self.params, "notes": self.notes, "rows": [r.to_dict() for r in self.rows]}

    def to_json(self, **kw: Any) -> str:
        return json.dumps(self.to_dict(), **kw)

    def to_text(self) -> str:
        return _render(self)


def _scale(indicator: str, ref: Arr) -> float:
    if indicator == "rsi":
        return 1.0
    f = ref[np.isfinite(ref)]
    return float(np.median(np.abs(f))) if f.size and np.median(np.abs(f)) > 0 else 1.0


def _first(x: Arr) -> Optional[int]:
    m = np.isfinite(x)
    return int(np.argmax(m)) if m.any() else None


def _verdicts(indicator: str, a: Arr, b: Arr) -> Tuple[int, str]:
    both = np.isfinite(a) & np.isfinite(b)
    if indicator == "rsi":

        def zone(x: Arr) -> Arr:
            return np.where(x < 30, -1, np.where(x > 70, 1, 0))

        return int(np.sum(zone(a[both]) != zone(b[both]))), "RSI zone (below 30 / 30-70 / above 70)"
    if indicator == "macd_hist":
        return int(np.sum(np.sign(a[both]) != np.sign(b[both]))), "histogram sign (MACD above / below signal)"
    if indicator == "macd":
        return int(np.sum(np.sign(a[both]) != np.sign(b[both]))), "MACD line sign"
    return 0, "none (ATR has no verdict; see the size of the difference)"


def _converged_from(indicator: str, a: Arr, b: Arr, scale: float) -> Optional[int]:
    tol = CONVERGE_RSI if indicator == "rsi" else CONVERGE_REL * scale
    both = np.isfinite(a) & np.isfinite(b)
    if not both.any():
        return None
    bad = both & (np.abs(a - b) > tol)
    if not bad.any():
        return int(np.argmax(both))
    last_bad = int(np.flatnonzero(bad)[-1])
    return last_bad + 1 if last_bad + 1 < len(a) else None


def _component(x: Arr, k: int) -> Arr:
    return x[k] if x.ndim == 2 else x


def _match(a: Arr, b: Arr, scale: float) -> bool:
    both = np.isfinite(a) & np.isfinite(b)
    if not both.any():
        return False
    if np.any(np.isfinite(a) != np.isfinite(b)):
        return False
    return bool(np.max(np.abs(a[both] - b[both])) <= MATCH_REL * max(scale, 1.0))


def identify(indicator: str, values: Any, series: Mapping[str, Any], params: Optional[Mapping[str, int]] = None) -> Tuple[Optional[Variant], str]:
    """Name the formula behind ``values`` (an RSI, ATR or MACD output over ``series``; MACD
    may be the line alone or line, signal and histogram as three rows).

    Compares the output with every :data:`VARIANTS` entry on the bars where both have a
    value. Returns ``(variant, how)``:

    * ``"exact"``: it matches the variant on every shared bar (to 1e-6 of the scale);
    * ``"rounded"``: it matches to 1e-3 of the scale, as when the output is rounded to a few
      decimals;
    * ``"family"``: only the last 40 % of each output matches (the smoothing is identified,
      the seed or warm-up is not one in the catalogue);
    * ``(None, "unknown")`` otherwise.
    """
    s = {k: np.asarray(_np(v), dtype=float) for k, v in series.items()}
    p = _params(params or {})
    v = _np_any(values)
    if v.ndim == 1:
        v = v[None, :]
    rows = 3 if (v.shape[0] == 3 and indicator == "macd") else 1
    v = v[:rows]
    cands = [x for x in VARIANTS if x.indicator == indicator]
    ref_scale = max(_scale(indicator, _component(cands[0].compute(s, p), 0)), 1.0) if indicator == "rsi" else _scale(indicator, _component(cands[0].compute(s, p), 0))

    def rows_of(var: Variant) -> Optional[Arr]:
        w = var.compute(s, p)
        w = w[None, :] if w.ndim == 1 else w
        w = w[:rows]
        return w if w.shape == v.shape else None

    def err(w: Arr, tail: bool) -> Optional[float]:
        worst = 0.0
        for k in range(rows):
            both = np.flatnonzero(np.isfinite(w[k]) & np.isfinite(v[k]))
            if both.size < (10 if tail else 3):
                return None
            idx = both[int(both.size * 0.6) :] if tail else both
            worst = max(worst, float(np.max(np.abs(w[k][idx] - v[k][idx]))))
        return worst

    scored: List[Tuple[float, Variant]] = []
    for var in cands:
        w = rows_of(var)
        e = None if w is None else err(w, tail=False)
        if e is not None:
            scored.append((e, var))
    if scored:
        e, var = min(scored, key=lambda t: t[0])
        if e <= MATCH_REL * ref_scale:
            return var, "exact"
        if e <= 1e-3 * ref_scale:
            return var, "rounded"
    fams: Dict[str, Tuple[float, Variant]] = {}
    for var in cands:
        w = rows_of(var)
        e = None if w is None else err(w, tail=True)
        if e is not None and (var.family not in fams or e < fams[var.family][0]):
            fams[var.family] = (e, var)
    if fams:
        e, var = min(fams.values(), key=lambda t: t[0])
        if e <= (0.05 if indicator == "rsi" else 1e-3 * ref_scale):
            return var, "family"
    return None, "unknown"


def _params(p: Mapping[str, int]) -> Dict[str, int]:
    out = {"length": 14, "fast": 12, "slow": 26, "signal": 9}
    out.update({k: int(v) for k, v in p.items()})
    return out


def _gap_probe(indicator: str, fn: Callable[[Dict[str, Arr], Dict[str, int]], Arr], s: Dict[str, Arr], p: Dict[str, int]) -> Optional[str]:
    """What the implementation does after one missing close at 60 % of the series."""
    k = int(len(s["close"]) * 0.6)
    g = {key: v.copy() for key, v in s.items()}
    g["close"][k] = np.nan
    try:
        with warnings.catch_warnings(), np.errstate(all="ignore"):
            warnings.simplefilter("ignore")
            out = _component(np.atleast_1d(_np_any(fn(g, p))), 0)
    except Exception as e:  # noqa: BLE001
        return f"refuses a missing price ({type(e).__name__})"
    after = out[k + 1 :]
    if after.size and not np.isfinite(after).any():
        return "one missing price turns every later value into NaN"
    nan_bars = int(np.sum(~np.isfinite(out[k:])))
    if nan_bars == 0:
        return "returns a value even on the missing bar (the gap is absorbed silently)"
    return f"{nan_bars} bar(s) of NaN from the missing price, then values resume"


def _np_any(x: Any) -> Arr:
    if isinstance(x, np.ndarray):
        return x.astype(float)
    if hasattr(x, "columns"):
        return np.vstack([_np(x[c]) for c in x.columns])
    if isinstance(x, (tuple, list)) and x and hasattr(x[0], "__len__"):
        return np.vstack([_np(c) for c in x])
    return _np(x)


def _compare(name: str, indicator: str, out: Arr, ref: Arr, s: Dict[str, Arr], p: Dict[str, int], gap: Optional[str]) -> List[Row]:
    rows: List[Row] = []
    comps = [("macd", 0), ("macd_signal", 1), ("macd_hist", 2)] if indicator == "macd" else [(indicator, 0)]
    variant, how = identify(indicator, out, s, p)
    for label, k in comps:
        if indicator == "macd" and out.ndim == 1 and k > 0:
            continue
        a = _component(out, k)
        b = _component(ref, k)
        if a.shape != b.shape:
            rows.append(_err_row(name, label, f"returned {a.shape[0]} values for {b.shape[0]} bars"))
            continue
        scale = _scale(indicator, b)
        both = np.isfinite(a) & np.isfinite(b)
        maxd = float(np.max(np.abs(a[both] - b[both]))) if both.any() else None
        last = (float(a[-1] - b[-1]) if np.isfinite(a[-1]) and np.isfinite(b[-1]) else None)
        ok = _match(a, b, scale)
        flips, rule = _verdicts(label, a, b)
        fill = None
        early = np.isfinite(a) & ~np.isfinite(b)
        if early.sum() >= 2:
            # A constant (0, 50, ...) standing in for "not enough data yet"; the last early
            # bar may already be the seed.
            vals, counts = np.unique(a[early], return_counts=True)
            top = int(np.argmax(counts))
            if counts[top] >= 2 and counts[top] >= early.sum() - 1:
                fill = float(vals[top])
        rows.append(
            Row(
                implementation=name,
                indicator=label,
                matches=ok,
                max_abs_diff=maxd,
                last_diff=last,
                last_value=float(a[-1]) if np.isfinite(a[-1]) else None,
                textbook_last=float(b[-1]) if np.isfinite(b[-1]) else None,
                first_value_bar=_first(a),
                textbook_first_bar=_first(b),
                agrees_from_bar=_converged_from(label, a, b, scale),
                verdict_flips=flips,
                verdict_rule=rule,
                warmup_fill=fill,
                identified_as=(f"{variant.key} ({how})" if variant else None),
                reason=_reason(ok, variant, how, a, b, fill),
                nan_after_gap=gap,
            )
        )
    return rows


def _textbook_family(indicator: str) -> str:
    return next(v.family for v in VARIANTS if v.key == f"{indicator}.textbook")


def _reason(ok: bool, variant: Optional[Variant], how: str, a: Arr, b: Arr, fill: Optional[float]) -> str:
    if ok:
        return "matches the textbook value"
    parts: List[str] = []
    if variant is None:
        parts.append("does not match Wilder, EMA or SMA smoothing in the catalogue")
    elif how == "family":
        fam = {"wilder": "Wilder's smoothing (alpha = 1/n)", "ema": "EMA smoothing", "sma": "simple-average smoothing"}[variant.family]
        conv = "converges later" if variant.family == _textbook_family(variant.indicator) else "never converges"
        parts.append(f"{fam} with a seed or warm-up not in the catalogue; {conv}")
    elif variant.key.endswith("textbook"):
        parts.append("same formula as the textbook value" + (", output rounded" if how == "rounded" else ""))
    else:
        parts.append(variant.reason + (" Output rounded." if how == "rounded" else ""))
    fa, fb = _first(a), _first(b)
    if fa is not None and fb is not None and fa != fb:
        if fill is not None:
            n_fill = int(np.sum(np.isfinite(a[:fb]) & (a[:fb] == fill)))
            parts.append(f"fills {n_fill} warm-up bar(s) before bar {fb} with {fill:g} instead of leaving them empty")
        elif fa < fb:
            parts.append(f"shows values from bar {fa}, {fb - fa} bar(s) before there is enough data (textbook starts at bar {fb})")
        else:
            parts.append(f"hides {fa - fb} more bar(s) of warm-up (first value at bar {fa}, textbook at bar {fb})")
    missing = np.isfinite(b) & ~np.isfinite(a)
    if fa is not None and missing[fa:].any():
        parts.append(f"{int(missing[fa:].sum())} bar(s) empty where the textbook has a value")
    return "; ".join(parts)


def _err_row(name: str, indicator: str, msg: str) -> Row:
    return Row(name, indicator, False, None, None, None, None, None, None, None, 0, "", None, None, f"error: {msg}", error=msg)


Candidate = Tuple[str, Callable[..., Any]]


def diff(
    close: Any,
    high: Any = None,
    low: Any = None,
    *,
    indicators: Iterable[str] = INDICATORS,
    length: int = 14,
    fast: int = 12,
    slow: int = 26,
    signal: int = 9,
    libraries: bool = True,
    candidates: Optional[Mapping[str, Candidate]] = None,
    allow_nan: bool = False,
    gap_probe: bool = True,
) -> Report:
    """Run ``close`` (and ``high``/``low`` for ATR) through every RSI, ATR and MACD found and
    compare each with the textbook value.

    ``candidates`` adds your own: ``{"name": ("rsi", fn)}`` where ``fn`` takes a pandas
    DataFrame with ``open``/``high``/``low``/``close`` columns (also ``Open``/``High``/...
    and ``CLOSE``...) and returns one value per bar (MACD: a DataFrame or tuple of line,
    signal, histogram, or just the line).

    Missing prices are refused (``ValueError``) unless ``allow_nan=True``: libraries treat a
    gap differently, so a series with gaps is not a fair comparison. ``gap_probe`` reports
    what each implementation does after one missing price, on a copy of the series.
    """
    s: Dict[str, Arr] = {"close": _np(close)}
    inds = tuple(indicators)
    bad = [i for i in inds if i not in INDICATORS]
    if bad:
        raise ValueError(f"unknown indicator(s) {bad}; choose from {list(INDICATORS)}")
    if high is not None:
        s["high"] = _np(high)
    if low is not None:
        s["low"] = _np(low)
    if "atr" in inds and ("high" not in s or "low" not in s):
        raise ValueError("ATR needs high and low; pass them or leave 'atr' out of indicators")
    n = len(s["close"])
    for k, v in s.items():
        if len(v) != n:
            raise ValueError(f"{k} has {len(v)} values, close has {n}")
        if not allow_nan and not np.isfinite(v).all():
            i = int(np.argmax(~np.isfinite(v)))
            raise ValueError(f"{k}[{i}] is missing; libraries disagree on gaps, so drop or fill them first (or pass allow_nan=True)")
    s.setdefault("open", s["close"].copy())
    p = _params({"length": length, "fast": fast, "slow": slow, "signal": signal})

    notes: List[str] = []
    need = {"rsi": 10 * p["length"], "atr": 10 * p["length"], "macd": 10 * p["slow"]}
    short = [i for i in inds if n < need[i]]
    if short:
        notes.append(
            f"{n} bars is short for {', '.join(short)}: seed effects can last about 10x the period, so"
            " the reason column may say a seed variant converges when the series never gets there."
        )

    refs: Dict[str, Arr] = {}
    for ind in inds:
        var = next(v for v in VARIANTS if v.key == f"{ind}.textbook")
        refs[ind] = var.compute(s, p)

    impls: List[Impl] = _library_impls(inds) if libraries else []
    for cname, (cind, fn) in (candidates or {}).items():
        if cind not in INDICATORS:
            raise ValueError(f"candidate {cname!r}: indicator must be one of {list(INDICATORS)}")
        if cind in inds:
            impls.append((cname, cind, _wrap_candidate(fn)))

    rows: List[Row] = []
    for name, ind, fn in impls:
        try:
            with warnings.catch_warnings(), np.errstate(all="ignore"):
                warnings.simplefilter("ignore")
                out = _np_any(fn(s, p))
        except Exception as e:  # noqa: BLE001 - a broken implementation is a finding
            rows.append(_err_row(name, ind, f"{type(e).__name__}: {e}"))
            continue
        if ind != "macd" and out.ndim == 2:
            out = out[0]
        gap = _gap_probe(ind, fn, s, p) if gap_probe else None
        rows += _compare(name, ind, out, refs[ind], s, p, gap)

    for modname in ("pandas_ta", "pandas_ta_classic"):
        a = {(r.indicator): r for r in rows if r.implementation == modname}
        b = {(r.indicator): r for r in rows if r.implementation == f"{modname} (talib=False)"}
        changed = [k for k in a if k in b and a[k].matches != b[k].matches]
        if changed:
            notes.append(
                f"{modname} gives different {', '.join(changed)} depending on whether TA-Lib is installed:"
                " the same code returns other numbers on another machine."
            )
    return Report(n_bars=n, params=p, rows=rows, notes=notes)


def _frame(s: Dict[str, Arr]) -> Any:
    import pandas as pd

    df = pd.DataFrame({k: s[k] for k in ("open", "high", "low", "close") if k in s})
    for k in list(df.columns):
        df[k.capitalize()] = df[k]
        df[k.upper()] = df[k]
    return df


def _wrap_candidate(fn: Callable[..., Any]) -> Callable[[Dict[str, Arr], Dict[str, int]], Any]:
    def run(s: Dict[str, Arr], p: Dict[str, int]) -> Any:
        return fn(_frame(s))

    return run


# --------------------------------------------------------------------------------------
# Text report
# --------------------------------------------------------------------------------------


def _fmt(x: Optional[float], digits: int = 4) -> str:
    if x is None:
        return "-"
    if x == 0:
        return "0"
    if abs(x) < 10 ** (-digits):
        return f"{x:.1e}"
    return f"{x:.{digits}f}"


def _render(r: Report) -> str:
    p = r.params
    present = {x.indicator.split("_")[0] for x in r.rows}
    names = {"rsi": f"RSI({p['length']})", "atr": f"ATR({p['length']})", "macd": f"MACD({p['fast']},{p['slow']},{p['signal']})"}
    lines = [
        f"indicator-diff: {r.n_bars} bars, " + ", ".join(names[k] for k in INDICATORS if k in present),
        "textbook = Wilder (1978) RSI and ATR as TA-Lib computes them; MACD with SMA-seeded EMAs (fin_primitives.fast)",
        "",
    ]
    order = ["rsi", "atr", "macd", "macd_signal", "macd_hist"]
    for ind in order:
        rows = [x for x in r.rows if x.indicator == ind]
        if not rows:
            continue
        ref_last = next((x.textbook_last for x in rows if x.textbook_last is not None), None)
        lines.append(f"== {ind.upper()}  (textbook last value {_fmt(ref_last)})")
        w = max(len(x.implementation) for x in rows)
        lines.append(f"  {'implementation':<{w}}  {'match':5}  {'max diff':>10}  {'last diff':>10}  {'1st bar':>7}  {'flips':>5}  {'agrees from':>11}")
        for x in rows:
            if x.error:
                lines.append(f"  {x.implementation:<{w}}  ERROR  {x.error}")
                continue
            lines.append(
                f"  {x.implementation:<{w}}  {'yes' if x.matches else 'NO':5}  {_fmt(x.max_abs_diff):>10}  {_fmt(x.last_diff):>10}"
                f"  {('-' if x.first_value_bar is None else str(x.first_value_bar)):>7}  {x.verdict_flips:>5}"
                f"  {('never' if x.agrees_from_bar is None else 'bar ' + str(x.agrees_from_bar)):>11}"
            )
        for x in rows:
            if not x.matches and not x.error:
                lines.append(f"    - {x.implementation}: {x.reason}")
        gaps = sorted({(x.implementation, x.nan_after_gap) for x in rows if x.nan_after_gap})
        if ind in ("rsi", "atr", "macd") and gaps:
            lines.append("    after one missing price:")
            for impl, g in gaps:
                lines.append(f"      {impl}: {g}")
        lines.append("")
    flips = [x for x in r.rows if x.verdict_flips]
    if flips:
        worst = max(flips, key=lambda x: x.verdict_flips)
        lines.append(f"Most verdict changes: {worst.implementation} {worst.indicator.upper()}, {worst.verdict_flips} bar(s) ({worst.verdict_rule}).")
    lines += [f"Note: {n}" for n in r.notes]
    bad = r.disagreements()
    lines.append(
        f"{len(bad)} of {len(r.rows)} result(s) disagree with the textbook value." if bad else "Every implementation matches the textbook value."
    )
    return "\n".join(lines)


# --------------------------------------------------------------------------------------
# CLI
# --------------------------------------------------------------------------------------


def _read_csv(path: str, cols: Dict[str, Optional[str]]) -> Dict[str, Arr]:
    import csv

    with open(path, newline="", encoding="utf-8-sig") as f:
        rows = list(csv.DictReader(f))
    if not rows:
        raise ValueError(f"{path}: no rows")
    header = list(rows[0].keys())
    lower = {h.strip().lower(): h for h in header if h is not None}
    out: Dict[str, Arr] = {}
    for key, given in cols.items():
        name = given if given is not None else lower.get(key) or lower.get({"close": "adj close"}.get(key, ""), None)
        if name is None:
            if key == "close":
                raise ValueError(f"{path}: no close column (have {header}); name it with --close")
            continue
        if name not in header:
            raise ValueError(f"{path}: no column {name!r} (have {header})")
        vals = []
        for r in rows:
            v = (r.get(name) or "").strip()
            vals.append(float(v) if v not in ("", "nan", "NaN", "null", "None") else math.nan)
        out[key] = np.array(vals, dtype=float)
    return out


def _load_module(path: str) -> Dict[str, Any]:
    import importlib.util
    import os

    name = "_idiff_" + os.path.splitext(os.path.basename(path))[0]
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise ValueError(f"cannot load {path}")
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    spec.loader.exec_module(mod)
    return dict(vars(mod))


def _expr_candidate(text: str, namespace: Dict[str, Any]) -> Tuple[str, Candidate]:
    if "=" not in text:
        raise ValueError(f"--expr {text!r}: write INDICATOR=EXPRESSION, for example rsi=compute_rsi(close, 14)")
    ind, expr = text.split("=", 1)
    ind = ind.strip().lower()
    if ind not in INDICATORS:
        raise ValueError(f"--expr {text!r}: indicator must be one of {list(INDICATORS)}")
    code = compile(expr, "<--expr>", "eval")

    def fn(df: Any) -> Any:
        ns = dict(namespace)
        ns.update({"df": df, "np": np, "close": df["close"], "high": df["high"], "low": df["low"], "open": df["open"]})
        try:
            import pandas as pd

            ns.setdefault("pd", pd)
        except ImportError:
            pass
        return eval(code, ns)  # noqa: S307 - the user's own expression, run on their machine

    return expr.strip(), (ind, fn)


def main(argv: Optional[Sequence[str]] = None) -> int:
    """Entry point of the ``indicator-diff`` command. Returns the exit status: 0 when every
    implementation matches the textbook value, 1 when one disagrees, 2 on bad input."""
    ap = argparse.ArgumentParser(
        prog="indicator-diff",
        description="Run one price series through every RSI, ATR and MACD installed (TA-Lib, pandas-ta, ta, "
        "common hand-rolled versions, your own) and report where they disagree with the textbook value, by how much, and why.",
    )
    ap.add_argument("csv", nargs="?", help="CSV with close (and high/low for ATR) columns; header names are matched case-insensitively")
    ap.add_argument("--demo", action="store_true", help="use the built-in 500-bar standard series instead of a CSV")
    ap.add_argument("--close", help="close column name")
    ap.add_argument("--high", help="high column name")
    ap.add_argument("--low", help="low column name")
    ap.add_argument("--indicators", default="rsi,atr,macd", help="comma-separated subset of rsi,atr,macd")
    ap.add_argument("--length", type=int, default=14, help="RSI and ATR period (default 14)")
    ap.add_argument("--fast", type=int, default=12)
    ap.add_argument("--slow", type=int, default=26)
    ap.add_argument("--signal", type=int, default=9)
    ap.add_argument("--load", action="append", default=[], help="Python file whose functions --expr can call (repeatable)")
    ap.add_argument("--expr", action="append", default=[], help="your implementation, INDICATOR=EXPRESSION over close/high/low/open/df, e.g. rsi=my_rsi(close, 14) (repeatable)")
    ap.add_argument("--only-expr", action="store_true", help="compare only the --expr implementations, not the installed libraries")
    ap.add_argument("--allow-nan", action="store_true", help="accept missing prices instead of refusing the input")
    ap.add_argument("--json", action="store_true", help="print the report as JSON")
    ap.add_argument("--no-fail", action="store_true", help="exit 0 even when something disagrees")
    a = ap.parse_args(argv)

    if a.demo == bool(a.csv):
        ap.print_usage(sys.stderr)
        print("indicator-diff: give a CSV file or --demo (not both)", file=sys.stderr)
        return 2
    inds = [x.strip().lower() for x in a.indicators.split(",") if x.strip()]
    try:
        if a.demo:
            s = standard_series()
        else:
            s = _read_csv(a.csv, {"close": a.close, "high": a.high, "low": a.low})
            if "atr" in inds and ("high" not in s or "low" not in s):
                print("indicator-diff: no high/low columns; leaving ATR out", file=sys.stderr)
                inds = [i for i in inds if i != "atr"]
        ns: Dict[str, Any] = {}
        for path in a.load:
            ns.update(_load_module(path))
        cands = dict(_expr_candidate(e, ns) for e in a.expr)
        rep = diff(
            s["close"], s.get("high"), s.get("low"),
            indicators=inds, length=a.length, fast=a.fast, slow=a.slow, signal=a.signal,
            libraries=not a.only_expr, candidates=cands, allow_nan=a.allow_nan,
        )
    except (ValueError, OSError) as e:
        print(f"indicator-diff: {e}", file=sys.stderr)
        return 2
    print(rep.to_json(indent=2) if a.json else rep.to_text())
    return 0 if (a.no_fail or not rep.disagreements()) else 1


if __name__ == "__main__":  # pragma: no cover
    sys.exit(main())
