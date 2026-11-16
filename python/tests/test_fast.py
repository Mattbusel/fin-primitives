"""``fin_primitives.fast``: whole-series float64 indicators.

Two layers of checks:

* against plain numpy references written here (always run), and
* against pandas-ta-classic 0.9.0, the library ``fast`` follows (run when it is installed;
  it needs Python 3.11+): the same NaN positions and values within 1e-12 relative.
"""
import math

import pytest

np = pytest.importorskip("numpy")

from fin_primitives import fast  # noqa: E402

REL = 1e-12


def walk(n, seed=7, vol=0.01, start=100.0, cents=True):
    rng = np.random.default_rng(seed)
    close = start * np.exp(np.cumsum(rng.normal(0, vol, n)))
    high = close * (1 + np.abs(rng.normal(0, 0.003, n)))
    low = close * (1 - np.abs(rng.normal(0, 0.003, n)))
    if cents:
        close, high, low = close.round(2), high.round(2), low.round(2)
    return high, low, close


def real_like(n=6000, seed=11):
    """Regime changes, jumps, flat runs (a halted or illiquid symbol) and a price level
    that wanders over several orders of magnitude."""
    rng = np.random.default_rng(seed)
    vol = np.where(np.arange(n) % 1500 < 750, 0.002, 0.03)
    ret = rng.normal(0, 1, n) * vol
    ret[rng.integers(0, n, 20)] += rng.normal(0, 0.15, 20)
    ret += np.linspace(0, -0.004, n)  # long decline, from 50 to about 0.3
    close = 50 * np.exp(np.cumsum(ret))
    for s in (1000, 2500, 4000):
        close[s : s + 60] = close[s]
    close = close.round(4)
    high = np.maximum(close * (1 + np.abs(rng.normal(0, 0.004, n))), close)
    low = np.minimum(close * (1 - np.abs(rng.normal(0, 0.004, n))), close)
    return high.round(4), low.round(4), close


def assert_same(got, want, scale=None):
    got = np.asarray(got, dtype=float)
    want = np.asarray(want, dtype=float)
    assert got.shape == want.shape
    np.testing.assert_array_equal(np.isnan(got), np.isnan(want), err_msg="NaN positions differ")
    m = ~np.isnan(want)
    if not m.any():
        return
    tol = REL * np.abs(want[m])
    if scale is not None:
        # Rolling deviations of a nearly flat window: compare against the price level.
        tol = np.maximum(tol, REL * np.asarray(scale, dtype=float)[m])
    diff = np.abs(got[m] - want[m])
    worst = int(np.argmax(diff - tol))
    assert np.all(diff <= tol), f"index {np.flatnonzero(m)[worst]}: {got[m][worst]!r} vs {want[m][worst]!r}"


# ---------------------------------------------------------------------------
# numpy references (always run)
# ---------------------------------------------------------------------------


def ref_sma(x, p):
    out = np.full(len(x), np.nan)
    for i in range(p - 1, len(x)):
        out[i] = x[i - p + 1 : i + 1].mean()
    return out


def ref_ema(x, p, alpha=None):
    out = np.full(len(x), np.nan)
    if len(x) < p:
        return out
    a = 2 / (p + 1) if alpha is None else alpha
    out[p - 1] = x[:p].mean()
    for i in range(p, len(x)):
        out[i] = a * x[i] + (1 - a) * out[i - 1]
    return out


def ref_std(x, p, ddof):
    out = np.full(len(x), np.nan)
    for i in range(p - 1, len(x)):
        out[i] = x[i - p + 1 : i + 1].std(ddof=ddof)
    return out


def ref_tr(h, l, c):
    tr = np.full(len(c), np.nan)
    for i in range(1, len(c)):
        tr[i] = max(h[i] - l[i], abs(h[i] - c[i - 1]), abs(c[i - 1] - l[i]))
    return tr


@pytest.mark.parametrize("period", [1, 2, 5, 20, 200])
def test_sma_ema_against_numpy(period):
    _, _, c = walk(3000)
    assert_same(fast.sma(c, period), ref_sma(c, period))
    assert_same(fast.ema(c, period), ref_ema(c, period))
    assert_same(fast.rma(c, period), ref_ema(c, period, alpha=1 / period))


