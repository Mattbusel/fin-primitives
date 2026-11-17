"""pandas-ta-classic 0.9.0's indicators over float64 numpy arrays.

The shared engine behind ``df.fp`` (:mod:`fin_primitives.pandas`) and the Polars
namespaces (:mod:`fin_primitives.polars`). Every function takes numpy arrays and pandas-ta's
keyword arguments (same names, same defaults) and returns a list of ``(column name,
values)`` pairs in pandas-ta's column order, with pandas-ta's column names.

The moving averages, rolling sums, rolling extremes and recursions (PSAR, Supertrend,
QQE, Wilder smoothing) run in Rust (:mod:`fin_primitives.fast` and the ``fast_*``
kernels); the rest is elementwise numpy. Each function follows the pandas-ta-classic code
it replaces operation for operation, so the values agree with pandas-ta to within a few
units in the last place.
"""
from __future__ import annotations

import math
from numbers import Real
from typing import Any, Callable, Dict, List, Optional, Sequence, Tuple

import numpy as np

from . import _fin_primitives as _native
from . import fast as _f

Out = List[Tuple[str, np.ndarray]]

_EPS = np.finfo(float).eps


# ---------------------------------------------------------------------------------------
# parameters
# ---------------------------------------------------------------------------------------


def _num(val: Any, default: Any, name: str, *, integer: bool = False, gt: Optional[float] = 0, ge: Optional[float] = None) -> Any:
    """pandas-ta-classic's ``_pos_int`` / ``_pos_float``: ``None`` gives the default; a
    whole number keeps an int default's type, so the column names match pandas-ta's."""
    if val is None:
        return default
    ok = isinstance(val, Real) and not isinstance(val, bool) and math.isfinite(val)
    ok = ok and (not integer or float(val).is_integer())
    ok = ok and (gt is None or val > gt) and (ge is None or val >= ge)
    if not ok:
        kind = "an integer" if integer else "a number"
        bound = f" > {gt}" if gt is not None else (f" >= {ge}" if ge is not None else "")
        raise ValueError(f"{name} must be {kind}{bound}, got {val!r}")
    if integer or (isinstance(default, int) and not isinstance(default, bool) and float(val).is_integer()):
        return int(val)
    return float(val)


# ---------------------------------------------------------------------------------------
# array helpers (pandas semantics)
# ---------------------------------------------------------------------------------------


def _shift(a: np.ndarray, k: int) -> np.ndarray:
    out = np.full(a.shape, np.nan)
    if k == 0:
        return a.copy()
    if abs(k) >= a.size:
        return out
    if k > 0:
        out[k:] = a[:-k]
    else:
        out[:k] = a[-k:]
    return out


def _diff(a: np.ndarray, k: int = 1) -> np.ndarray:
    return a - _shift(a, k)


def _cumsum(a: np.ndarray) -> np.ndarray:
    """pandas ``cumsum``: a NaN reads NaN and the sum carries on past it."""
    nan = np.isnan(a)
    out = np.cumsum(np.where(nan, 0.0, a))
    out[nan] = np.nan
    return out


def _mask(values: np.ndarray, cond: np.ndarray, fill: float) -> np.ndarray:
    """pandas ``Series.mask(cond, fill)``."""
    return np.where(cond, fill, values)


def _div(num: np.ndarray, den: np.ndarray) -> np.ndarray:
    with np.errstate(divide="ignore", invalid="ignore"):
        return num / den


def _rsum(a: np.ndarray, n: int) -> np.ndarray:
    return _native.fast_rolling_sum(np.ascontiguousarray(a, dtype=np.float64), int(n))


def _rmax(a: np.ndarray, n: int) -> np.ndarray:
    return _native.fast_rolling_max(np.ascontiguousarray(a, dtype=np.float64), int(n))


def _rmin(a: np.ndarray, n: int) -> np.ndarray:
    return _native.fast_rolling_min(np.ascontiguousarray(a, dtype=np.float64), int(n))


def _sma(a: np.ndarray, n: int) -> np.ndarray:
    return _f.sma(np.ascontiguousarray(a, dtype=np.float64), int(n))


def _ema(a: np.ndarray, n: int) -> np.ndarray:
    return _f.ema(np.ascontiguousarray(a, dtype=np.float64), int(n))


def _rma(a: np.ndarray, n: int) -> np.ndarray:
    return _f.rma(np.ascontiguousarray(a, dtype=np.float64), int(n))


def _wma(a: np.ndarray, n: int) -> np.ndarray:
    return _native.fast_wma(np.ascontiguousarray(a, dtype=np.float64), int(n))


def _tr(h: np.ndarray, l: np.ndarray, c: np.ndarray) -> np.ndarray:
    return _f.true_range(h, l, c)


def _atr(h: np.ndarray, l: np.ndarray, c: np.ndarray, n: int) -> np.ndarray:
    return _f.atr(h, l, c, int(n))


def _lead_trim(arrays: Sequence[np.ndarray]) -> int:
    """Index of the first row where every array is finite (pandas-ta's
    ``skip_leading_nan``), or the length when there is none."""
    ok = np.ones(arrays[0].shape, dtype=bool)
    for a in arrays:
        ok &= np.isfinite(a)
    return int(ok.argmax()) if ok.any() else arrays[0].size


def _skip_leading_nan(fn: Callable[..., Out], arrays: Sequence[np.ndarray], *rest: Any, **kw: Any) -> Out:
    """Run ``fn`` on the rows from the first all-finite one and pad the result back."""
    n = arrays[0].size
    start = _lead_trim(arrays)
    if start == 0:
        return fn(*arrays, *rest, **kw)
    if start >= n:
        names = fn(*[np.full(400, 1.0) for _ in arrays], *rest, **kw)
        return [(name, np.full(n, np.nan)) for name, _ in names]
    out = fn(*[a[start:] for a in arrays], *rest, **kw)
    padded = []
    for name, v in out:
        full = np.full(n, np.nan)
        full[start:] = v
        padded.append((name, full))
    return padded


# ---------------------------------------------------------------------------------------
# overlap
# ---------------------------------------------------------------------------------------


def hl2(high, low) -> Out:
    return [("HL2", 0.5 * (high + low))]


def hlc3(high, low, close) -> Out:
    return [("HLC3", (high + low + close) / 3.0)]


def ohlc4(open_, high, low, close) -> Out:
    return [("OHLC4", 0.25 * (open_ + high + low + close))]


def wma(close, length=None) -> Out:
    n = _num(length, 10, "length", integer=True)
    return [(f"WMA_{n}", _wma(close, n))]


def hma(close, length=None) -> Out:
    n = _num(length, 10, "length", integer=True)
    half, root = int(n / 2), int(np.sqrt(n))
    return [(f"HMA_{n}", _wma(2 * _wma(close, half) - _wma(close, n), root))]


def dema(close, length=None) -> Out:
    n = _num(length, 10, "length", integer=True)
    e1 = _ema(close, n)
    e2 = _ema(e1, n)
    return [(f"DEMA_{n}", 2 * e1 - e2)]


def tema(close, length=None) -> Out:
    n = _num(length, 10, "length", integer=True)
    e1 = _ema(close, n)
    e2 = _ema(e1, n)
    e3 = _ema(e2, n)
    return [(f"TEMA_{n}", 3 * (e1 - e2) + e3)]


def t3(close, length=None, a=None) -> Out:
    n = _num(length, 10, "length", integer=True)
    a = _num(a, 0.7, "a")
    c1 = -a * a**2
    c2 = 3 * a**2 + 3 * a**3
    c3 = -6 * a**2 - 3 * a - 3 * a**3
    c4 = a**3 + 3 * a**2 + 3 * a + 1
    e = [close]
    for _ in range(6):
        e.append(_ema(e[-1], n))
    return [(f"T3_{n}_{a}", c1 * e[6] + c2 * e[5] + c3 * e[4] + c4 * e[3])]