@pytest.mark.parametrize("period", [2, 14, 50])
def test_rsi_against_numpy(period):
    _, _, c = walk(3000)
    d = np.diff(c, prepend=np.nan)
    g = ref_ema(np.where(d > 0, d, 0.0)[1:], period, alpha=1 / period)
    loss = ref_ema(np.where(d < 0, -d, 0.0)[1:], period, alpha=1 / period)
    want = np.concatenate([[np.nan], 100 * g / (g + loss)])
    assert_same(fast.rsi(c, period), want)


@pytest.mark.parametrize("ddof", [0, 1])
@pytest.mark.parametrize("period", [2, 20, 100])
def test_stdev_against_numpy(period, ddof):
    for h, l, c in (walk(3000), real_like()):
        assert_same(fast.stdev(c, period, ddof), ref_std(c, period, ddof), scale=c)
        assert_same(fast.variance(c, period, ddof), ref_std(c, period, ddof) ** 2, scale=c * c)


def test_atr_against_numpy_both_seeds():
    h, l, c = walk(2000)
    tr = ref_tr(h, l, c)
    want = np.concatenate([[np.nan], ref_ema(tr[1:], 14, alpha=1 / 14)])
    assert_same(fast.atr(h, l, c, 14), want)
    assert_same(fast.true_range(h, l, c), tr)
    # TradingView: bar 0's true range is high - low and the seed is the mean of bars 0..13.
    tr_tv = tr.copy()
    tr_tv[0] = h[0] - l[0]
    assert_same(fast.atr(h, l, c, 14, seed="tradingview"), ref_ema(tr_tv, 14, alpha=1 / 14))
    assert_same(fast.true_range(h, l, c, seed="tradingview"), tr_tv)


def test_macd_against_numpy():
    _, _, c = walk(2000)
    line, sig, hist = fast.macd(c, 12, 26, 9)
    f = ref_ema(c, 12)
    s = ref_ema(c, 26)
    want_line = f - s
    want_sig = np.full(len(c), np.nan)
    want_sig[25:] = ref_ema(want_line[25:], 9)
    assert_same(line, want_line)
    assert_same(sig, want_sig)
    assert_same(hist, want_line - want_sig)
    assert np.isnan(line[24]) and not np.isnan(line[25])
    assert np.isnan(sig[32]) and not np.isnan(sig[33])


def test_bbands_against_numpy():
    _, _, c = walk(2000)
    lower, mid, upper = fast.bbands(c, 20, 2.0)
    sd = ref_std(c, 20, 0)
    assert_same(mid, ref_sma(c, 20))
    assert_same(lower, ref_sma(c, 20) - 2 * sd)
    assert_same(upper, ref_sma(c, 20) + 2 * sd)


def test_flat_window_reads_exact_zero():
    c = np.r_[np.linspace(10, 11, 30), np.full(40, 0.42), np.linspace(0.42, 1, 30)]
    sd = fast.stdev(c, 20)
    assert np.all(sd[49:70] == 0.0)
    assert fast.rsi(np.full(30, 5.0), 14)[20] == 0.0


def test_volatile_then_calm_keeps_precision():
    # Running sums built in a volatile stretch must not swamp a calm one.
    rng = np.random.default_rng(3)
    c = np.r_[1000 + rng.normal(0, 50, 500), 1000 + rng.normal(0, 1e-4, 500)]
    assert_same(fast.stdev(c, 20), ref_std(c, 20, 0))


def test_long_series_does_not_drift():
    _, _, c = walk(200_000, seed=5)
    tail = slice(-500, None)
    assert_same(fast.sma(c, 50)[tail], ref_sma(c, 50)[tail])
    assert_same(fast.stdev(c, 50)[tail], ref_std(c[-600:], 50, 0)[-500:])


# ---------------------------------------------------------------------------
# edge cases, input types and argument checks
# ---------------------------------------------------------------------------


@pytest.mark.parametrize("n", [0, 1, 5, 13, 14])
def test_short_input_is_all_nan(n):
    h, l, c = walk(max(n, 1))
    h, l, c = h[:n], l[:n], c[:n]
    for out in (fast.sma(c, 20), fast.ema(c, 20), fast.rsi(c, 14), fast.atr(h, l, c, 14), fast.stdev(c, 20)):
        assert len(out) == n and np.all(np.isnan(out))
    for out in (*fast.macd(c), *fast.bbands(c)):
        assert len(out) == n and np.all(np.isnan(out))


def test_period_equal_to_length():
    _, _, c = walk(20)
    sma = fast.sma(c, 20)
    assert np.all(np.isnan(sma[:19])) and sma[19] == pytest.approx(c.mean(), rel=1e-15)
    assert np.all(np.isnan(fast.rsi(c, 20)))  # 20 closes give only 19 changes


def test_nan_input():
    _, _, c = walk(200)
    c = c.copy()
    c[:5] = np.nan  # leading run: the seed starts after it
    c[100] = np.nan  # interior
    sma = fast.sma(c, 10)
    assert np.all(np.isnan(sma[:14])) and not np.isnan(sma[14])
    assert np.all(np.isnan(sma[100:110])) and not np.isnan(sma[110])
    ema = fast.ema(c, 10)
    assert np.all(np.isnan(ema[:14])) and not np.isnan(ema[14])
    assert np.isnan(ema[100]) and not np.isnan(ema[101])
    line, sig, hist = fast.macd(c)
    assert np.isnan(line[100]) and not np.isnan(line[101])
    assert np.all(np.isnan(fast.sma(np.full(50, np.nan), 5)))
    assert np.all(np.isnan(fast.rsi(np.full(50, np.nan), 5)))


def test_pandas_in_pandas_out():
    pd = pytest.importorskip("pandas")
    _, _, c = walk(300)
    s = pd.Series(c, index=pd.date_range("2024-01-01", periods=300, freq="min"))
    out = fast.rsi(s, 14)
    assert isinstance(out, pd.Series) and out.index.equals(s.index)
    lower, mid, upper = fast.bbands(s)
    assert all(isinstance(x, pd.Series) for x in (lower, mid, upper))
    np.testing.assert_array_equal(out.to_numpy(), fast.rsi(c, 14))


def test_list_int_and_strided_input():
    _, _, c = walk(300)
    np.testing.assert_array_equal(fast.sma(list(c), 5), fast.sma(c, 5))
    np.testing.assert_array_equal(fast.sma(c[::2], 5), fast.sma(c[::2].copy(), 5))
    assert fast.sma(np.arange(10), 2)[1] == 0.5


def test_argument_errors():
    _, _, c = walk(50)
    with pytest.raises(ValueError):
        fast.sma(c, 0)
    with pytest.raises(ValueError):
        fast.stdev(c, 1)
    with pytest.raises(ValueError):
        fast.stdev(c, 5, ddof=5)
    with pytest.raises(ValueError):
        fast.bbands(c, 20, -1.0)
    with pytest.raises(ValueError):
        fast.atr(c, c, c, 14, seed="pine")
    with pytest.raises(ValueError):
        fast.atr(c[:-1], c, c, 14)
    with pytest.raises(ValueError):
        fast.sma(np.ones((3, 3)), 2)
    with pytest.raises((OverflowError, TypeError, ValueError)):
        fast.sma(c, -1)


# ---------------------------------------------------------------------------
# pandas-ta-classic parity (the library fin_primitives.fast follows)
# ---------------------------------------------------------------------------


@pytest.fixture(scope="module")
def ta():
    pytest.importorskip("pandas")
    if not hasattr(np, "NaN"):
        np.NaN = np.nan  # older pandas-ta releases still reference it
    return pytest.importorskip("pandas_ta_classic")


def _series():
    import pandas as pd

    cases = {"walk": walk(20_000), "real_like": real_like(), "unrounded": walk(5000, seed=2, cents=False)}
    for name, (h, l, c) in cases.items():
        yield name, pd.Series(h), pd.Series(l), pd.Series(c)