def trima(close, length=None) -> Out:
    n = _num(length, 10, "length", integer=True)
    return [(f"TRIMA_{n}", _sma(_sma(close, n // 2 + 1), (n + 1) // 2))]


def zlma(close, length=None) -> Out:
    """Zero-lag EMA (``mamode="ema"``, pandas-ta's default)."""
    n = _num(length, 10, "length", integer=True)
    lag = int(0.5 * (n - 1))
    return [(f"ZL_EMA_{n}", _ema(2 * close - _shift(close, lag), n))]


def vwma(close, volume, length=None) -> Out:
    n = _num(length, 10, "length", integer=True)
    return [(f"VWMA_{n}", _sma(close * volume, n) / _sma(volume, n))]


def midpoint(close, length=None) -> Out:
    n = _num(length, 2, "length", integer=True)
    return [(f"MIDPOINT_{n}", 0.5 * (_rmin(close, n) + _rmax(close, n)))]


def midprice(high, low, length=None) -> Out:
    n = _num(length, 2, "length", integer=True)
    return [(f"MIDPRICE_{n}", 0.5 * (_rmin(low, n) + _rmax(high, n)))]


def linreg(close, length=None) -> Out:
    """Linear regression value at the last bar of each window (``LR_{length}``)."""
    n = _num(length, 14, "length", integer=True)
    out = np.full(close.size, np.nan)
    if n <= close.size:
        x = np.arange(n, dtype=float)
        x_sum = 0.5 * n * (n - 1)
        x2_sum = n * (n - 1) * (2 * n - 1) / 6
        divisor = n * x2_sum - x_sum * x_sum
        w = np.lib.stride_tricks.sliding_window_view(np.asarray(close, dtype=float), n)
        y_sums = w.sum(axis=1)
        xy_sums = (w * x).sum(axis=1)
        m = (n * xy_sums - x_sum * y_sums) / divisor
        b = (y_sums * x2_sum - x_sum * xy_sums) / divisor
        out[n - 1 :] = m * (n - 1) + b
    return [(f"LR_{n}", out)]


def supertrend(high, low, close, length=None, multiplier=None) -> Out:
    n = _num(length, 7, "length", integer=True)
    k = _num(multiplier, 3.0, "multiplier")
    return _skip_leading_nan(_supertrend, (high, low, close), n, k)


def _supertrend(high, low, close, n, k) -> Out:
    matr = k * _atr(high, low, close, n)
    mid = 0.5 * (high + low)
    d, trend, long, short = _native.fast_supertrend_loop(
        np.ascontiguousarray(close, dtype=np.float64), mid + matr, mid - matr
    )
    p = f"_{n}_{k}"
    return [(f"SUPERT{p}", trend), (f"SUPERTd{p}", d), (f"SUPERTl{p}", long), (f"SUPERTs{p}", short)]


def ichimoku(high, low, close, tenkan=None, kijun=None, senkou=None, include_chikou=None) -> Out:
    """Tenkan, kijun, both spans and (by default) the chikou span; the forward span rows
    past the last bar (pandas-ta's ``append_span``) are not included."""
    t = _num(tenkan, 9, "tenkan", integer=True)
    kj = _num(kijun, 26, "kijun", integer=True)
    s = _num(senkou, 52, "senkou", integer=True)
    ts = 0.5 * (_rmin(low, t) + _rmax(high, t))
    ks = 0.5 * (_rmin(low, kj) + _rmax(high, kj))
    span_a = _shift(0.5 * (ts + ks), kj)
    span_b = _shift(0.5 * (_rmin(low, s) + _rmax(high, s)), kj)
    out = [(f"ISA_{t}", span_a), (f"ISB_{kj}", span_b), (f"ITS_{t}", ts), (f"IKS_{kj}", ks)]
    if include_chikou is None or include_chikou:
        out.append((f"ICS_{kj}", _shift(close, -kj)))
    return out


# ---------------------------------------------------------------------------------------
# momentum
# ---------------------------------------------------------------------------------------


def mom(close, length=None) -> Out:
    n = _num(length, 10, "length", integer=True)
    return [(f"MOM_{n}", _diff(close, n))]


def roc(close, length=None, scalar=None) -> Out:
    n = _num(length, 10, "length", integer=True)
    s = _num(scalar, 100, "scalar")
    return [(f"ROC_{n}", s * _diff(close, n) / _shift(close, n))]


def cmo(close, length=None, scalar=None) -> Out:
    n = _num(length, 14, "length", integer=True)
    s = _num(scalar, 100, "scalar", gt=None)
    m = _diff(close, 1)
    pos = _rsum(np.where(m < 0, 0.0, m), n)
    neg = _rsum(np.abs(np.where(m > 0, 0.0, m)), n)
    den = pos + neg
    return [(f"CMO_{n}", _mask(_div(s * (pos - neg), den), den == 0, 0.0))]


def stoch(high, low, close, k=None, d=None, smooth_k=None) -> Out:
    k = _num(k, 14, "k", integer=True)
    d = _num(d, 3, "d", integer=True)
    sk = _num(smooth_k, 3, "smooth_k", integer=True)
    ll, hh = _rmin(low, k), _rmax(high, k)
    rng = hh - ll
    raw = _mask(_div(100 * (close - ll), rng), rng == 0, 0.0)
    stoch_k = _sma(raw, sk)
    stoch_d = _sma(stoch_k, d)
    p = f"_{k}_{d}_{sk}"
    return [(f"STOCHk{p}", stoch_k), (f"STOCHd{p}", stoch_d)]


def stochrsi(close, length=None, rsi_length=None, k=None, d=None) -> Out:
    n = _num(length, 14, "length", integer=True)
    rn = _num(rsi_length, 14, "rsi_length", integer=True)
    k = _num(k, 3, "k", integer=True)
    d = _num(d, 3, "d", integer=True)
    r = _f.rsi(close, rn)
    lo, hi = _rmin(r, n), _rmax(r, n)
    rng = hi - lo
    raw = _mask(_div(100 * (r - lo), rng), np.isclose(rng, 0, atol=1e-12), 0.0)
    sk = _sma(raw, k)
    sd = _sma(sk, d)
    p = f"_{n}_{rn}_{k}_{d}"
    return [(f"STOCHRSIk{p}", sk), (f"STOCHRSId{p}", sd)]


def willr(high, low, close, length=None) -> Out:
    n = _num(length, 14, "length", integer=True)
    ll, hh = _rmin(low, n), _rmax(high, n)
    rng = hh - ll
    out = 100 * (_div(close - ll, np.where(rng != 0, rng, np.nan)) - 1)
    return [(f"WILLR_{n}", _mask(out, rng == 0, 0.0))]


def cci(high, low, close, length=None, c=None) -> Out:
    n = _num(length, 14, "length", integer=True)
    c = _num(c, 0.015, "c")
    tp = (high + low + close) / 3.0
    mean = _sma(tp, n)
    mad_ = _mad(tp, n)
    den = c * mad_
    return [(f"CCI_{n}_{c}", _mask(_div(tp - mean, den), den == 0, 0.0))]


def ao(high, low, fast=None, slow=None) -> Out:
    f = _num(fast, 5, "fast", integer=True)
    s = _num(slow, 34, "slow", integer=True)
    if s < f:
        f, s = s, f
    mp = 0.5 * (high + low)
    return [(f"AO_{f}_{s}", _sma(mp, f) - _sma(mp, s))]


def apo(close, fast=None, slow=None) -> Out:
    f = _num(fast, 12, "fast", integer=True)
    s = _num(slow, 26, "slow", integer=True)
    if s < f:
        f, s = s, f
    return [(f"APO_{f}_{s}", _sma(close, f) - _sma(close, s))]


def ppo(close, fast=None, slow=None, signal=None, scalar=None) -> Out:
    f = _num(fast, 12, "fast", integer=True)
    s = _num(slow, 26, "slow", integer=True)
    g = _num(signal, 9, "signal", integer=True)
    sc = _num(scalar, 100, "scalar", gt=None)
    if s < f:
        f, s = s, f
    slow_ma = _sma(close, s)
    line = sc * (_sma(close, f) - slow_ma)
    line = line / slow_ma
    sig = _ema(line, g)
    p = f"_{f}_{s}_{g}"
    return [(f"PPO{p}", line), (f"PPOh{p}", line - sig), (f"PPOs{p}", sig)]


def trix(close, length=None, signal=None, scalar=None) -> Out:
    n = _num(length, 30, "length", integer=True)
    g = _num(signal, 9, "signal", integer=True)
    sc = _num(scalar, 100, "scalar", gt=None)
    e3 = _ema(_ema(_ema(close, n), n), n)
    line = sc * (e3 / _shift(e3, 1) - 1)
    return [(f"TRIX_{n}_{g}", line), (f"TRIXs_{n}_{g}", _sma(line, g))]


def tsi(close, fast=None, slow=None, signal=None, scalar=None) -> Out:
    f = _num(fast, 13, "fast", integer=True)
    s = _num(slow, 25, "slow", integer=True)
    g = _num(signal, 13, "signal", integer=True)
    sc = _num(scalar, 100, "scalar", gt=None)
    d = _diff(close, 1)
    num = _ema(_ema(d, s), f)
    den = _ema(_ema(np.abs(d), s), f)
    line = _mask(_div(sc * num, den), den == 0, 0.0)
    p = f"_{f}_{s}_{g}"
    return [(f"TSI{p}", line), (f"TSIs{p}", _ema(line, g))]


def uo(high, low, close, fast=None, medium=None, slow=None, fast_w=None, medium_w=None, slow_w=None) -> Out:
    f = _num(fast, 7, "fast", integer=True)
    m = _num(medium, 14, "medium", integer=True)
    s = _num(slow, 28, "slow", integer=True)
    fw = _num(fast_w, 4.0, "fast_w")
    mw = _num(medium_w, 2.0, "medium_w")
    sw = _num(slow_w, 1.0, "slow_w")
    pc = _shift(close, 1)
    bp = close - np.minimum(low, pc)
    tr = np.maximum(high, pc) - np.minimum(low, pc)

    def pressure(n):
        den = _rsum(tr, n)
        return _mask(_div(_rsum(bp, n), den), den == 0, 0.0)

    weights = fw * pressure(f) + mw * pressure(m) + sw * pressure(s)
    return [(f"UO_{f}_{m}_{s}", 100 * weights / (fw + mw + sw))]


def bop(open_, high, low, close, scalar=None) -> Out:
    sc = _num(scalar, 1, "scalar")
    out = _div(sc * (close - open_), high - low)
    return [("BOP", np.where(high != low, out, 0.0))]


def qqe(close, length=None, smooth=None, factor=None) -> Out:
    n = _num(length, 14, "length", integer=True)
    sm = _num(smooth, 5, "smooth", integer=True)
    fc = _num(factor, 4.236, "factor", gt=None)
    return _skip_leading_nan(_qqe, (close,), n, sm, fc)


def _qqe(close, n, sm, fc) -> Out:
    wl = 2 * n - 1
    rsi_ma = _ema(_f.rsi(close, n), sm)
    tr = np.abs(_diff(rsi_ma, 1))
    dar = fc * _ema(_ema(tr, wl), wl)
    long, short, trend, line, ql, qs = _native.fast_qqe_loop(rsi_ma, rsi_ma + dar, rsi_ma - dar)
    trend = np.where(np.isnan(line), np.nan, trend)
    p = f"_{n}_{sm}_{fc}"
    return [
        (f"QQE{p}", line),
        (f"QQE{p}_RSIMA", rsi_ma),
        (f"QQEl{p}", ql),
        (f"QQEs{p}", qs),
        (f"QQEb_l{p}", long),
        (f"QQEb_s{p}", short),
        (f"QQEd{p}", trend),
    ]


# ---------------------------------------------------------------------------------------
# trend
# ---------------------------------------------------------------------------------------


def _zero_small(a: np.ndarray) -> np.ndarray:
    return np.where(np.abs(a) < _EPS, 0.0, a)


def adx(high, low, close, length=None, lensig=None, scalar=None) -> Out:
    n = _num(length, 14, "length", integer=True)
    ls = _num(lensig, n, "lensig", integer=True)
    sc = _num(scalar, 100, "scalar", gt=None)
    up = high - _shift(high, 1)
    dn = _shift(low, 1) - low
    pos = _zero_small(((up > dn) & (up > 0)) * up)
    neg = _zero_small(((dn > up) & (dn > 0)) * dn)
    tr_s = _native.fast_wilder_sum(np.ascontiguousarray(_tr(high, low, close)), n)
    dmp = _mask(_div(sc * _native.fast_wilder_sum(pos, n), tr_s), tr_s == 0, 0.0)
    dmn = _mask(_div(sc * _native.fast_wilder_sum(neg, n), tr_s), tr_s == 0, 0.0)
    valid = ~np.isnan(tr_s)
    if valid.any():
        first = int(valid.argmax())
        dmp[first] = np.nan
        dmn[first] = np.nan
    den = dmp + dmn
    dx = _mask(_div(sc * np.abs(dmp - dmn), den), den == 0, 0.0)
    return [(f"ADX_{ls}", _rma(dx, ls)), (f"DMP_{n}", dmp), (f"DMN_{n}", dmn)]


def aroon(high, low, length=None, scalar=None) -> Out:
    n = _num(length, 14, "length", integer=True)
    sc = _num(scalar, 100, "scalar", gt=None)
    return _skip_leading_nan(_aroon, (high, low), n, sc)


def _aroon(high, low, n, sc) -> Out:
    since_hh = _native.fast_bars_since(np.ascontiguousarray(high), n + 1, True)
    since_ll = _native.fast_bars_since(np.ascontiguousarray(low), n + 1, False)
    up = sc * (1 - since_hh / n)
    down = sc * (1 - since_ll / n)
    return [(f"AROOND_{n}", down), (f"AROONU_{n}", up), (f"AROONOSC_{n}", up - down)]


def psar(high, low, close=None, af0=None, af=None, max_af=None) -> Out:
    a = _num(af, 0.02, "af")
    a0 = _num(af0, a, "af0")
    mx = _num(max_af, 0.2, "max_af")
    if close is None:
        return _skip_leading_nan(lambda h, l: _psar(h, l, None, a0, mx), (high, low))
    return _skip_leading_nan(lambda h, l, c: _psar(h, l, c, a0, mx), (high, low, close))


def _psar(high, low, close, a0, mx) -> Out:
    m = high.size
    falling = False
    if m > 1:
        up = high[1] - high[0]
        dn = low[0] - low[1]
        dmn = dn if (dn > up and dn > 0) else 0.0
        falling = (0.0 if abs(dmn) < _EPS else dmn) > 0
    if falling:
        sar, ep = high[0], (low[1] if m > 1 else low[0])
    else:
        sar, ep = low[0], (high[1] if m > 1 else high[0])
    if close is not None:
        sar = close[0]
    long, short, af_, rev = _native.fast_psar_loop(
        np.ascontiguousarray(high), np.ascontiguousarray(low), bool(falling), float(sar), float(ep), float(a0), float(mx)
    )
    if close is not None:
        both = np.where(~np.isnan(long), long, short)
        long = np.where(both < close, both, np.nan)
        short = np.where(both >= close, both, np.nan)
    missing = np.isnan(high) | np.isnan(low)
    af_, long, short, rev = (np.where(missing, np.nan, v) for v in (af_, long, short, rev))
    p = f"_{a0}_{mx}"
    return [(f"PSARl{p}", long), (f"PSARs{p}", short), (f"PSARaf{p}", af_), (f"PSARr{p}", rev)]


def vortex(high, low, close, length=None) -> Out:
    n = _num(length, 14, "length", integer=True)
    tr_sum = _rsum(_tr(high, low, close), n)
    vmp = _rsum(np.abs(high - _shift(low, 1)), n)
    vmm = _rsum(np.abs(low - _shift(high, 1)), n)

    def ddiv(a, b):
        return _mask(_div(a, b), (b == 0) & (a == 0), 0.0)

    return [(f"VTXP_{n}", ddiv(vmp, tr_sum)), (f"VTXM_{n}", ddiv(vmm, tr_sum))]


def chop(high, low, close, length=None, atr_length=None, scalar=None) -> Out:
    n = _num(length, 14, "length", integer=True)
    an = _num(atr_length, 1, "atr_length", integer=True)
    sc = _num(scalar, 100, "scalar")
    rng = _rmax(high, n) - _rmin(low, n)
    atr_sum = _rsum(_atr(high, low, close, an), n)
    with np.errstate(divide="ignore", invalid="ignore"):
        out = sc * (np.log10(atr_sum) - np.log10(rng)) / np.log10(n)
    return [(f"CHOP_{n}_{an}_{sc}", _mask(out, (rng == 0) & (atr_sum == 0), 0.0))]


def dpo(close, length=None, centered=None) -> Out:
    n = _num(length, 20, "length", integer=True)
    t = int(0.5 * n) + 1
    ma = _sma(close, n)
    if centered is None or centered:
        out = _shift(_shift(close, t) - ma, -t)
    else:
        out = close - _shift(ma, t)
    return [(f"DPO_{n}", out)]


# ---------------------------------------------------------------------------------------
# volatility
# ---------------------------------------------------------------------------------------


def natr(high, low, close, length=None, scalar=None) -> Out:
    n = _num(length, 14, "length", integer=True)
    sc = _num(scalar, 100, "scalar", gt=None)
    out = sc / close
    return [(f"NATR_{n}", out * _atr(high, low, close, n))]


def kc(high, low, close, length=None, scalar=None, tr=None) -> Out:
    """Keltner Channels with pandas-ta's EMA basis (``mamode="ema"``)."""
    n = _num(length, 20, "length", integer=True)
    sc = _num(scalar, 2, "scalar")
    rng = _tr(high, low, close) if (tr is None or tr) else high - low
    basis = _ema(close, n)
    band = _ema(rng, n)
    p = f"e_{n}_{sc}"
    return [(f"KCL{p}", basis - sc * band), (f"KCB{p}", basis), (f"KCU{p}", basis + sc * band)]


def donchian(high, low, lower_length=None, upper_length=None) -> Out:
    lo_n = _num(lower_length, 20, "lower_length", integer=True)
    up_n = _num(upper_length, 20, "upper_length", integer=True)
    lower = _rmin(low, lo_n)
    upper = _rmax(high, up_n)
    p = f"_{lo_n}_{up_n}"
    return [(f"DCL{p}", lower), (f"DCM{p}", 0.5 * (lower + upper)), (f"DCU{p}", upper)]


# ---------------------------------------------------------------------------------------
# statistics and performance
# ---------------------------------------------------------------------------------------


def _mad(values: np.ndarray, n: int) -> np.ndarray:
    values = np.asarray(values, dtype=np.float64)
    out = np.full(values.size, np.nan)
    if values.size >= n:
        w = np.lib.stride_tricks.sliding_window_view(values, n)
        means = w.mean(axis=1, keepdims=True)
        out[n - 1 :] = np.abs(w - means).mean(axis=1)
    return out


def mad(close, length=None) -> Out:
    n = _num(length, 30, "length", integer=True)
    return [(f"MAD_{n}", _mad(close, n))]


def zscore(close, length=None, std=None, ddof=None) -> Out:
    n = _num(length, 30, "length", integer=True, gt=1)
    sd = _num(std, 1, "std")
    dd = _num(ddof, 1, "ddof", integer=True, gt=None, ge=0)
    values = np.asarray(close, dtype=np.float64)
    out = np.full(values.size, np.nan)
    if values.size >= n:
        w = np.lib.stride_tricks.sliding_window_view(values, n)
        den = sd * w.std(axis=1, ddof=dd)
        with np.errstate(divide="ignore", invalid="ignore"):
            scores = (values[n - 1 :] - w.mean(axis=1)) / den
        out[n - 1 :] = np.where(np.isclose(den, 0, atol=1e-12), 0.0, scores)
    return [(f"ZS_{n}", out)]


def log_return(close, length=None, cumulative=None) -> Out:
    n = _num(length, 1, "length", integer=True)
    with np.errstate(divide="ignore", invalid="ignore"):
        if cumulative:
            return [(f"CUMLOGRET_{n}", np.log(close / close[0]))]
        return [(f"LOGRET_{n}", np.log(close / _shift(close, n)))]


def percent_return(close, length=None, cumulative=None) -> Out:
    n = _num(length, 1, "length", integer=True)
    if cumulative:
        return [(f"CUMPCTRET_{n}", close / close[0] - 1)]
    return [(f"PCTRET_{n}", close / _shift(close, n) - 1)]


# ---------------------------------------------------------------------------------------
# volume
# ---------------------------------------------------------------------------------------


def obv(close, volume) -> Out:
    return _skip_leading_nan(_obv, (close, volume))


def _obv(close, volume) -> Out:
    sign = _diff(close, 1)
    sign = np.where(sign > 0, 1.0, np.where(sign < 0, -1.0, sign))
    if sign.size:
        sign[0] = 1.0
    return [("OBV", _cumsum(sign * volume))]


def mfi(high, low, close, volume, length=None) -> Out:
    n = _num(length, 14, "length", integer=True)
    return _skip_leading_nan(_mfi, (high, low, close, volume), n)


def _mfi(high, low, close, volume, n) -> Out:
    tp = (high + low + close) / 3.0
    flow = tp * volume
    ch = _diff(tp, 1)
    pos = np.where(ch > 0, flow, 0.0)
    neg = np.where(ch < 0, flow, 0.0)
    pos[:1] = np.nan
    neg[:1] = np.nan
    ps, ns = _rsum(pos, n), _rsum(neg, n)
    den = ps + ns
    out = _mask(_div(100 * ps, den), den == 0, 0.0)
    missing = np.isnan(high) | np.isnan(low) | np.isnan(close) | np.isnan(volume)
    return [(f"MFI_{n}", np.where(missing, np.nan, out))]


def _ad_line(high, low, close, volume) -> np.ndarray:
    rng = high - low
    clv = 2 * close - (high + low)
    ad_ = _mask(clv * _div(volume, np.where(rng != 0, rng, np.nan)), rng == 0, 0.0)
    return ad_


def ad(high, low, close, volume) -> Out:
    return [("AD", _cumsum(_ad_line(high, low, close, volume)))]


def adosc(high, low, close, volume, fast=None, slow=None) -> Out:
    f = _num(fast, 3, "fast", integer=True)
    s = _num(slow, 10, "slow", integer=True)
    n = high.size
    ok = np.isfinite(high) & np.isfinite(low) & np.isfinite(close) & np.isfinite(volume)
    out = np.full(n, np.nan)
    if ok.any():
        idx = np.flatnonzero(ok)
        line = _cumsum(_ad_line(high[idx], low[idx], close[idx], volume[idx]))
        out[idx] = _native.fast_adosc_loop(np.ascontiguousarray(line), f, s)
    return [(f"ADOSC_{f}_{s}", out)]


def cmf(high, low, close, volume, length=None) -> Out:
    n = _num(length, 20, "length", integer=True)
    return [(f"CMF_{n}", _rsum(_ad_line(high, low, close, volume), n) / _rsum(volume, n))]


def efi(close, volume, length=None) -> Out:
    n = _num(length, 13, "length", integer=True)
    return [(f"EFI_{n}", _ema(_diff(close, 1) * volume, n))]


def pvt(close, volume) -> Out:
    return [("PVT", _cumsum(100 * _diff(close, 1) / _shift(close, 1) * volume))]


# ---------------------------------------------------------------------------------------
# registry: method name -> (input columns, function, outputs one column or several)
# ---------------------------------------------------------------------------------------

C, HL, HLC, HLCV, CV, OHLC = ("close",), ("high", "low"), ("high", "low", "close"), ("high", "low", "close", "volume"), ("close", "volume"), ("open", "high", "low", "close")

SPEC: Dict[str, Tuple[Tuple[str, ...], Callable[..., Out]]] = {
    # overlap
    "hl2": (HL, hl2),
    "hlc3": (HLC, hlc3),
    "ohlc4": (OHLC, ohlc4),
    "wma": (C, wma),
    "hma": (C, hma),
    "dema": (C, dema),
    "tema": (C, tema),
    "t3": (C, t3),
    "trima": (C, trima),
    "zlma": (C, zlma),
    "vwma": (CV, vwma),
    "midpoint": (C, midpoint),
    "midprice": (HL, midprice),
    "linreg": (C, linreg),
    "supertrend": (HLC, supertrend),
    "ichimoku": (HLC, ichimoku),
    # momentum
    "mom": (C, mom),
    "roc": (C, roc),
    "cmo": (C, cmo),
    "stoch": (HLC, stoch),
    "stochrsi": (C, stochrsi),
    "willr": (HLC, willr),
    "cci": (HLC, cci),
    "ao": (HL, ao),
    "apo": (C, apo),
    "ppo": (C, ppo),
    "trix": (C, trix),
    "tsi": (C, tsi),
    "uo": (HLC, uo),
    "bop": (OHLC, bop),
    "qqe": (C, qqe),
    # trend
    "adx": (HLC, adx),
    "aroon": (HL, aroon),
    "psar": (HLC, psar),
    "vortex": (HLC, vortex),
    "chop": (HLC, chop),
    "dpo": (C, dpo),
    # volatility
    "natr": (HLC, natr),
    "kc": (HLC, kc),
    "donchian": (HL, donchian),
    # statistics and performance
    "mad": (C, mad),
    "zscore": (C, zscore),
    "log_return": (C, log_return),
    "percent_return": (C, percent_return),
    # volume
    "obv": (CV, obv),
    "mfi": (HLCV, mfi),
    "ad": (HLCV, ad),
    "adosc": (HLCV, adosc),
    "cmf": (HLCV, cmf),
    "efi": (CV, efi),
    "pvt": (CV, pvt),
}

# Indicators whose result is a frame of several columns in pandas-ta.
MULTI = {"supertrend", "ichimoku", "stoch", "stochrsi", "ppo", "trix", "tsi", "qqe", "adx", "aroon", "psar", "vortex", "kc", "donchian"}


def run(name: str, columns: Dict[str, np.ndarray], **params: Any) -> Out:
    """Run indicator ``name`` on float64 ``columns`` (keyed open/high/low/close/volume)."""
    inputs, fn = SPEC[name]
    args = [np.ascontiguousarray(columns[c], dtype=np.float64) for c in inputs]
    return fn(*args, **params)