@pytest.mark.parametrize("period", [2, 5, 14, 20, 50, 200])
def test_parity_single_output(ta, period):
    for name, h, l, c in _series():
        assert_same(fast.sma(c, period), ta.sma(c, period))
        assert_same(fast.ema(c, period), ta.ema(c, period))
        assert_same(fast.rma(c, period), ta.rma(c, period))
        assert_same(fast.rsi(c, period), ta.rsi(c, period))
        assert_same(fast.atr(h, l, c, period), ta.atr(h, l, c, period))
        assert_same(fast.true_range(h, l, c), ta.true_range(h, l, c))
        for ddof in (0, 1):
            assert_same(fast.stdev(c, period, ddof), ta.stdev(c, period, ddof=ddof), scale=c)


@pytest.mark.parametrize("params", [(12, 26, 9), (5, 35, 5), (3, 10, 16), (26, 12, 9)])
def test_parity_macd(ta, params):
    for name, h, l, c in _series():
        want = ta.macd(c, *params)
        line, sig, hist = fast.macd(c, *params)
        assert_same(line, want.iloc[:, 0])
        assert_same(hist, want.iloc[:, 1])
        assert_same(sig, want.iloc[:, 2])


@pytest.mark.parametrize("period,k,ddof", [(20, 2.0, 0), (5, 1.5, 0), (50, 2.5, 1)])
def test_parity_bbands(ta, period, k, ddof):
    for name, h, l, c in _series():
        want = ta.bbands(c, period, k, ddof=ddof)
        for got, col in zip(fast.bbands(c, period, k, ddof), range(3)):
            assert_same(got, want.iloc[:, col], scale=c)


def test_parity_with_nan_input(ta):
    import pandas as pd

    h, l, c = (pd.Series(v) for v in walk(3000))
    for s in (h, l, c):
        s.iloc[:7] = np.nan
    c.iloc[[500, 501, 1700]] = np.nan
    h.iloc[900] = np.nan
    assert_same(fast.sma(c, 20), ta.sma(c, 20))
    assert_same(fast.ema(c, 20), ta.ema(c, 20))
    assert_same(fast.rsi(c, 14), ta.rsi(c, 14))
    assert_same(fast.atr(h, l, c, 14), ta.atr(h, l, c, 14))
    assert_same(fast.stdev(c, 20), ta.stdev(c, 20), scale=c.ffill().bfill())
    want = ta.macd(c, 12, 26, 9)
    line, sig, hist = fast.macd(c, 12, 26, 9)
    assert_same(line, want.iloc[:, 0])
    assert_same(hist, want.iloc[:, 1])
    assert_same(sig, want.iloc[:, 2])
    want = ta.bbands(c, 20, 2.0)
    for got, col in zip(fast.bbands(c, 20, 2.0), range(3)):
        assert_same(got, want.iloc[:, col], scale=c.ffill().bfill())


@pytest.mark.parametrize("n", [3, 10, 25, 26, 33, 34])
def test_parity_short_input(ta, n):
    import pandas as pd

    h, l, c = (pd.Series(v) for v in walk(n))
    assert_same(fast.sma(c, 20), ta.sma(c, 20))
    assert_same(fast.ema(c, 20), ta.ema(c, 20))
    assert_same(fast.rsi(c, 14), ta.rsi(c, 14))
    assert_same(fast.atr(h, l, c, 14), ta.atr(h, l, c, 14))
    want = ta.macd(c, 12, 26, 9)
    line, sig, hist = fast.macd(c, 12, 26, 9)
    assert_same(line, want.iloc[:, 0])
    assert_same(sig, want.iloc[:, 2])
    want = ta.bbands(c, 20, 2.0)
    for got, col in zip(fast.bbands(c, 20, 2.0), range(3)):
        assert_same(got, want.iloc[:, col])


def test_math_is_finite_after_warmup():
    _, _, c = walk(1000)
    assert all(math.isfinite(v) for v in fast.rsi(c, 14)[14:])
